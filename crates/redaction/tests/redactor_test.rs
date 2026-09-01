use agentworth_redaction::{RedactionCategory, RedactionReport, RedactionRule, Redactor};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, OutcomeEvidence, OutcomeKind,
    Provenance, ShellCommand, ToolCall,
};
use chrono::Utc;
use serde_json::json;

#[test]
fn test_scrub_api_keys() {
    let redactor = Redactor::new();

    // OpenAI key
    let text = "My OpenAI key is sk-abcdef1234567890abcdef1234567890.";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("sk-abcdef1234567890abcdef1234567890"));
    assert!(redacted.contains("[REDACTED_API_KEY]"));

    // OpenAI project key
    let text = "Use sk-proj-1234567890abcdef1234567890 for auth";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("sk-proj-1234567890abcdef1234567890"));
    assert!(redacted.contains("[REDACTED_API_KEY]"));

    // Anthropic key
    let text = "Anthropic key: sk-ant-api03-abcdef1234567890abcdef1234567890-xyz";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("sk-ant-api03-abcdef1234567890abcdef1234567890-xyz"));
    assert!(redacted.contains("[REDACTED_API_KEY]"));

    // Google API key
    let text = "Google key is AIzaSyD1234567890abcdef1234567890Abcde.";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("AIzaSyD1234567890abcdef1234567890Abcde"));
    assert!(redacted.contains("[REDACTED_API_KEY]"));

    // GitHub token
    let text = "ghp_1234567890abcdef1234567890abcdef123456";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("ghp_1234567890abcdef1234567890abcdef123456"));
    assert!(redacted.contains("[REDACTED_GITHUB_TOKEN]"));

    // GitHub fine-grained pat
    let text = "github_pat_11AAAAAAA01234567890abcdefghijklmnopqrstuvwxyz1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ12";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("github_pat_11AAAAAAA"));
    assert!(redacted.contains("[REDACTED_GITHUB_TOKEN]"));

    // Bearer token
    let text = "Authorization: Bearer mysecretbearertoken1234567890abcdef";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("mysecretbearertoken1234567890abcdef"));
    assert!(redacted.contains("Bearer [REDACTED_TOKEN]"));
}

#[test]
fn test_scrub_env_vars() {
    let redactor = Redactor::new();

    let text = r#"
DATABASE_URL="postgres://postgres:mypassword@localhost:5432/mydb"
SECRET_KEY=super_secret_value_12345
API_KEY='xyz987654321'
PASSWORD=my_strong_password!
AUTH_TOKEN=tok_123456789
AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY
OPENAI_API_KEY=sk-proj-supersecretkey123
STRIPE_SECRET_KEY="sk_test_51Mzxyz12345"
DEEPSEEK_API_KEY='sk-deepseeksecret'
MY_CUSTOM_DB_PASSWORD: super_secure_db_pass
GITHUB_ACCESS_TOKEN=ghp_secrettokenvalue123
"#;
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("super_secret_value_12345"));
    assert!(!redacted.contains("my_strong_password!"));
    assert!(!redacted.contains("tok_123456789"));
    assert!(!redacted.contains("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"));
    assert!(!redacted.contains("sk-proj-supersecretkey123"));
    assert!(!redacted.contains("sk_test_51Mzxyz12345"));
    assert!(!redacted.contains("sk-deepseeksecret"));
    assert!(!redacted.contains("super_secure_db_pass"));
    assert!(!redacted.contains("ghp_secrettokenvalue123"));
    assert!(redacted.contains("OPENAI_API_KEY=[REDACTED_ENV_VAR]"));
    assert!(redacted.contains("STRIPE_SECRET_KEY=[REDACTED_ENV_VAR]"));
    assert!(redacted.contains("DEEPSEEK_API_KEY=[REDACTED_ENV_VAR]"));
    assert!(redacted.contains("MY_CUSTOM_DB_PASSWORD=[REDACTED_ENV_VAR]"));
    assert!(redacted.contains("GITHUB_ACCESS_TOKEN=[REDACTED_ENV_VAR]"));
}

#[test]
fn test_scrub_home_directories() {
    let redactor = Redactor::new();

    let text = "Found config at /Users/saurabh/code/unfoundbox/agentworth/Cargo.toml and /home/alice/.config/app.json";
    let redacted = redactor.redact_text(text);
    assert_eq!(
        redacted,
        "Found config at ~/code/unfoundbox/agentworth/Cargo.toml and ~/.config/app.json"
    );

    let win_text = r"File saved to C:\Users\Administrator\Documents\secret.txt";
    let redacted_win = redactor.redact_text(win_text);
    assert_eq!(redacted_win, r"File saved to ~\Documents\secret.txt");
}

#[test]
fn test_scrub_emails_and_credentials_and_ips() {
    let redactor = Redactor::new();

    let text = "Contact dev-lead@company.internal or user.name+tag@sub.domain.org for info";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("dev-lead@company.internal"));
    assert!(!redacted.contains("user.name+tag@sub.domain.org"));
    assert!(redacted.contains("[REDACTED_EMAIL]"));

    let url_text = "Clone git repo: https://saurabh:ghp_secrettoken@github.com/org/repo.git";
    let redacted_url = redactor.redact_text(url_text);
    assert!(!redacted_url.contains("ghp_secrettoken"));
    assert!(!redacted_url.contains("saurabh:"));
    assert!(redacted_url.contains("https://[REDACTED_CREDENTIALS]@github.com/org/repo.git"));

    let ip_text = "Server running on 192.168.1.100 and database at 10.0.0.5";
    let redacted_ip = redactor.redact_text(ip_text);
    assert!(!redacted_ip.contains("192.168.1.100"));
    assert!(!redacted_ip.contains("10.0.0.5"));
    assert!(redacted_ip.contains("[REDACTED_IP]"));
}

#[test]
fn test_scrub_jwt_and_pem_keys() {
    let redactor = Redactor::new();

    let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    let text = format!("Header Authorization: {jwt}");
    let redacted = redactor.redact_text(&text);
    assert!(!redacted.contains(jwt));
    assert!(redacted.contains("[REDACTED_JWT]"));

    let pem =
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA0Y3...\n-----END RSA PRIVATE KEY-----";
    let text_pem = format!("Key content:\n{pem}\nEnd of file.");
    let redacted_pem = redactor.redact_text(&text_pem);
    assert!(!redacted_pem.contains("MIIEowIBAAKCAQEA0Y3"));
    assert!(redacted_pem.contains("[REDACTED_PRIVATE_KEY]"));
}

#[test]
fn test_redact_event_nested_json() {
    let redactor = Redactor::new();

    let event = NormalizedEvent::new(
        1,
        Utc::now(),
        EventPayload::ToolCall(ToolCall {
            id: Some("call_1".to_string()),
            name: "execute_code".to_string(),
            arguments: json!({
                "script": "export API_KEY=sk-1234567890abcdef1234567890",
                "nested": {
                    "path": "/Users/john_doe/secrets/config.json",
                    "email": "john@example.com",
                    "numbers": [1, 2, 3],
                    "active": true
                }
            }),
        }),
    );

    let redacted_event = redactor.redact_event(&event);

    if let EventPayload::ToolCall(tc) = &redacted_event.payload {
        let script = tc.arguments["script"].as_str().unwrap();
        assert!(!script.contains("sk-1234567890abcdef1234567890"));
        assert!(script.contains("[REDACTED_API_KEY]") || script.contains("[REDACTED_ENV_VAR]"));

        let path = tc.arguments["nested"]["path"].as_str().unwrap();
        assert_eq!(path, "~/secrets/config.json");

        let email = tc.arguments["nested"]["email"].as_str().unwrap();
        assert_eq!(email, "[REDACTED_EMAIL]");

        assert_eq!(tc.arguments["nested"]["numbers"], json!([1, 2, 3]));
        assert_eq!(tc.arguments["nested"]["active"], json!(true));
    } else {
        panic!("unexpected payload type");
    }
}

#[test]
fn test_redact_trace_and_preview() {
    let redactor = Redactor::new();
    let start = Utc::now();
    let prov = Provenance::new(
        "/Users/saurabh/logs/trace.jsonl",
        "claude_code",
        100,
        12345,
        "fp123",
    );
    let mut trace = AgentWorthTrace::new("sess-1", "claude_code", prov, start);

    trace.metadata = json!({
        "developer_email": "saurabh@example.com",
        "home": "/Users/saurabh/dev"
    });

    trace.events.push(NormalizedEvent::new(
        1,
        start,
        EventPayload::UserMessage {
            content:
                "Please check my OpenAI key sk-1234567890abcdef1234567890 in /Users/saurabh/test.py"
                    .to_string(),
        },
    ));

    trace.events.push(NormalizedEvent::new(
        2,
        start,
        EventPayload::ShellCommand(ShellCommand {
            command: "cat /Users/saurabh/.env".to_string(),
            cwd: Some("/Users/saurabh/code".to_string()),
            exit_code: Some(0),
            output: Some("SECRET_KEY=shhh123456\nPASSWORD=mypassword123".to_string()),
        }),
    ));

    trace.events.push(NormalizedEvent::new(
        3,
        start,
        EventPayload::FileAction {
            path: "/Users/saurabh/app.js".to_string(),
            action: FileActionType::Write,
            diff: Some("+ const key = 'sk-ant-api03-1234567890abcdef1234567890-xyz';".to_string()),
            lines_changed: Some(1),
        },
    ));

    // Preview
    let report = redactor.preview_redactions(&trace);
    assert!(!report.is_clean());
    assert!(report.total() > 0);
    assert!(report.api_keys_count >= 2);
    assert!(report.paths_count >= 5);
    assert!(report.emails_count >= 1);
    assert!(report.env_vars_count >= 2);

    // Redact trace
    let redacted_trace = redactor.redact_trace(&trace);
    assert_eq!(redacted_trace.provenance.source_path, "~/logs/trace.jsonl");
    assert_eq!(
        redacted_trace.metadata["developer_email"],
        "[REDACTED_EMAIL]"
    );
    assert_eq!(redacted_trace.metadata["home"], "~/dev");

    if let EventPayload::UserMessage { content } = &redacted_trace.events[0].payload {
        assert!(!content.contains("sk-1234567890abcdef1234567890"));
        assert!(!content.contains("/Users/saurabh"));
        assert!(content.contains("~/test.py"));
    } else {
        panic!("unexpected payload");
    }

    if let EventPayload::ShellCommand(cmd) = &redacted_trace.events[1].payload {
        assert_eq!(cmd.command, "cat ~/.env");
        assert_eq!(cmd.cwd.as_deref(), Some("~/code"));
        assert!(!cmd.output.as_ref().unwrap().contains("shhh123456"));
        assert!(!cmd.output.as_ref().unwrap().contains("mypassword123"));
    } else {
        panic!("unexpected payload");
    }

    if let EventPayload::FileAction { path, diff, .. } = &redacted_trace.events[2].payload {
        assert_eq!(path, "~/app.js");
        assert!(!diff
            .as_ref()
            .unwrap()
            .contains("sk-ant-api03-1234567890abcdef1234567890-xyz"));
    } else {
        panic!("unexpected payload");
    }
}

#[test]
fn test_all_event_variants_redaction() {
    let redactor = Redactor::new();
    let now = Utc::now();

    // Assistant message with thinking
    let event = NormalizedEvent::new(
        1,
        now,
        EventPayload::AssistantMessage {
            content: "Found token: sk-abcdef1234567890abcdef1234567890".to_string(),
            thinking: Some("Let's analyze password: PASSWORD=supersecret_pass123".to_string()),
        },
    );
    let redacted = redactor.redact_event(&event);
    if let EventPayload::AssistantMessage { content, thinking } = &redacted.payload {
        assert!(content.contains("[REDACTED_API_KEY]"));
        assert!(!content.contains("sk-abcdef1234567890abcdef1234567890"));
        let t = thinking.as_ref().unwrap();
        assert!(t.contains("[REDACTED_ENV_VAR]"));
        assert!(!t.contains("supersecret_pass123"));
    } else {
        panic!("expected AssistantMessage");
    }

    // OutcomeEvidence
    let event = NormalizedEvent::new(
        2,
        now,
        EventPayload::OutcomeEvidence(OutcomeEvidence {
            kind: OutcomeKind::ArtifactChanged,
            summary:
                "Updated /Users/alice/repo/index.ts with API key sk-proj-1234567890abcdef1234567890"
                    .to_string(),
            confidence: 0.9,
        }),
    );
    let redacted = redactor.redact_event(&event);
    if let EventPayload::OutcomeEvidence(oe) = &redacted.payload {
        assert_eq!(oe.kind, OutcomeKind::ArtifactChanged);
        assert_eq!(oe.confidence, 0.9);
        assert!(!oe.summary.contains("/Users/alice"));
        assert!(oe.summary.contains("~/repo/index.ts"));
        assert!(oe.summary.contains("[REDACTED_API_KEY]"));
    } else {
        panic!("expected OutcomeEvidence");
    }

    // Error
    let event = NormalizedEvent::new(
        3,
        now,
        EventPayload::Error {
            message: "Connection failed to postgres://user:secret123@10.0.0.1:5432/db".to_string(),
            is_recovered: true,
        },
    );
    let redacted = redactor.redact_event(&event);
    if let EventPayload::Error {
        message,
        is_recovered,
    } = &redacted.payload
    {
        assert!(is_recovered);
        assert!(!message.contains("secret123"));
        assert!(!message.contains("10.0.0.1"));
        assert!(message.contains("[REDACTED_CREDENTIALS]"));
        assert!(message.contains("[REDACTED_IP]"));
    } else {
        panic!("expected Error");
    }

    // HumanIntervention
    let event = NormalizedEvent::new(
        4,
        now,
        EventPayload::HumanIntervention(agentworth_schema::HumanIntervention {
            action: "User sent email to dev@company.com".to_string(),
            details: Some("Attached /home/bob/secret.env".to_string()),
        }),
    );
    let redacted = redactor.redact_event(&event);
    if let EventPayload::HumanIntervention(hi) = &redacted.payload {
        assert!(hi.action.contains("[REDACTED_EMAIL]"));
        assert_eq!(hi.details.as_deref(), Some("Attached ~/secret.env"));
    } else {
        panic!("expected HumanIntervention");
    }

    // Custom
    let event = NormalizedEvent::new(
        5,
        now,
        EventPayload::Custom {
            kind: "custom_step_/Users/alice".to_string(),
            data: json!({
                "secret_list": ["sk-1234567890abcdef1234567890", "plain_text"],
                "contact": "support@domain.xyz"
            }),
        },
    );
    let redacted = redactor.redact_event(&event);
    if let EventPayload::Custom { kind, data } = &redacted.payload {
        assert_eq!(kind, "custom_step_~");
        assert_eq!(data["secret_list"][0], "[REDACTED_API_KEY]");
        assert_eq!(data["secret_list"][1], "plain_text");
        assert_eq!(data["contact"], "[REDACTED_EMAIL]");
    } else {
        panic!("expected Custom");
    }
}

// --- High-entropy fallback detector ---------------------------------------
//
// The regex rules above only catch secrets with a known shape. These tests cover
// the complementary fallback: Shannon-entropy detection of secret-*shaped* strings
// that don't match any named vendor pattern. The false-positive tests are the
// point of this layer — git SHAs, UUIDs, and content-hash fingerprints are exactly
// the high-entropy-looking strings that are routine, non-secret data in AgentWorth's
// own session/provenance model (see crates/schema, crates/adapters), and must
// never be redacted.

#[test]
fn test_high_entropy_detector_flags_novel_secret_shapes() {
    let redactor = Redactor::new();

    // A made-up prefix that doesn't match any of the 13 named formats above.
    let text = "Found leaked credential: xk_live_9fH2mQ7vB4nR8pL1sT6wZ3jC0aD5eG in output";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("9fH2mQ7vB4nR8pL1sT6wZ3jC0aD5eG"));
    assert!(redacted.contains("[REDACTED_HIGH_ENTROPY_SECRET]"));

    // A real-shaped AWS secret access key, standalone with no `AWS_SECRET_ACCESS_KEY=`
    // label to trigger the env-var rule -- today this leaks straight through.
    let text = "the value was wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY at the time";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"));
    assert!(redacted.contains("[REDACTED_HIGH_ENTROPY_SECRET]"));

    // A Stripe-shaped test key with no `STRIPE_SECRET_KEY=` label -- the existing
    // env-var test only proves the *labeled* form is caught; this is the gap.
    let text = "printed sk_test_51Mzxyz12345abcdefGHIJKL9876 to the console by mistake";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("sk_test_51Mzxyz12345abcdefGHIJKL9876"));
    assert!(redacted.contains("[REDACTED_HIGH_ENTROPY_SECRET]"));

    // A generic random mixed-case/digit blob with no recognizable prefix at all.
    let text = "unlabeled token dump: Kx9mQ2vLpT4wZ8nR1jH6fD3gB0cA5eSy7uNiO end of dump";
    let redacted = redactor.redact_text(text);
    assert!(!redacted.contains("Kx9mQ2vLpT4wZ8nR1jH6fD3gB0cA5eSy7uNiO"));
    assert!(redacted.contains("[REDACTED_HIGH_ENTROPY_SECRET]"));
}

#[test]
fn test_high_entropy_detector_report_category() {
    let redactor = Redactor::new();
    let text = "unlabeled token dump: Kx9mQ2vLpT4wZ8nR1jH6fD3gB0cA5eSy7uNiO end of dump";
    let mut report = RedactionReport::new();
    let redacted = redactor.redact_text_with_counts(text, &mut report);

    assert!(redacted.contains("[REDACTED_HIGH_ENTROPY_SECRET]"));
    assert_eq!(report.high_entropy_secrets_count, 1);
    assert_eq!(report.total(), 1);
    assert_eq!(
        report.breakdown_by_category.get("High-Entropy Secret"),
        Some(&1)
    );
}

#[test]
fn test_high_entropy_detector_spares_git_shas() {
    let redactor = Redactor::new();

    let text = "Merge commit a1b2c3d4e5f60718293a4b5c6d7e8f9012345678 into main";
    let redacted = redactor.redact_text(text);
    assert_eq!(redacted, text, "full 40-char git SHA must survive untouched");

    // Both 32 hex chars: long enough to clear min_length, so this specifically
    // exercises the pure-hex exclusion rather than the length gate.
    let text = "seen in blame output: 9f86d081884c7d659a2feaa0c55ad015 and c438719d2b0f00a08cd15d6c15b0f00a";
    let redacted = redactor.redact_text(text);
    assert_eq!(
        redacted, text,
        "long hex-only SHA fragments must survive untouched"
    );
}

#[test]
fn test_high_entropy_detector_spares_uuids() {
    let redactor = Redactor::new();

    let text = "Session f47ac10b-58cc-4372-a567-0e02b2c3d479 completed successfully";
    let redacted = redactor.redact_text(text);
    assert_eq!(redacted, text, "dashed UUID v4 must survive untouched");

    let text = "trace id f47ac10b58cc4372a5670e02b2c3d479 was recorded for this run";
    let redacted = redactor.redact_text(text);
    assert_eq!(
        redacted, text,
        "undashed UUID (pure hex) must survive untouched"
    );
}

#[test]
fn test_high_entropy_detector_spares_content_fingerprint_hashes() {
    let redactor = Redactor::new();

    // Shape matches `agentworth_schema::Provenance::content_fingerprint` and
    // `compute_fast_fingerprint` in crates/adapter-sdk: hex::encode(Sha256::finalize()).
    let prov = Provenance::new(
        "claude-session.jsonl",
        "claude_code",
        4096,
        1_720_000_000,
        "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
    );
    let redacted_fingerprint = redactor.redact_text(&prov.content_fingerprint);
    assert_eq!(
        redacted_fingerprint, prov.content_fingerprint,
        "a SHA-256 content fingerprint must never be flagged as a secret"
    );

    // Fallback session-id shape used by every adapter's derive_session_id() when a
    // filename stem isn't available (crates/adapters/src/*.rs): uuid::Uuid::new_v4().
    let session_id = "f47ac10b-58cc-4372-a567-0e02b2c3d479".to_string();
    let redacted_session_id = redactor.redact_text(&session_id);
    assert_eq!(redacted_session_id, session_id);
}

#[test]
fn test_high_entropy_detector_spares_agentworth_identifiers() {
    let redactor = Redactor::new();

    // Real snake_case signature, lifted from crates/storage/src/lib.rs.
    let text = "fn find_sessions_for_blame(file_path_pattern: &str) -> Result<Vec<BlameMatch>>";
    assert_eq!(redactor.redact_text(text), text);

    // Adapter test-fixture session-id slugs (crates/adapters/src/hermes.rs, storage tests).
    let text = "session hermes-session-007 and sess_blame_1 are both routine slugs, not secrets";
    assert_eq!(redactor.redact_text(text), text);

    // A long kebab-case CLI flag/name, and a CONSTANT_CASE env var *name* (not value).
    let text = "run with --high-entropy-secret-detector-threshold-override flag set";
    assert_eq!(redactor.redact_text(text), text);

    // An adversarial, deliberately long chained camelCase identifier -- the hardest
    // realistic case for a naive entropy check, since it's what the shape checks
    // (hex/underscore exclusion) can't split apart.
    let text = "pub fn computeFastFingerprintFromPathAndSizeAndMtime(path, size, mtime)";
    assert_eq!(redactor.redact_text(text), text);
}

#[test]
fn test_high_entropy_detector_selective_within_mixed_shell_output() {
    let redactor = Redactor::new();

    let event = NormalizedEvent::new(
        1,
        Utc::now(),
        EventPayload::ShellCommand(ShellCommand {
            command: "git log --oneline -1 && env | grep TOKEN".to_string(),
            cwd: Some("~/code/unfoundbox/agentworth".to_string()),
            exit_code: Some(0),
            output: Some(
                "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678 fix: entropy detector\n\
                 SOME_NEW_SERVICE_TOKEN=xk_live_9fH2mQ7vB4nR8pL1sT6wZ3jC0aD5eG"
                    .to_string(),
            ),
        }),
    );

    let redacted = redactor.redact_event(&event);
    if let EventPayload::ShellCommand(cmd) = &redacted.payload {
        let output = cmd.output.as_ref().unwrap();
        assert!(
            output.contains("a1b2c3d4e5f60718293a4b5c6d7e8f9012345678"),
            "git SHA in the same blob must survive: {output}"
        );
        assert!(
            !output.contains("9fH2mQ7vB4nR8pL1sT6wZ3jC0aD5eG"),
            "the secret in the same blob must be redacted: {output}"
        );
    } else {
        panic!("expected ShellCommand");
    }
}

#[test]
fn test_high_entropy_detector_spares_short_random_token() {
    // Below the 24-char minimum length, random-token and natural-identifier entropy
    // distributions overlap too much to call reliably -- so this detector deliberately
    // doesn't try. Short well-known formats are the named regex rules' job.
    let redactor = Redactor::new();
    let text = "short token abc123XYZ789 seen here";
    assert_eq!(redactor.redact_text(text), text);
}

#[test]
fn test_custom_rule_addition() {
    use regex::Regex;

    let mut redactor = Redactor::new();
    let ssn_rule = RedactionRule::new(
        "ssn",
        RedactionCategory::Custom,
        Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(),
        "[REDACTED_SSN]",
    );
    redactor.add_rule(ssn_rule);

    let text = "User SSN is 000-12-3456.";
    let mut report = RedactionReport::new();
    let redacted = redactor.redact_text_with_counts(text, &mut report);
    assert_eq!(redacted, "User SSN is [REDACTED_SSN].");
    assert_eq!(report.custom_count, 1);
    assert_eq!(report.total(), 1);
}
