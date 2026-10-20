//! Markdown and check-run JSON for the AgentWorth / Evidence check (spec §3.3).
//!
//! Every word here comes from an enum, an integer, a validated hex string, an allowlisted
//! adapter name, a validated GitHub login, or the PR's own commit subjects (escaped). Nothing is
//! copied from a receipt's free-form fields, because a receipt has none and the verifier must not
//! start trusting one. Glyphs stay inside the set `ci.yml` enforces: `● ○ · — →` plus box
//! drawing. The spec mock's `◌` for "claimed" is outside that set, so claims use `○` and an
//! empty cell is `·`.

use std::fmt::Write as _;

use agentworth_schema::work_receipt::{CommandClass, EvidenceSource, ReceiptTestOrigin};
use serde_json::json;

use super::{
    AttestedRun, CiState, Claim, CommitReport, Conclusion, Coverage, InvalidKind, ReceiptReport,
    SignatureState, Transport, VerifiedFact, VerifyReport, CHECK_NAME,
};

/// GitHub rejects a check-run `output.summary` over 65,535 characters.
pub const MAX_SUMMARY_CHARS: usize = 65_000;
/// Longest commit subject shown, in characters.
const SUBJECT_CHARS: usize = 60;

const FOOTER: &str = "<sub>What this check cannot see: transcripts never leave the author's machine. A receipt is the author's machine attesting to itself; only the \"Verified here\" column is checked by this runner. Install: `npx -y agentworth@latest repo receipt init`.</sub>";

fn rung_label(rung: u8) -> &'static str {
    match rung {
        5 => "CI verified",
        4 => "commit in PR",
        3 => "tests passed locally (signed)",
        2 => "files match diff",
        _ => "done claimed",
    }
}

/// What a rung *claims*, for the "Claimed only" column (no "verified" wording).
fn claimed_kind(rung: u8) -> &'static str {
    match rung {
        5 => "CI or deploy pass",
        4 => "commit",
        3 => "test or build pass",
        2 => "file change",
        _ => "completion claim",
    }
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `SHA256:` plus the first 4 fingerprint characters, as the spec mock shows it.
fn short_keyid(keyid: &str) -> String {
    let body = keyid.strip_prefix("SHA256:").unwrap_or(keyid);
    let head: String = body
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/')
        .take(4)
        .collect();
    format!("SHA256:{head}...")
}

/// Escape a commit subject for a markdown table cell and cap its length.
pub fn escape_cell(s: &str) -> String {
    let mut out = String::new();
    for (n, ch) in s.chars().enumerate() {
        if n == SUBJECT_CHARS {
            out.push_str("...");
            break;
        }
        match ch {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '#' | '~' | '!' | '@' => {
                out.push('\\');
                out.push(ch);
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn ci_phrase(ci: CiState) -> String {
    match ci {
        CiState::Unknown => "CI status not provided".into(),
        CiState::NoChecks => "no CI checks reported".into(),
        CiState::Pending { done, total } => format!("CI pending, {done} of {total} checks done"),
        CiState::Red { failed, total } => format!("CI red, {failed} of {total} checks failed"),
        CiState::Green { passed, total } => format!("{passed} of {total} checks passed"),
    }
}

fn ci_title(ci: CiState) -> String {
    match ci {
        CiState::Unknown => "CI unknown".into(),
        CiState::NoChecks => "no CI checks".into(),
        CiState::Pending { done, total } => format!("CI pending {done}/{total}"),
        CiState::Red { failed, total } => format!("CI red {failed}/{total} failed"),
        CiState::Green { passed, total } => format!("head CI {passed}/{total} green"),
    }
}

/// The check-run title, e.g. `Rung 5 · CI verified · 2 of 3 commits carry a signed receipt`.
pub fn render_title(r: &VerifyReport) -> String {
    let mut parts: Vec<String> = Vec::new();
    if r.any_invalid() {
        parts.push("Signature invalid".into());
    }
    match (r.highest_verified(), r.highest_attested()) {
        (Some(v), _) => parts.push(format!("Rung {v} · {}", rung_label(v))),
        (None, Some(a)) => parts.push(format!("Rung {a} attested")),
        (None, None) => {}
    }
    parts.push(format!(
        "{} of {} carry a signed receipt",
        r.signed_commits(),
        plural(r.commits.len(), "commit", "commits")
    ));
    if r.highest_verified() != Some(5) {
        parts.push(ci_title(r.ci));
    }
    parts.join(" · ")
}

fn signature_line(rec: &ReceiptReport) -> String {
    let id = rec
        .receipt_id
        .as_deref()
        .map(|i| i.get(..8).unwrap_or(i).to_string());
    let label = match &id {
        Some(i) => format!("Receipt {i}"),
        None => "Unreadable receipt data".into(),
    };
    let via = match rec.transport {
        Transport::Note => "git note",
        Transport::File => "receipt file",
        Transport::Explicit => "file argument",
    };
    let sessions = rec
        .statement
        .as_ref()
        .map(|s| {
            let mut adapters: Vec<&str> = s
                .predicate
                .sessions
                .iter()
                .map(|x| super::AdapterLabel::of(&x.adapter).as_str())
                .collect();
            adapters.sort_unstable();
            adapters.dedup();
            let n = s.predicate.sessions.len();
            if n == 0 {
                "0 sessions".to_string()
            } else {
                format!(
                    "{} · {}",
                    plural(n, "session", "sessions"),
                    adapters.join(", ")
                )
            }
        })
        .unwrap_or_default();
    let state = match &rec.signature {
        SignatureState::SignedBy { login, keyid } => format!(
            "signed by @{login} (GitHub signing key {})",
            short_keyid(keyid)
        ),
        SignatureState::KeyNotLinked { keyid } => format!(
            "signed, key not linked ({}): integrity holds, identity does not",
            short_keyid(keyid)
        ),
        SignatureState::Invalid(kind) => format!(
            "signature invalid ({}): every rung it names is claimed only",
            invalid_label(*kind)
        ),
    };
    let mut line = format!("{label} · {state} · {via}");
    if rec.signature.is_valid() && !sessions.is_empty() {
        let _ = write!(line, " · {sessions}");
    }
    if rec.subjects_outside_pr > 0 {
        let _ = write!(
            line,
            " · {} not in this PR",
            plural(rec.subjects_outside_pr, "commit", "commits")
        );
    }
    line
}

fn invalid_label(kind: InvalidKind) -> &'static str {
    match kind {
        InvalidKind::Unreadable => "not a DSSE envelope",
        InvalidKind::UnsupportedPayloadType => "unsupported payload type",
        InvalidKind::BadEncoding => "bad base64",
        InvalidKind::TooLarge => "over the 64 KiB cap",
        InvalidKind::MalformedStatement => "not a Work Receipt v1",
        InvalidKind::BadSignerKey => "bad signer key",
        InvalidKind::SignatureMismatch => "signature does not match",
        InvalidKind::NonCanonicalPayload => "payload not canonical",
    }
}

fn command_label(c: CommandClass) -> &'static str {
    match c {
        CommandClass::CargoTest => "cargo test",
        CommandClass::CargoBuild => "cargo build",
        CommandClass::NpmTest => "npm test",
        CommandClass::PnpmTest => "pnpm test",
        CommandClass::Pytest => "pytest",
        CommandClass::GoTest => "go test",
        CommandClass::MakeTest => "make test",
        CommandClass::GhPrCreate => "gh pr create",
        CommandClass::OtherTestCommand => "other test command",
    }
}

fn attested_text(a: &AttestedRun) -> String {
    let what = match a.command_class {
        Some(c) => format!("`{}`", command_label(c.class())),
        None => "test or build".into(),
    };
    let how = match a.source.source() {
        EvidenceSource::ShellExit0 => "exit 0",
        EvidenceSource::ToolResult => "tool reported success",
        _ => "passed",
    };
    let mut s = format!("● 3 {what} {how}{}", times(a.count));
    match a.test_origin.map(|o| o.origin()) {
        Some(ReceiptTestOrigin::AgentChosen) => s.push_str(" · tests chosen by agent"),
        Some(ReceiptTestOrigin::HeldOut) => s.push_str(" · held-out tests"),
        Some(ReceiptTestOrigin::Unknown) => s.push_str(" · test authorship unknown"),
        None => {}
    }
    s
}

fn verified_text(f: &VerifiedFact, ci: CiState) -> String {
    match f {
        VerifiedFact::Ci { passed, total } => format!("● 5 CI {passed}/{total} on this SHA"),
        VerifiedFact::CommitInPr => "● 4 commit in PR".into(),
        VerifiedFact::FilesMatch { matched, claimed } => {
            format!("● 2 files match diff ({matched}/{claimed})")
        }
        VerifiedFact::HeadCiCoversFinalTree => match ci {
            CiState::Green { passed, total } => {
                format!("→ head CI {passed}/{total} covers final tree")
            }
            _ => "→ head CI covers final tree".into(),
        },
    }
}

fn claim_text(c: &Claim) -> String {
    match c {
        Claim::CompletionClaims(n) => format!("○ 1 completion claim{}, no exit code", times(*n)),
        Claim::NotVerifiedLocally { rung, count } => format!(
            "○ {rung} {}{} not re-checked locally",
            claimed_kind(*rung),
            times(*count)
        ),
        Claim::FilesNotInDiff(n) => format!("○ 2 claimed files not in diff ({n})"),
        Claim::InvalidSignature { rungs, highest } => {
            let mut s = "○ receipt signature invalid".to_string();
            if *rungs > 0 {
                let _ = write!(s, ": {} claimed", plural(*rungs, "rung", "rungs"));
                if let Some(h) = highest {
                    let _ = write!(s, ", highest {h}");
                }
            }
            s
        }
    }
}

/// ` (Nx)` for a repeated item, nothing for one.
fn times(n: usize) -> String {
    if n > 1 {
        format!(" ({n}x)")
    } else {
        String::new()
    }
}

fn cell(items: Vec<String>) -> String {
    if items.is_empty() {
        "·".into()
    } else {
        items.join(" · ")
    }
}

fn row(c: &CommitReport, ci: CiState) -> String {
    let commit = format!("{} {}", short(&c.sha), escape_cell(&c.subject));
    let sessions = cell(
        c.sessions
            .iter()
            .map(|t| format!("{} · {}", t.adapter.as_str(), t.short_ref))
            .collect(),
    );
    let (verified, attested, claimed) = match c.coverage {
        Coverage::None => (
            "·".to_string(),
            "·".to_string(),
            "unattributed: no receipt covers this commit".to_string(),
        ),
        _ => (
            cell(c.verified.iter().map(|f| verified_text(f, ci)).collect()),
            cell(c.attested.iter().map(attested_text).collect()),
            cell(c.claimed.iter().map(claim_text).collect()),
        ),
    };
    format!("| {commit} | {sessions} | {verified} | {attested} | {claimed} |")
}

fn headline(r: &VerifyReport) -> String {
    let head = short(&r.head);
    match r.highest_verified() {
        Some(v) => format!(
            "**Highest evidence rung: {v}, {}** (head {head}, {})",
            rung_label(v),
            ci_phrase(r.ci)
        ),
        None => format!(
            "**Highest evidence rung: none.** No commit in this PR is covered by a valid receipt (head {head}, {})",
            ci_phrase(r.ci)
        ),
    }
}

fn claims_vs_evidence(r: &VerifyReport) -> Vec<String> {
    let signed = r.signed_commits();
    let (mut matched, mut claimed_files, mut missing) = (0, 0, 0);
    let (mut runs, mut agent_chosen) = (0, 0);
    let (mut completion, mut not_local, mut invalid_rungs) = (0, 0, 0);
    for c in &r.commits {
        // FilesMatch.claimed already counts a commit's misses, so misses only add to the
        // claimed total when nothing in that commit matched.
        let mut has_match = false;
        for f in &c.verified {
            if let VerifiedFact::FilesMatch {
                matched: m,
                claimed,
            } = f
            {
                matched += m;
                claimed_files += claimed;
                has_match = true;
            }
        }
        for a in &c.attested {
            runs += a.count;
            if a.test_origin.map(|o| o.origin()) == Some(ReceiptTestOrigin::AgentChosen) {
                agent_chosen += a.count;
            }
        }
        for cl in &c.claimed {
            match cl {
                Claim::CompletionClaims(n) => completion += n,
                Claim::NotVerifiedLocally { count, .. } => not_local += count,
                Claim::FilesNotInDiff(n) => {
                    missing += n;
                    if !has_match {
                        claimed_files += n;
                    }
                }
                Claim::InvalidSignature { rungs, .. } => invalid_rungs += rungs,
            }
        }
    }
    let unattributed = r
        .commits
        .iter()
        .filter(|c| c.coverage == Coverage::None)
        .count();

    let mut evidence = vec![format!(
        "{} named by a signed receipt {} in this PR",
        plural(signed, "commit", "commits"),
        if signed == 1 { "is" } else { "are" }
    )];
    if claimed_files > 0 {
        evidence.push(format!(
            "{matched} of {} appear in their diffs",
            plural(claimed_files, "claimed file", "claimed files")
        ));
    }
    evidence.push(match r.ci {
        CiState::Green { passed, total } => format!("head CI {passed}/{total} green"),
        other => ci_phrase(other),
    });

    let attested = if runs == 0 {
        "none".to_string()
    } else {
        let mut s = format!(
            "{} exited 0",
            plural(runs, "local test or build run", "local test or build runs")
        );
        if agent_chosen > 0 {
            let _ = write!(s, " ({agent_chosen} with agent-chosen tests)");
        }
        s
    };

    let mut claims = Vec::new();
    if completion > 0 {
        claims.push(format!(
            "{} with no exit code",
            plural(completion, "completion claim", "completion claims")
        ));
    }
    if not_local > 0 {
        claims.push(format!(
            "{} not re-checked locally",
            plural(not_local, "rung", "rungs")
        ));
    }
    if missing > 0 {
        claims.push(format!(
            "{} not in the diff",
            plural(missing, "claimed file", "claimed files")
        ));
    }
    if invalid_rungs > 0 {
        claims.push(format!(
            "{} from receipts with invalid signatures",
            plural(invalid_rungs, "rung", "rungs")
        ));
    }
    if unattributed > 0 {
        claims.push(format!(
            "{} with no receipt",
            plural(unattributed, "commit", "commits")
        ));
    }
    let claims = if claims.is_empty() {
        "none".to_string()
    } else {
        claims.join("; ")
    };

    vec![
        format!(
            "- Evidence, re-derived on this runner: {}.",
            evidence.join("; ")
        ),
        format!("- Attested, signed but not re-runnable here: {attested}."),
        format!("- Claims, not evidence: {claims}."),
    ]
}

/// The check summary as GitHub-flavored markdown.
pub fn render_markdown(r: &VerifyReport) -> String {
    let mut head = vec!["### AgentWorth / Evidence".to_string(), String::new()];
    head.push(headline(r));
    head.push(String::new());
    if let Some(a) = r.highest_attested() {
        head.push(format!("Highest attested rung: {a}, {}", rung_label(a)));
        head.push(String::new());
    }
    for rec in &r.receipts {
        head.push(signature_line(rec));
        head.push(String::new());
    }
    if r.stale_receipts > 0 {
        head.push(format!(
            "{} no commit in this PR (rebased or squashed SHAs). Push again to re-sign.",
            if r.stale_receipts == 1 {
                "1 signed receipt names".to_string()
            } else {
                format!("{} signed receipts name", r.stale_receipts)
            }
        ));
        head.push(String::new());
    }
    if r.commits.is_empty() {
        head.push("This PR has no commits in `base..head`.".into());
        head.push(String::new());
    } else {
        head.push(
            "| Commit | Session | Verified here | Attested locally (signed) | Claimed only |"
                .into(),
        );
        head.push("| :-- | :-- | :-- | :-- | :-- |".into());
    }

    let mut tail = vec![String::new(), "**Claims vs evidence**".to_string()];
    tail.extend(claims_vs_evidence(r));
    tail.push(String::new());
    if r.risk.sessions > 0 {
        let mut risk = format!(
            "Risk notes: {}, {}, {}",
            plural(r.risk.loop_alerts as usize, "loop alert", "loop alerts"),
            plural(
                r.risk.unresolved_loops as usize,
                "unresolved loop",
                "unresolved loops"
            ),
            plural(
                r.risk.governor_halts as usize,
                "governor halt",
                "governor halts"
            ),
        );
        if r.risk.demoted_claims > 0 {
            let _ = write!(
                risk,
                ", {}",
                plural(
                    r.risk.demoted_claims as usize,
                    "demoted claim",
                    "demoted claims"
                )
            );
        }
        if r.risk.unknown_sessions > 0 {
            let _ = write!(
                risk,
                "; risk unknown for {}",
                plural(r.risk.unknown_sessions, "session", "sessions")
            );
        }
        risk.push('.');
        tail.push(risk);
        tail.push(String::new());
    }
    let mut notes = Vec::new();
    if r.skipped_commits > 0 {
        notes.push(format!(
            "{} beyond the first {} not examined.",
            plural(r.skipped_commits, "commit", "commits"),
            super::MAX_PR_COMMITS
        ));
    }
    if r.skipped_envelopes > 0 {
        notes.push(format!(
            "{} beyond the first {} not examined.",
            plural(r.skipped_envelopes, "receipt", "receipts"),
            super::MAX_ENVELOPES
        ));
    }
    if r.conclusion == Conclusion::Failure {
        let names: Vec<String> = r
            .failed_on
            .iter()
            .map(|f| match f {
                super::FailOn::InvalidSignature => "invalid-signature".to_string(),
                super::FailOn::BelowRung(n) => format!("below-rung:{n}"),
                super::FailOn::Unattributed => "unattributed".to_string(),
            })
            .collect();
        notes.push(format!(
            "Failing because this repository opted in to `fail-on: {}`.",
            names.join(", ")
        ));
    }
    for n in notes {
        tail.push(n);
        tail.push(String::new());
    }
    tail.push(FOOTER.to_string());

    // Fit GitHub's summary limit by dropping rows from the end (oldest commits).
    let rows: Vec<String> = r.commits.iter().map(|c| row(c, r.ci)).collect();
    let fixed: usize = head
        .iter()
        .chain(&tail)
        .map(|l| l.chars().count() + 1)
        .sum();
    let mut budget = MAX_SUMMARY_CHARS.saturating_sub(fixed + 80);
    let mut shown = Vec::new();
    for line in &rows {
        let n = line.chars().count() + 1;
        if n > budget {
            break;
        }
        budget -= n;
        shown.push(line.clone());
    }
    let hidden = rows.len() - shown.len();
    let mut out = head;
    out.extend(shown);
    if hidden > 0 {
        out.push(format!(
            "| {} not shown (summary size limit) | · | · | · | · |",
            plural(hidden, "older commit", "older commits")
        ));
    }
    out.extend(tail);
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// The `POST /repos/{o}/{r}/check-runs` body, pretty-printed with a trailing newline.
pub fn render_check_json(r: &VerifyReport) -> String {
    let body = json!({
        "name": CHECK_NAME,
        "head_sha": r.head,
        "status": "completed",
        "conclusion": r.conclusion.as_str(),
        "output": {
            "title": render_title(r),
            "summary": render_markdown(r),
        },
    });
    let mut s = serde_json::to_string_pretty(&body).expect("check JSON serializes");
    s.push('\n');
    s
}
