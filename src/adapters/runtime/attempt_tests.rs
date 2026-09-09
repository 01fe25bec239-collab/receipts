use std::{any::TypeId, error::Error};

use crate::{AttemptId, CodexCapabilityEvidence, CodexCapabilityProbeReport, RuntimeCapabilities};

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
fn capabilities_binding_is_the_original_report_with_no_second_inference() {
    assert_eq!(
        TypeId::of::<RuntimeCapabilities>(),
        TypeId::of::<CodexCapabilityProbeReport>()
    );
    let report = CodexCapabilityProbeReport {
        version: "synthetic".into(),
        json: CodexCapabilityEvidence::Supported,
        output_schema: CodexCapabilityEvidence::Unknown,
        sandbox: CodexCapabilityEvidence::Unknown,
    };
    let binding: &RuntimeCapabilities = &report;
    assert!(std::ptr::eq(binding, &report));
    assert_eq!(binding.output_schema, CodexCapabilityEvidence::Unknown);
    assert_eq!(binding.sandbox, CodexCapabilityEvidence::Unknown);
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
