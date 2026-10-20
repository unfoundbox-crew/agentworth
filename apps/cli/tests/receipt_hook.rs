//! The pre-push hook and `archie repo receipt`, end to end through the real binary and a real
//! `git push` to a bare remote (Evidence slice 1, spec §1 mechanics and §6.2 "Hook").
//!
//! Every test runs with its own `HOME`, so the index, `config.toml` (consent) and the receipt
//! key all live in a temp dir and nothing touches the developer's own `~/.agentworth`.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Sandbox {
    _dir: tempfile::TempDir,
    home: PathBuf,
    bin: PathBuf,
    remote: PathBuf,
    repo: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let home = root.join("home");
        let bin = root.join("bin");
        let remote = root.join("remote.git");
        let repo = root.join("repo");
        for d in [&home, &bin, &repo] {
            std::fs::create_dir_all(d).unwrap();
        }
        // `archie` and `agentworth` on PATH, as the hook expects, pointing at this build.
        for (name, target) in [
            ("archie", assert_cmd::cargo::cargo_bin("archie")),
            ("agentworth", assert_cmd::cargo::cargo_bin("agentworth")),
        ] {
            std::os::unix::fs::symlink(target, bin.join(name)).unwrap();
        }
        let sb = Self {
            _dir: dir,
            home,
            bin,
            remote,
            repo,
        };
        sb.git_at(
            sb.remote.parent().unwrap(),
            &["init", "-q", "--bare", "-b", "main", "remote.git"],
        );
        sb.git(&["init", "-q", "-b", "main"]);
        sb.git(&["config", "user.email", "test@example.invalid"]);
        sb.git(&["config", "user.name", "Test"]);
        sb.git(&["config", "commit.gpgsign", "false"]);
        sb.git(&["remote", "add", "origin", &sb.remote.to_string_lossy()]);
        sb
    }

    fn env(&self, c: &mut Command) {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        c.env("HOME", &self.home)
            .env("PATH", path)
            .env("NO_COLOR", "1")
            .env_remove("AGENTWORTH_RECEIPT_KEY")
            .env_remove("AGENTWORTH_CONFIG_PATH")
            .env_remove("AGENTWORTH_RECEIPT_PUSH")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE");
    }

    fn run_git(&self, dir: &Path, args: &[&str]) -> Output {
        let mut c = Command::new("git");
        c.arg("-C").arg(dir).args(args);
        self.env(&mut c);
        c.output().expect("run git")
    }

    fn git_at(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.run_git(dir, args);
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_at(&self.repo.clone(), args)
    }

    fn archie(&self, args: &[&str]) -> Output {
        let mut c = Command::new(self.bin.join("archie"));
        c.current_dir(&self.repo).args(args);
        self.env(&mut c);
        c.output().expect("run archie")
    }

    fn commit(&self, rel: &str, body: &str) -> String {
        std::fs::write(self.repo.join(rel), body).unwrap();
        self.git(&["add", rel]);
        self.git(&["commit", "-q", "-m", &format!("add {rel}")]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn install_hook(&self) {
        let out = self.archie(&["repo", "suspect", "--hook"]);
        assert!(out.status.success());
        let hook = self.repo.join(".git/hooks/pre-push");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, &out.stdout).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn remote_notes(&self) -> String {
        let out = self.run_git(&self.remote, &["notes", "--ref", "agentworth", "list"]);
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn push(&self) -> Output {
        self.run_git(&self.repo.clone(), &["push", "origin", "main"])
    }
}

#[test]
fn without_consent_the_hook_exports_nothing_and_the_push_goes_through() {
    let sb = Sandbox::new();
    sb.install_hook();
    let tip = sb.commit("a.txt", "a\n");
    let out = sb.push();
    assert!(
        out.status.success(),
        "push must never be blocked: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(sb.git_at(&sb.remote, &["rev-parse", "main"]), tip);
    assert!(sb.remote_notes().is_empty(), "no consent, no note");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("receipt"), "one line says why: {stderr}");
}

#[test]
fn init_write_then_push_signs_and_pushes_a_note_once() {
    let sb = Sandbox::new();
    sb.commit("a.txt", "a\n");
    let init = sb.archie(&["repo", "receipt", "init", "--write"]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let key = sb.home.join(".agentworth/keys/receipt_ed25519");
    assert!(key.exists() && sb.repo.join(".github/workflows/agentworth.yml").exists());

    let tip = sb.commit("b.txt", "b\n");
    let out = sb.push();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("signed"), "{stderr}");

    // The note is on the tip, on the remote, exactly once: the nested push of the notes ref
    // did not re-run the hook (it would have looped or doubled the work).
    let notes = sb.remote_notes();
    assert_eq!(notes.lines().count(), 1, "{notes}");
    assert!(notes.ends_with(&tip), "{notes}");
    assert_eq!(stderr.matches("signed").count(), 1, "{stderr}");

    // `show` by commit decodes it and recognises this machine's key.
    let show = sb.archie(&["repo", "receipt", "show", &tip, "--json"]);
    assert!(
        show.status.success(),
        "{}",
        String::from_utf8_lossy(&show.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(v[0]["signature"], "key_not_linked");
    assert_eq!(v[0]["signed_by_this_machine"], true);
    assert_eq!(v[0]["commits_present"], 2);

    // `key --github` prints the signing-key line for the same key.
    let gh = sb.archie(&["repo", "receipt", "key", "--github"]);
    let gh = String::from_utf8_lossy(&gh.stdout);
    assert!(
        gh.contains("gh ssh-key add") && gh.contains("--type signing"),
        "{gh}"
    );
}

#[test]
fn a_rejected_notes_push_never_fails_the_real_push() {
    let sb = Sandbox::new();
    sb.commit("a.txt", "a\n");
    assert!(sb
        .archie(&["repo", "receipt", "init", "--write"])
        .status
        .success());
    // The remote refuses any notes ref.
    let hook = sb.remote.join("hooks/pre-receive");
    std::fs::write(
        &hook,
        "#!/bin/sh\nwhile read old new ref; do case \"$ref\" in refs/notes/*) echo 'no notes here' >&2; exit 1;; esac; done\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    let tip = sb.commit("b.txt", "b\n");
    let out = sb.push();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(sb.git_at(&sb.remote, &["rev-parse", "main"]), tip);
    assert!(sb.remote_notes().is_empty());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not pushed"), "{stderr}");
}

#[test]
fn create_json_without_consent_previews_and_signs_nothing() {
    let sb = Sandbox::new();
    sb.commit("a.txt", "a\n");
    // A key but no consent: `init --write` makes both, then the consent is taken back.
    let init = sb.archie(&["repo", "receipt", "init", "--write"]);
    assert!(init.status.success());
    std::fs::remove_file(sb.home.join(".agentworth/config.toml")).unwrap();

    let out = sb.archie(&["repo", "receipt", "create", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["signed"], false);
    assert_eq!(v["skipped"], "consent_missing");
    assert!(v["statement"]["predicate"]["sessions"].is_array());
    assert!(v["reveals"].as_array().is_some_and(|r| !r.is_empty()));
    let notes = sb.run_git(&sb.repo, &["notes", "--ref", "agentworth", "list"]);
    assert!(String::from_utf8_lossy(&notes.stdout).trim().is_empty());
}

#[test]
fn create_without_a_key_is_one_line_in_quiet_mode_and_an_error_otherwise() {
    let sb = Sandbox::new();
    sb.commit("a.txt", "a\n");
    let quiet = sb.archie(&["repo", "receipt", "create", "--quiet"]);
    assert!(quiet.status.success());
    assert!(String::from_utf8_lossy(&quiet.stderr).contains("receipt init"));
    let loud = sb.archie(&["repo", "receipt", "create"]);
    assert!(!loud.status.success());
}

#[test]
fn the_push_guard_makes_create_a_no_op() {
    let sb = Sandbox::new();
    sb.commit("a.txt", "a\n");
    let mut c = Command::new(sb.bin.join("archie"));
    c.current_dir(&sb.repo).args(["repo", "receipt", "create"]);
    sb.env(&mut c);
    c.env("AGENTWORTH_RECEIPT_PUSH", "1");
    let out = c.output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}
