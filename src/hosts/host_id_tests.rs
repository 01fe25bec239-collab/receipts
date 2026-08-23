//! Tests for the closed [`HostId`] identity set.

use std::collections::HashSet;

use super::*;

/// The identity set is exactly the three frozen hosts: all three are
/// representable, they are pairwise distinct, and the match is exhaustive,
/// which proves no fourth identity is representable.
#[test]
fn identity_set_is_exactly_the_three_frozen_hosts() {
    let all = [
        (HostId::ClaudeCode, "CLAUDE_CODE"),
        (HostId::Codex, "CODEX"),
        (HostId::Headless, "HEADLESS"),
    ];

    let distinct: HashSet<HostId> = all.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        distinct.len(),
        3,
        "host identities must be pairwise distinct"
    );

    for (id, expected) in all {
        // Exhaustive match: adding or removing a variant breaks compilation.
        let rendered = match id {
            HostId::ClaudeCode => "CLAUDE_CODE",
            HostId::Codex => "CODEX",
            HostId::Headless => "HEADLESS",
        };
        assert_eq!(rendered, expected);
        assert_eq!(id.as_str(), expected);
    }
}

/// Identity values carry no hidden behavior: copying, comparing, and
/// hashing stay value-level.
#[test]
fn identities_are_plain_values() {
    let original = HostId::ClaudeCode;
    let copy = original;
    assert_eq!(original, copy);
    assert_ne!(HostId::Codex, HostId::Headless);
}
