use std::error::Error;

use crate::{
    CapabilityStatus, ExtensionCapabilityReport, ExtensionCapabilityReportError,
    RuntimeCapabilities,
};
use CapabilityStatus::{Supported, Unknown, Unsupported};

const SECRET: &str = "sk-test-NOT-A-REAL-CREDENTIAL-RA-W1-4";

fn report(
    id: &str,
    entries: &[(&str, CapabilityStatus)],
) -> Result<ExtensionCapabilityReport, ExtensionCapabilityReportError> {
    ExtensionCapabilityReport::new(id, entries.iter().copied())
}

#[test]
fn identity_and_names_are_open_exact_strings() {
    let long = "🦀".repeat(10_000);
    let names = [
        " ",
        "\0\n",
        " e\u{301} 世界 ",
        "é",
        "e\u{301}",
        "A",
        "a",
        "a.b..c",
        "a b",
        long.as_str(),
    ];
    for id in [
        " ",
        "\0",
        " e\u{301} ",
        "Runtime.With/Any:Shape",
        long.as_str(),
    ] {
        let entries: Vec<_> = names.iter().map(|name| (*name, Supported)).collect();
        let built = report(id, &entries).unwrap();
        assert_eq!(built.runtime_id(), id);
        assert_eq!(built.entries().count(), names.len());
        for name in names {
            assert_eq!(built.get(name), Some(Supported), "{name:?}");
        }
    }
    // No trimming, case folding or Unicode normalization.
    let built = report("r", &[("a", Supported)]).unwrap();
    for other in [" a", "a ", "A", "á"] {
        assert_eq!(built.get(other), None);
    }
    assert_ne!(report("r", &[]), report(" r", &[]));
    assert_ne!(report("é", &[]), report("e\u{301}", &[]));
}

#[test]
fn entries_are_deterministic_and_independent_of_input_order() {
    let forward = [
        ("b", Supported),
        ("a", Unsupported),
        ("é", Unknown),
        ("C", Supported),
    ];
    let mut reversed = forward;
    reversed.reverse();
    let one = report("r", &forward).unwrap();
    let two = report("r", &reversed).unwrap();
    assert_eq!(one, two);
    let observed: Vec<_> = one.entries().collect();
    assert_eq!(
        observed,
        [
            ("C", Supported),
            ("a", Unsupported),
            ("b", Supported),
            ("é", Unknown)
        ]
    );
    assert_eq!(observed, two.entries().collect::<Vec<_>>());
    assert_eq!(one.clone(), one);
}

#[test]
fn statuses_stay_distinct_and_absence_is_never_supported_or_unsupported() {
    let built = report(
        "r",
        &[("yes", Supported), ("no", Unsupported), ("maybe", Unknown)],
    )
    .unwrap();
    assert_eq!(built.get("yes"), Some(Supported));
    assert_eq!(built.get("no"), Some(Unsupported));
    assert_eq!(built.get("maybe"), Some(Unknown));
    assert_eq!(built.get("missing"), None);
    assert_eq!(built.status("yes"), Supported);
    assert_eq!(built.status("no"), Unsupported);
    assert_eq!(built.status("maybe"), Unknown);
    assert_eq!(built.status("missing"), Unknown);
    assert_eq!(built.status(""), Unknown);
    // A present Unknown remains observable and is not the same report as absence.
    assert_eq!(built.entries().count(), 3);
    assert_ne!(
        report("r", &[("maybe", Unknown)]).unwrap(),
        report("r", &[]).unwrap()
    );
}

#[test]
fn empty_report_is_valid_and_implies_no_capability() {
    let empty = report("r", &[]).unwrap();
    assert_eq!(empty.entries().count(), 0);
    assert_eq!(empty.get("anything"), None);
    assert_eq!(empty.status("anything"), Unknown);
    assert_ne!(
        RuntimeCapabilities::Extension(empty.clone()),
        RuntimeCapabilities::Unknown
    );
    assert_ne!(empty, report("other", &[]).unwrap());
}

#[test]
fn empty_identifiers_and_duplicates_refuse_without_a_partial_report() {
    assert_eq!(
        report("", &[("a", Supported)]),
        Err(ExtensionCapabilityReportError::EmptyRuntimeId)
    );
    assert_eq!(
        report("", &[]),
        Err(ExtensionCapabilityReportError::EmptyRuntimeId)
    );
    assert_eq!(
        report("r", &[("a", Supported), ("", Supported)]),
        Err(ExtensionCapabilityReportError::EmptyCapabilityName)
    );
    // Duplicates never overwrite, whether the status agrees, differs or is Unknown.
    for (first, second) in [
        (Supported, Supported),
        (Supported, Unsupported),
        (Unsupported, Unknown),
        (Unknown, Supported),
    ] {
        assert_eq!(
            report("r", &[("ok", Supported), ("dup", first), ("dup", second)]),
            Err(ExtensionCapabilityReportError::DuplicateCapabilityName)
        );
    }
    // Only exact repeats are duplicates.
    assert!(
        report(
            "r",
            &[("dup", Supported), ("Dup", Unsupported), ("dup ", Unknown)]
        )
        .is_ok()
    );
    // Owned strings are accepted through the same constructor.
    let owned = ExtensionCapabilityReport::new("r", vec![(String::from("a"), Supported)]).unwrap();
    assert_eq!(owned.get("a"), Some(Supported));
}

#[test]
fn diagnostics_do_not_format_caller_identifiers_or_values() {
    let built = report(SECRET, &[(SECRET, Supported)]).unwrap();
    let carrier = RuntimeCapabilities::Extension(built.clone());
    for text in [
        format!("{built:?}"),
        format!("{built:#?}"),
        format!("{carrier:?}"),
    ] {
        assert!(!text.contains(SECRET), "{text}");
    }
    for error in [
        report("", &[(SECRET, Supported)]).unwrap_err(),
        report(SECRET, &[("", Supported)]).unwrap_err(),
        report(SECRET, &[(SECRET, Supported), (SECRET, Unsupported)]).unwrap_err(),
    ] {
        assert!(!format!("{error:?}").contains(SECRET));
        assert!(!error.to_string().contains(SECRET));
        assert!(error.source().is_none());
    }
}
