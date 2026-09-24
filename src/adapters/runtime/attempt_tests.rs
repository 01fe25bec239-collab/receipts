use std::{any::TypeId, error::Error};

use crate::{
    AttemptId, ClaudeCapabilityEvidence, ClaudeCapabilityProbeReport, CodexCapabilityEvidence,
    CodexCapabilityProbeReport, RuntimeCapabilities,
};

const SECRET: &str = "sk-test-NOT-A-REAL-CREDENTIAL-A3-012";

#[test]
fn attempt_identity_preserves_frozen_unicode_length_and_exact_caller_bytes() {
    for value in [
        "a".into(),
        " ".into(),
        "\0\n".into(),
        " e\u{301} 世界 ".into(),
        "🦀".repeat(200),
    ] {
        let id = AttemptId::new(&value).unwrap();
        assert_eq!(id.as_str(), value);
        assert_eq!(id, id.clone());
        assert!(id.as_str().len() <= 800);
    }
    for value in [String::new(), "x".repeat(201), "🦀".repeat(201)] {
        assert!(AttemptId::new(&value).is_err());
    }
    // No normalization, trimming, uniqueness assertion, or generated identity.
    assert_ne!(
        AttemptId::new("é").unwrap(),
        AttemptId::new("e\u{301}").unwrap()
    );
    assert_ne!(AttemptId::new("a").unwrap(), AttemptId::new(" a ").unwrap());
    assert_eq!(
        AttemptId::new("same").unwrap(),
        AttemptId::new("same").unwrap()
    );
}

#[test]
fn identity_and_validation_errors_do_not_format_caller_values() {
    let id = AttemptId::new(SECRET).unwrap();
    assert!(!format!("{id:?}").contains(SECRET));
    let error = AttemptId::new(&SECRET.repeat(200)).unwrap_err();
    assert!(!format!("{error:?}").contains(SECRET));
    assert!(!error.to_string().contains(SECRET));
    assert!(error.source().is_none());
}

#[test]
fn capabilities_carrier_preserves_both_reports_and_whole_report_unknown() {
    assert_ne!(
        TypeId::of::<RuntimeCapabilities>(),
        TypeId::of::<CodexCapabilityProbeReport>()
    );
    let codex = CodexCapabilityProbeReport {
        version: "codex synthetic".into(),
        json: CodexCapabilityEvidence::Supported,
        output_schema: CodexCapabilityEvidence::Unknown,
        sandbox: CodexCapabilityEvidence::Supported,
    };
    let claude = ClaudeCapabilityProbeReport {
        version: "claude synthetic".into(),
        print: ClaudeCapabilityEvidence::Supported,
        input_format: ClaudeCapabilityEvidence::Unknown,
        output_format: ClaudeCapabilityEvidence::Supported,
        no_session_persistence: ClaudeCapabilityEvidence::Unknown,
        permission_mode: ClaudeCapabilityEvidence::Supported,
        verbose: ClaudeCapabilityEvidence::Unknown,
    };

    match RuntimeCapabilities::Codex(codex.clone()) {
        RuntimeCapabilities::Codex(report) => assert_eq!(report, codex),
        _ => panic!("Codex report changed variant"),
    }
    match RuntimeCapabilities::Claude(claude.clone()) {
        RuntimeCapabilities::Claude(report) => assert_eq!(report, claude),
        _ => panic!("Claude report changed variant"),
    }
    assert_ne!(
        RuntimeCapabilities::Unknown,
        RuntimeCapabilities::Codex(codex)
    );
    assert_ne!(
        RuntimeCapabilities::Unknown,
        RuntimeCapabilities::Claude(claude)
    );

    // Exhaustive matching keeps Unknown a separate no-evidence state.
    let _: fn(RuntimeCapabilities) = |capabilities| match capabilities {
        RuntimeCapabilities::Codex(_) => {}
        RuntimeCapabilities::Claude(_) => {}
        RuntimeCapabilities::Unknown => {}
    };
}

#[test]
fn physical_bindings_add_no_shadow_contract_or_process_authority() {
    for source in [include_str!("attempt.rs"), include_str!("raw_failure.rs")] {
        for name in [
            "Model",
            "ModelRef",
            "TaskCapsule",
            "RepairCapsule",
            "ReviewCapsule",
            "RuntimeCapsuleFamily",
            "WorkspaceHandle",
            "DispatchAdmissionDecision",
        ] {
            for declaration in ["struct", "enum", "type"] {
                assert!(!source.contains(&format!("{declaration} {name}")));
            }
        }
        for forbidden in [
            "std::process",
            "Command::",
            "Child",
            "SIGTERM",
            "SIGKILL",
            "waitpid",
            "killpg",
            "setpgid",
            "std::thread",
            "AtomicBool",
            "Mutex",
            "impl Drop",
            "ManuallyDrop",
            "mem::forget",
            "serde_json",
            "impl RuntimeAdapter for",
        ] {
            assert!(
                !source.contains(forbidden),
                "unexpected authority: {forbidden}"
            );
        }
    }
    let attempt = include_str!("attempt.rs");
    assert!(attempt.contains("codex: CodexLiveAttempt"));
    assert!(attempt.contains("codex: CodexLiveOutcome"));
}
