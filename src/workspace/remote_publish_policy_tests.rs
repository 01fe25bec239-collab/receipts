use crate::WorkspaceRemotePublishPolicy;

#[test]
fn all_has_exact_count() {
    assert_eq!(WorkspaceRemotePublishPolicy::ALL.len(), 3);
}

#[test]
fn all_has_schema_order() {
    assert_eq!(
        WorkspaceRemotePublishPolicy::ALL,
        [
            WorkspaceRemotePublishPolicy::LocalOnly,
            WorkspaceRemotePublishPolicy::PushOnAccept,
            WorkspaceRemotePublishPolicy::PushAlways,
        ]
    );
    assert_eq!(
        WorkspaceRemotePublishPolicy::ALL.map(WorkspaceRemotePublishPolicy::as_str),
        ["LOCAL_ONLY", "PUSH_ON_ACCEPT", "PUSH_ALWAYS"]
    );
}

#[test]
fn variants_have_exact_canonical_strings() {
    assert_eq!(
        WorkspaceRemotePublishPolicy::LocalOnly.as_str(),
        "LOCAL_ONLY"
    );
    assert_eq!(
        WorkspaceRemotePublishPolicy::PushOnAccept.as_str(),
        "PUSH_ON_ACCEPT"
    );
    assert_eq!(
        WorkspaceRemotePublishPolicy::PushAlways.as_str(),
        "PUSH_ALWAYS"
    );
}

#[test]
fn enum_values_and_canonical_strings_are_pairwise_unique() {
    let [local_only, push_on_accept, push_always] = WorkspaceRemotePublishPolicy::ALL;

    assert_ne!(local_only, push_on_accept);
    assert_ne!(local_only, push_always);
    assert_ne!(push_on_accept, push_always);

    assert_ne!(local_only.as_str(), push_on_accept.as_str());
    assert_ne!(local_only.as_str(), push_always.as_str());
    assert_ne!(push_on_accept.as_str(), push_always.as_str());
}

#[test]
fn exhaustive_mapping_agrees_with_public_accessor() {
    fn exhaustive(policy: WorkspaceRemotePublishPolicy) -> &'static str {
        match policy {
            WorkspaceRemotePublishPolicy::LocalOnly => "LOCAL_ONLY",
            WorkspaceRemotePublishPolicy::PushOnAccept => "PUSH_ON_ACCEPT",
            WorkspaceRemotePublishPolicy::PushAlways => "PUSH_ALWAYS",
        }
    }

    for policy in WorkspaceRemotePublishPolicy::ALL {
        assert_eq!(exhaustive(policy), policy.as_str());
    }
}
