//! The AgentWorth / Evidence GitHub Action (`action.yml`, `action/evidence.sh`), end to end
//! against fixture repositories (spec `docs/specs/evidence-check.md` §6.2 "Action e2e").
//!
//! Each test builds a real author repository, signs a receipt with the real binary, pushes the
//! branch and the notes ref to a bare "GitHub" remote (or a fork), then runs the Action's
//! script in a fresh clone exactly as the composite step does: same environment variables,
//! a GitHub event payload on disk, and `gh` replaced by `tests/fixtures/action/fake-gh.sh`,
//! which answers API calls from files and records what the Action posts. The posted check-run
//! body is snapshotted after the hex and fingerprints that change per run are masked.
//!
//! Set `UPDATE_SNAPSHOTS=1` to rewrite the snapshots.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const REPO: &str = "octo-org/widget";
const AUTHOR: &str = "octo-dev";
const FORKER: &str = "forker";
const RUN_ID: &str = "77";
const SELF_SUITE: u64 = 9;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn action_script() -> PathBuf {
    manifest_dir().join("../../action/evidence.sh")
}

/// The Action needs bash, jq, git and curl on the runner; skip (loudly) where they are missing.
fn tools_present() -> bool {
    ["bash", "jq", "git", "curl", "tar"].iter().all(|t| {
        let ok = Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {t}"))
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("skipping: {t} is not on PATH");
        }
        ok
    })
}

/// Same key rule as fake-gh.sh.
fn route_key(method: &str, path: &str) -> String {
    format!("{method} {path}")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum PushTo {
    Origin,
    Fork,
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    bin: PathBuf,
    origin: PathBuf,
    fork: PathBuf,
    dev: PathBuf,
    runner: PathBuf,
    fake: PathBuf,
    base: String,
    head: String,
}

impl Fixture {
    fn new(push_to: PushTo) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let home = root.join("home");
        let bin = root.join("bin");
        let github = root.join("github");
        let origin = github.join("octo-org/widget.git");
        let fork = github.join(format!("{FORKER}/widget.git"));
        let dev = root.join("dev");
        let runner = root.join("runner");
        let fake = root.join("fake-gh");
        for d in [&home, &bin, &dev, &fake.join("routes"), &origin, &fork] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::os::unix::fs::symlink(assert_cmd::cargo::cargo_bin("archie"), bin.join("archie"))
            .unwrap();
        let gh = bin.join("gh-fake");
        std::fs::copy(manifest_dir().join("tests/fixtures/action/fake-gh.sh"), &gh).unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut fx = Self {
            _dir: dir,
            root,
            home,
            bin,
            origin,
            fork,
            dev,
            runner,
            fake,
            base: String::new(),
            head: String::new(),
        };
        for bare in [&fx.origin, &fx.fork] {
            fx.git_in(bare, &["init", "-q", "--bare", "-b", "main"]);
        }

        // The author: main on GitHub, a consented repo, a feature branch with two commits and a
        // receipt for them in refs/notes/agentworth.
        fx.git(&["init", "-q", "-b", "main"]);
        fx.git(&["config", "user.email", "dev@example.invalid"]);
        fx.git(&["config", "user.name", "Dev"]);
        fx.git(&["config", "commit.gpgsign", "false"]);
        fx.git(&["remote", "add", "origin", &fx.origin.to_string_lossy()]);
        fx.git(&["remote", "add", "fork", &fx.fork.to_string_lossy()]);
        fx.commit("README.md", "widget\n", "2026-10-01T10:00:00Z");
        fx.git(&["push", "-q", "origin", "main"]);
        fx.base = fx.git(&["rev-parse", "HEAD"]);
        let init = fx.archie(&["repo", "receipt", "init", "--write"]);
        assert!(init.status.success(), "{}", stderr(&init));
        // init --write leaves the workflow for the author to commit; not part of this PR.
        std::fs::remove_dir_all(fx.dev.join(".github")).unwrap();
        fx.git(&["checkout", "-q", "-b", "feature"]);
        fx.commit("src.txt", "one\n", "2026-10-01T11:00:00Z");
        fx.commit("src.txt", "two\n", "2026-10-01T12:00:00Z");
        fx.head = fx.git(&["rev-parse", "HEAD"]);
        let create = fx.archie(&["repo", "receipt", "create", "--base", &fx.base]);
        assert!(create.status.success(), "{}", stderr(&create));
        let remote = match push_to {
            PushTo::Origin => "origin",
            PushTo::Fork => {
                fx.git(&["push", "-q", "fork", "main"]);
                "fork"
            }
        };
        fx.git(&["push", "-q", remote, "feature", "refs/notes/agentworth"]);
        // GitHub keeps every PR head under refs/pull/N/head of the base repository.
        fx.git(&["push", "-q", "origin", "feature:refs/pull/1/head"]);
        fx
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
            // Pushes here are fixture setup, never the pre-push hook's job.
            .env("AGENTWORTH_RECEIPT_PUSH", "1")
            .env("GIT_AUTHOR_NAME", "Dev")
            .env("GIT_AUTHOR_EMAIL", "dev@example.invalid")
            .env("GIT_COMMITTER_NAME", "Dev")
            .env("GIT_COMMITTER_EMAIL", "dev@example.invalid")
            .env_remove("AGENTWORTH_RECEIPT_KEY")
            .env_remove("AGENTWORTH_CONFIG_PATH")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE");
        for (k, _) in std::env::vars() {
            if k.starts_with("GITHUB_") || k.starts_with("INPUT_") || k.starts_with("RUNNER_") {
                c.env_remove(k);
            }
        }
    }

    fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let mut c = Command::new("git");
        c.arg("-C").arg(dir).args(args);
        self.env(&mut c);
        let out = c.output().expect("run git");
        assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.dev, args)
    }

    fn archie(&self, args: &[&str]) -> Output {
        let mut c = Command::new(self.bin.join("archie"));
        c.current_dir(&self.dev).args(args);
        self.env(&mut c);
        // The push guard would make `create` a no-op.
        c.env_remove("AGENTWORTH_RECEIPT_PUSH");
        c.output().expect("run archie")
    }

    fn commit(&self, rel: &str, body: &str, date: &str) {
        std::fs::write(self.dev.join(rel), body).unwrap();
        self.git(&["add", rel]);
        let mut c = Command::new("git");
        c.arg("-C")
            .arg(&self.dev)
            .args(["commit", "-q", "-m", &format!("edit {rel}")]);
        self.env(&mut c);
        c.env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date);
        assert!(c.output().unwrap().status.success());
    }

    /// The runner's checkout: a fresh clone (no notes, as on GitHub), at the PR head for
    /// pull_request, on the default branch for workflow_run.
    fn checkout(&self, at_pr_head: bool) {
        let out = Command::new("git")
            .args(["clone", "-q"])
            .arg(&self.origin)
            .arg(&self.runner)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        if at_pr_head {
            self.git_in(&self.runner, &["fetch", "-q", "origin", "refs/pull/1/head"]);
            self.git_in(&self.runner, &["checkout", "-q", "--detach", "FETCH_HEAD"]);
        }
    }

    fn route(&self, method: &str, path: &str, body: &str) {
        std::fs::write(self.fake.join("routes").join(route_key(method, path)), body).unwrap();
    }

    fn fail_route(&self, method: &str, path: &str, stderr: &str) {
        let name = format!("{}.fail", route_key(method, path));
        std::fs::write(self.fake.join("routes").join(name), stderr).unwrap();
    }

    fn public_key(&self) -> String {
        std::fs::read_to_string(self.home.join(".agentworth/keys/receipt_ed25519.pub"))
            .unwrap()
            .trim()
            .to_string()
    }

    /// The API as GitHub would answer it for this PR: head CI green (plus this job's own
    /// check, still running, which must be ignored), one commit author, their signing key.
    fn standard_routes(&self, ci_status: &str) {
        let checks = serde_json::json!({
            "total_count": 3,
            "check_runs": [
                { "name": "test", "status": ci_status,
                  "conclusion": if ci_status == "completed" { serde_json::json!("success") } else { serde_json::Value::Null },
                  "check_suite": { "id": 5 } },
                { "name": "Receipt verify", "status": "in_progress", "conclusion": null,
                  "check_suite": { "id": SELF_SUITE } },
                { "name": "AgentWorth / Evidence", "status": "completed", "conclusion": "neutral",
                  "check_suite": { "id": 6 } }
            ]
        });
        self.route(
            "GET",
            &format!("repos/{REPO}/actions/runs/{RUN_ID}"),
            &serde_json::json!({ "check_suite_id": SELF_SUITE }).to_string(),
        );
        self.route(
            "GET",
            &format!("repos/{REPO}/commits/{}/check-runs?per_page=100", self.head),
            &checks.to_string(),
        );
        self.route(
            "GET",
            &format!("repos/{REPO}/commits/{}/status?per_page=100", self.head),
            r#"{"state":"success","statuses":[]}"#,
        );
        self.route(
            "GET",
            &format!("repos/{REPO}/pulls/1/commits?per_page=100"),
            &serde_json::json!([
                { "sha": self.head, "author": { "login": AUTHOR } },
                { "sha": self.head, "author": null }
            ])
            .to_string(),
        );
        self.route(
            "GET",
            &format!("users/{AUTHOR}/ssh_signing_keys?per_page=100"),
            &serde_json::json!([{ "id": 1, "key": self.public_key(), "title": "laptop" }])
                .to_string(),
        );
        self.route("POST", &format!("repos/{REPO}/check-runs"), r#"{"id":1}"#);
    }

    fn pull_request_event(&self, head_repo: &str) -> serde_json::Value {
        serde_json::json!({
            "action": "synchronize",
            "number": 1,
            "pull_request": {
                "number": 1,
                "user": { "login": AUTHOR },
                "base": { "sha": self.base, "ref": "main", "repo": { "full_name": REPO } },
                "head": { "sha": self.head, "ref": "feature", "repo": { "full_name": head_repo } }
            }
        })
    }

    /// Run the composite step's script the way action.yml does.
    fn run_action(
        &self,
        event_name: &str,
        event: &serde_json::Value,
        inputs: &[(&str, &str)],
    ) -> Run {
        let event_path = self.root.join("event.json");
        std::fs::write(&event_path, event.to_string()).unwrap();
        let summary = self.root.join("summary.md");
        let output = self.root.join("output.txt");
        let _ = std::fs::remove_file(&summary);
        let _ = std::fs::remove_file(&output);
        let mut c = Command::new("bash");
        c.arg(action_script()).current_dir(&self.runner);
        self.env(&mut c);
        c.env_remove("AGENTWORTH_RECEIPT_PUSH")
            .env("GITHUB_ACTIONS", "true")
            .env("GITHUB_REPOSITORY", REPO)
            .env("GITHUB_EVENT_NAME", event_name)
            .env("GITHUB_EVENT_PATH", &event_path)
            .env("GITHUB_STEP_SUMMARY", &summary)
            .env("GITHUB_OUTPUT", &output)
            .env("GITHUB_RUN_ID", RUN_ID)
            .env("RUNNER_TEMP", self.root.join("runner-temp"))
            .env("RUNNER_OS", "Linux")
            .env("RUNNER_ARCH", "X64")
            .env("AGENTWORTH_GH", self.bin.join("gh-fake"))
            .env("FAKE_GH_DIR", &self.fake)
            .env(
                "AGENTWORTH_GITHUB_URL",
                format!("file://{}", self.root.join("github").display()),
            )
            .env("AGENTWORTH_POLL_SECONDS", "1")
            .env("INPUT_WAIT_FOR_CI", "0")
            .env("INPUT_ARCHIE_PATH", self.bin.join("archie"));
        for (k, v) in inputs {
            c.env(k, v);
        }
        let out = c.output().expect("run action");
        Run {
            status: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            summary: std::fs::read_to_string(&summary).unwrap_or_default(),
            output: std::fs::read_to_string(&output).unwrap_or_default(),
            posted: (1..=4)
                .filter_map(|n| std::fs::read(self.fake.join(format!("posted-{n}.json"))).ok())
                .map(|b| serde_json::from_slice(&b).expect("posted body is JSON"))
                .collect(),
            calls: std::fs::read_to_string(self.fake.join("calls.log")).unwrap_or_default(),
        }
    }
}

struct Run {
    status: Option<i32>,
    stdout: String,
    stderr: String,
    summary: String,
    output: String,
    posted: Vec<serde_json::Value>,
    calls: String,
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exit {:?}\n--- stdout\n{}\n--- stderr\n{}\n--- output\n{}\n--- calls\n{}",
            self.status, self.stdout, self.stderr, self.output, self.calls
        )
    }
}

impl Run {
    fn assert_exit_zero(&self) {
        assert_eq!(
            self.status,
            Some(0),
            "the step must always exit 0: {self:?}"
        );
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

/// Mask what changes per run: hex runs of 7+ (SHAs, receipt ids, short SHAs) and key
/// fingerprints.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i..].starts_with(&['S', 'H', 'A', '2', '5', '6', ':']) {
            let mut j = i + 7;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || "+/=".contains(chars[j]))
            {
                j += 1;
            }
            out.push_str("SHA256:<fingerprint>");
            i = j;
            continue;
        }
        let boundary = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
        if boundary && chars[i].is_ascii_hexdigit() && !chars[i].is_ascii_uppercase() {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_hexdigit() && !chars[j].is_ascii_uppercase()
            {
                j += 1;
            }
            let end_ok = j == chars.len() || !chars[j].is_ascii_alphanumeric();
            if j - i >= 7 && end_ok {
                out.push_str(&format!("<hex{}>", j - i));
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn assert_snapshot(name: &str, value: &serde_json::Value) {
    let actual = normalize(&serde_json::to_string_pretty(value).unwrap()) + "\n";
    let path = manifest_dir().join("tests/fixtures/action").join(name);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() || !path.exists() {
        std::fs::write(&path, &actual).unwrap();
        if std::env::var_os("UPDATE_SNAPSHOTS").is_none() {
            panic!("wrote new snapshot {}; re-run to compare", path.display());
        }
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        actual, expected,
        "snapshot {name} differs (UPDATE_SNAPSHOTS=1 rewrites it)"
    );
}

#[test]
fn same_repo_pull_request_posts_the_check_and_ignores_its_own_job() {
    if !tools_present() {
        return;
    }
    let fx = Fixture::new(PushTo::Origin);
    fx.checkout(true);
    fx.standard_routes("completed");
    let run = fx.run_action("pull_request", &fx.pull_request_event(REPO), &[]);
    run.assert_exit_zero();
    assert_eq!(run.posted.len(), 1, "{run:?}");
    let check = &run.posted[0];
    assert_eq!(check["name"], "AgentWorth / Evidence");
    assert_eq!(check["head_sha"], fx.head.as_str());
    // Two commits, one valid receipt from a linked key, head CI green: the only success case.
    assert_eq!(check["conclusion"], "success", "{run:?}");
    assert!(run.output.contains("posted=true"), "{run:?}");
    assert!(run.output.contains("conclusion=success"), "{run:?}");
    assert!(run.summary.contains("### AgentWorth / Evidence"), "{run:?}");
    assert!(
        run.summary.contains(&format!("@{AUTHOR}")),
        "{}",
        run.summary
    );
    assert_snapshot("same_repo.check.json", check);
}

#[test]
fn fork_pull_request_with_a_read_only_token_falls_back_to_the_summary() {
    if !tools_present() {
        return;
    }
    let fx = Fixture::new(PushTo::Fork);
    fx.checkout(true);
    fx.standard_routes("completed");
    fx.fail_route(
        "POST",
        &format!("repos/{REPO}/check-runs"),
        "gh: Resource not accessible by integration (HTTP 403)\n",
    );
    let run = fx.run_action(
        "pull_request",
        &fx.pull_request_event(&format!("{FORKER}/widget")),
        &[],
    );
    run.assert_exit_zero();
    // It tried once, was refused, and said so without failing the job.
    assert_eq!(run.posted.len(), 1, "{run:?}");
    assert!(
        run.stdout
            .contains("::notice title=AgentWorth / Evidence::"),
        "{run:?}"
    );
    assert!(run.stdout.contains("cannot create check runs"), "{run:?}");
    assert!(run.stdout.contains("workflow_run"), "{run:?}");
    assert!(run.output.contains("posted=false"), "{run:?}");
    // The receipt came from the fork's notes, so the table still shows it.
    assert!(run.summary.contains("### AgentWorth / Evidence"), "{run:?}");
    assert_eq!(run.posted[0]["conclusion"], "success", "{run:?}");
}

#[test]
fn workflow_run_finds_a_fork_pull_request_by_branch_and_fetches_its_head() {
    if !tools_present() {
        return;
    }
    let fx = Fixture::new(PushTo::Fork);
    fx.checkout(false);
    fx.standard_routes("completed");
    fx.route(
        "GET",
        &format!("repos/{REPO}/pulls?state=open&per_page=100&head={FORKER}:feature"),
        &serde_json::json!([{ "number": 1, "head": { "sha": fx.head } }]).to_string(),
    );
    fx.route(
        "GET",
        &format!("repos/{REPO}/pulls/1"),
        &serde_json::json!({ "number": 1, "user": { "login": AUTHOR }, "base": { "sha": fx.base } })
            .to_string(),
    );
    let event = serde_json::json!({
        "action": "completed",
        "workflow_run": {
            "head_sha": fx.head,
            "head_branch": "feature",
            "pull_requests": [],
            "head_repository": { "full_name": format!("{FORKER}/widget"), "owner": { "login": FORKER } }
        }
    });
    let run = fx.run_action("workflow_run", &event, &[]);
    run.assert_exit_zero();
    assert_eq!(run.posted.len(), 1, "{run:?}");
    assert_eq!(run.posted[0]["head_sha"], fx.head.as_str());
    assert_eq!(run.posted[0]["conclusion"], "success", "{run:?}");
    assert!(run.output.contains("posted=true"), "{run:?}");
}

#[test]
fn pending_ci_is_reported_without_waiting_and_fail_on_is_passed_through() {
    if !tools_present() {
        return;
    }
    let fx = Fixture::new(PushTo::Origin);
    fx.checkout(true);
    fx.standard_routes("in_progress");
    let run = fx.run_action(
        "pull_request",
        &fx.pull_request_event(REPO),
        &[("INPUT_FAIL_ON", "unattributed")],
    );
    run.assert_exit_zero();
    assert_eq!(run.posted.len(), 1, "{run:?}");
    // Pending CI is never success; full coverage means fail-on: unattributed does not trip.
    assert_eq!(run.posted[0]["conclusion"], "neutral", "{run:?}");
    assert_snapshot("ci_pending.check.json", &run.posted[0]);

    // An unknown fail-on value stops with a warning, still exit 0, and posts nothing new.
    let bad = fx.run_action(
        "pull_request",
        &fx.pull_request_event(REPO),
        &[("INPUT_FAIL_ON", "always")],
    );
    bad.assert_exit_zero();
    assert!(bad.stdout.contains("::warning"), "{bad:?}");
    assert_eq!(bad.posted.len(), 1, "{bad:?}");
}

#[test]
fn a_pull_request_with_no_receipts_still_gets_a_neutral_check_with_the_install_line() {
    if !tools_present() {
        return;
    }
    let fx = Fixture::new(PushTo::Origin);
    fx.checkout(true);
    fx.standard_routes("completed");
    // transport: file skips the notes fetch, and nothing is committed under .agentworth/.
    let run = fx.run_action(
        "pull_request",
        &fx.pull_request_event(REPO),
        &[("INPUT_TRANSPORT", "file")],
    );
    run.assert_exit_zero();
    assert_eq!(run.posted[0]["conclusion"], "neutral", "{run:?}");
    assert!(
        run.summary
            .contains("npx -y agentworth@latest repo receipt init"),
        "{}",
        run.summary
    );
    assert!(!run.calls.contains("notes"), "{run:?}");
}

#[test]
fn unsupported_events_stop_cleanly() {
    if !tools_present() {
        return;
    }
    let fx = Fixture::new(PushTo::Origin);
    fx.checkout(true);
    fx.standard_routes("completed");
    let target = fx.run_action("pull_request_target", &fx.pull_request_event(REPO), &[]);
    target.assert_exit_zero();
    assert!(
        target
            .stdout
            .contains("pull_request_target is not supported"),
        "{target:?}"
    );
    let push = fx.run_action("push", &serde_json::json!({}), &[]);
    push.assert_exit_zero();
    assert!(push.stdout.contains("::notice"), "{push:?}");
    assert!(fx
        .run_action("push", &serde_json::json!({}), &[])
        .posted
        .is_empty());
}

/// Build a release directory the way release.yml lays it out, with an `archie` that runs
/// this build.
fn fake_release(fx: &Fixture, version: &str, good_checksum: bool) -> PathBuf {
    let rel = fx.root.join("releases");
    let dir = rel.join(version);
    let stage = fx.root.join("stage");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&stage).unwrap();
    let archie = stage.join("archie");
    std::fs::write(
        &archie,
        format!(
            "#!/bin/sh\nexec '{}' \"$@\"\n",
            assert_cmd::cargo::cargo_bin("archie").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&archie, std::fs::Permissions::from_mode(0o755)).unwrap();
    let name = format!("agentworth-{version}-x86_64-unknown-linux-gnu.tar.gz");
    let tar = Command::new("tar")
        .arg("-czf")
        .arg(dir.join(&name))
        .arg("-C")
        .arg(&stage)
        .arg("archie")
        .output()
        .unwrap();
    assert!(tar.status.success(), "{}", stderr(&tar));
    let sum = if good_checksum {
        let o = Command::new("sh")
            .arg("-c")
            .arg("if command -v sha256sum >/dev/null; then sha256sum \"$1\"; else shasum -a 256 \"$1\"; fi")
            .arg("sh")
            .arg(dir.join(&name))
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .next()
            .unwrap()
            .to_string()
    } else {
        "0".repeat(64)
    };
    std::fs::write(
        dir.join(format!("{name}.sha256")),
        format!("{sum}  {name}\n"),
    )
    .unwrap();
    rel
}

#[test]
fn install_verifies_the_release_checksum_before_running_archie() {
    if !tools_present() {
        return;
    }
    let fx = Fixture::new(PushTo::Origin);
    fx.checkout(true);
    fx.standard_routes("completed");

    let rel = fake_release(&fx, "v9.9.9", false);
    let base_url = format!("file://{}", rel.display());
    let cache = fx.root.join("tool-cache-bad");
    let cache_s = cache.to_string_lossy().to_string();
    let bad = fx.run_action(
        "pull_request",
        &fx.pull_request_event(REPO),
        &[
            ("INPUT_ARCHIE_PATH", ""),
            ("INPUT_ARCHIE_VERSION", "v9.9.9"),
            ("RUNNER_TOOL_CACHE", &cache_s),
            ("AGENTWORTH_RELEASE_BASE_URL", &base_url),
        ],
    );
    bad.assert_exit_zero();
    assert!(bad.stdout.contains("checksum mismatch"), "{bad:?}");
    assert!(
        bad.posted.is_empty(),
        "never runs an unverified binary: {bad:?}"
    );
    assert!(!cache.join("agentworth/9.9.9/x64/archie").exists());

    let rel = fake_release(&fx, "v9.9.9", true);
    let base_url = format!("file://{}", rel.display());
    let cache = fx.root.join("tool-cache-good");
    let cache_s = cache.to_string_lossy().to_string();
    let good = fx.run_action(
        "pull_request",
        &fx.pull_request_event(REPO),
        &[
            ("INPUT_ARCHIE_PATH", ""),
            ("AGENTWORTH_ACTION_REF", "v9.9.9"),
            ("RUNNER_TOOL_CACHE", &cache_s),
            ("AGENTWORTH_RELEASE_BASE_URL", &base_url),
        ],
    );
    good.assert_exit_zero();
    assert_eq!(good.posted.len(), 1, "{good:?}");
    assert!(
        cache.join("agentworth/9.9.9/x64/archie").exists(),
        "cached by version"
    );
}
