//! Tests for the closed normalized host event source-class vocabulary.
//!
//! The contract table is an exhaustive match, so adding or removing a variant
//! breaks compilation until the contract and complete set are deliberately updated.

use std::collections::HashSet;

use super::*;

fn source_class_contract(source_class: NormalizedHostEventSourceClass) -> (usize, &'static str) {
    match source_class {
        NormalizedHostEventSourceClass::HostHook => (0, "HOST_HOOK"),
        NormalizedHostEventSourceClass::WorkerDispatch => (1, "WORKER_DISPATCH"),
        NormalizedHostEventSourceClass::Elicitation => (2, "ELICITATION"),
        NormalizedHostEventSourceClass::CoreDriven => (3, "CORE_DRIVEN"),
    }
}

#[test]
fn source_class_vocabulary_is_exactly_four_values() {
    assert_eq!(NormalizedHostEventSourceClass::ALL.len(), 4);

    let distinct: HashSet<NormalizedHostEventSourceClass> = NormalizedHostEventSourceClass::ALL
        .iter()
        .copied()
        .collect();
    assert_eq!(
        distinct.len(),
        4,
        "source classes must be pairwise distinct"
    );

    let distinct_strings: HashSet<&'static str> = NormalizedHostEventSourceClass::ALL
        .iter()
        .map(|source_class| source_class.as_str())
        .collect();
    assert_eq!(
        distinct_strings.len(),
        4,
        "canonical strings must be pairwise distinct"
    );

    for source_class in NormalizedHostEventSourceClass::ALL {
        let (index, canonical) = source_class_contract(source_class);
        assert_eq!(source_class.as_str(), canonical);
        assert_eq!(NormalizedHostEventSourceClass::ALL[index], source_class);
    }
}
