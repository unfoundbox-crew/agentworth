//! `Redactor::for_publication()` -- the stricter profile used only where bytes leave the
//! machine.
//!
//! Measured 2026-09-10: `archie blunder --submit` was captured against a local listener
//! (STFUOPUS_API_URL pointed at 127.0.0.1) rather than the real endpoint, and the payload it
//! would have posted publicly still carried repository and organization names. The default
//! profile's only path rule strips a home directory's USERNAME (`/Users/alice` -> `~`) and
//! leaves everything after it, which is correct for the local surfaces -- `repo_blame` and
//! `session_show` exist to tell you which repo a session touched, and blanking that would make
//! them useless -- and wrong the moment the same text is published.
//!
//! Note what is NOT the hole: `redact_trace` already derives a `repository_identity_rule` from
//! the trace's own source path, so trace export was covered. `blunder --submit` redacts loose
//! strings with no trace context and never picked that rule up -- and an exhibit's snippet can
//! name several repositories anyway, not just its own session's. Hence a profile rather than
//! another per-trace rule.
//!
//! The default profile is unchanged and this one is additive. Nothing that reads redacted output
//! today gets a different answer; only `blunder --submit` opts in.

use agentworth_redaction::Redactor;

/// The real shape captured from the local dry run, with the username already neutralised.
const CAPTURED_SNIPPET: &str = "cd ~/code/motionvector/studio && mkdir -p \
archived/vl-gtm-launchy-landing && cp -R \"~/code/motionvector/vl-gtm/apps/web/src/app\" \
archived/vl-gtm-launchy-landing/marketing-page && git push";

#[test]
fn the_default_profile_keeps_repo_names_because_repo_blame_needs_them() {
    let out = Redactor::new().redact_text(CAPTURED_SNIPPET);
    assert!(
        out.contains("code/motionvector"),
        "the DEFAULT profile must not start blanking repo names -- repo_blame and \
         session_show are built on them. Got: {out}"
    );
}

#[test]
fn the_publication_profile_strips_absolute_path_tails() {
    let out = Redactor::for_publication().redact_text(CAPTURED_SNIPPET);
    assert!(
        !out.contains("code/motionvector"),
        "an organization name reached a published payload inside an absolute path: {out}"
    );
    assert!(
        out.contains('~'),
        "the shape of the command should survive so the receipt still reads as a command: {out}"
    );
}

/// The boundary, asserted rather than described, so nobody reads `for_publication` as a promise
/// of anonymity.
///
/// A RELATIVE path (`archived/vl-gtm-launchy-landing/...`) still carries its project name
/// through. There is no rule that removes it and leaves ordinary prose intact: the pattern that
/// catches `a/b-c/d` also catches `and/or` and `TODO/FIXME`, which would shred every receipt it
/// touched -- see `prose_is_not_shredded` below, which is the guard that makes this a real
/// constraint rather than laziness.
///
/// The product's answer is not a greedier regex. It is that `archie blunder` shows the operator
/// the EXACT payload before it posts anything (`--dry-run`, and the preview above the y/N
/// prompt), so a human decides with the real bytes in front of them.
#[test]
fn a_relative_path_still_carries_its_project_name_and_that_is_a_known_limit() {
    let out = Redactor::for_publication().redact_text("cp -R archived/vl-gtm-launchy-landing/x .");
    assert!(
        out.contains("vl-gtm"),
        "if this now passes, the rules got greedier -- check `prose_is_not_shredded` still \
         holds and then update this test and `publication_rules`' doc comment together: {out}"
    );
}

/// The username rule runs first and rewrites the prefix to `~`; the publication rule has to
/// run after it, on the result. Ordering regressions here are silent, so assert it directly.
#[test]
fn a_raw_absolute_path_loses_both_the_user_and_the_tail() {
    let out = Redactor::for_publication().redact_text("rm -rf /Users/alice/code/secret-repo");
    assert!(!out.contains("alice"), "username survived: {out}");
    assert!(!out.contains("secret-repo"), "repo name survived: {out}");
}

#[test]
fn windows_paths_are_covered_too() {
    let out = Redactor::for_publication().redact_text(r"del C:\Users\alice\code\secret-repo\x");
    assert!(!out.contains("alice"), "username survived: {out}");
    assert!(!out.contains("secret-repo"), "repo name survived: {out}");
}

/// Publication redaction is additive: everything the default profile catches, it still catches.
#[test]
fn the_publication_profile_still_catches_everything_the_default_does() {
    let secret = "export AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY";
    let default_out = Redactor::new().redact_text(secret);
    let publish_out = Redactor::for_publication().redact_text(secret);
    assert!(!default_out.contains("wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY"));
    assert!(!publish_out.contains("wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY"));
}

/// Ordinary prose must survive. A rule that eats `and/or` or `TODO/FIXME` would quietly
/// shred every receipt it touches.
#[test]
fn prose_is_not_shredded() {
    let prose = "I ran the tests and/or the build, then read TODO/FIXME notes in the report.";
    let out = Redactor::for_publication().redact_text(prose);
    assert_eq!(out, prose, "publication redaction damaged ordinary prose");
}
