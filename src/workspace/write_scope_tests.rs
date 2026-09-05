use super::*;
use crate::test_support::{TempDir, TestRepo, git};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn verify(
    repo: &TestRepo,
    base: &str,
    candidate: &str,
    allowed: &[&str],
    forbidden: &[&str],
) -> Result<WriteScopeVerification, WriteScopeVerificationError> {
    verify_write_scope(
        repo.path(),
        base,
        candidate,
        &strings(allowed),
        &strings(forbidden),
    )
}

fn commit_all(repo: &TestRepo) -> String {
    git(repo.path(), &["add", "--all"]);
    git(
        repo.path(),
        &["commit", "--quiet", "--allow-empty", "-m", "fixture"],
    );
    repo.head_sha()
}

#[test]
fn a0_matching_matrix() {
    for (pattern, path, expected) in [
        ("Cargo.toml", "Cargo.toml", true),
        ("Cargo.toml", "cargo.toml", false),
        ("src/*.rs", "src/lib.rs", true),
        ("src/*.rs", "src/.hidden.rs", true),
        ("src/*.rs", "src/core/lib.rs", false),
        ("*", ".env", true),
        ("*", "a/b", false),
        ("a*b*c", "abc", true),
        ("a*b*c", "axbyc", true),
        ("a*b*c", "acb", false),
        ("**/foo.rs", "foo.rs", true),
        ("**/foo.rs", "src/foo.rs", true),
        ("**/foo.rs", "src/core/foo.rs", true),
        ("src/**/foo.rs", "src/foo.rs", true),
        ("src/**/foo.rs", "src/core/foo.rs", true),
        ("src/**/foo.rs", "src/a/b/foo.rs", true),
        ("src/**", "src", true),
        ("src/**", "src/a", true),
        ("src/**", "src/a/b", true),
        ("src/**", "src2/a", false),
        ("**/**", "foo", true),
        ("**/**", "a/b/c", true),
        ("**/*.yml", ".github/workflows/ci.yml", true),
        ("a!b", "a!b", true),
        ("a/!b", "a/!b", true),
        ("é/猫*.rs", "é/猫🦀.rs", true),
        ("é", "e\u{301}", false),
        ("e\u{301}", "é", false),
        ("É", "é", false),
        ("*", "*", true),
        ("a*b", "a*b", true),
        ("a*b", "ab", true),
        ("a*b", "a?[]{}!b", true),
        ("a+$^()|", "a+$^()|", true),
    ] {
        validate_structure(path).unwrap();
        assert_eq!(
            Pattern::parse(pattern).unwrap().matches(path),
            expected,
            "{pattern:?} / {path:?}"
        );
    }
    // Empty components are invalid paths, but a star itself consumes zero scalars.
    assert!(match_component(&['*'], ""));
    assert!(match_component(&['a', '*', 'b'], "ab"));
    assert!(!match_component(&['é'], "e\u{301}"));
    let long = format!("{}z", "a*".repeat(128));
    assert!(!Pattern::parse(&long).unwrap().matches(&"a".repeat(256)));
}

#[test]
fn a0_rejects_only_specified_pattern_syntax() {
    for pattern in [
        "foo**bar", "**foo", "foo**", "***", "a***b", "?", "[a]", "a]", "{a,b}", "a}", "!a", "\\",
        "/a", "./a", "a/", ".", "..", "a/./b", "a/../b", "a//b", "", "a/**b/c",
    ] {
        assert!(Pattern::parse(pattern).is_err(), "{pattern:?}");
    }
    // No invented reserved characters or normalization.
    for pattern in [
        "a!", "a/!b", "a:b", "a b", "a\tb", "a\nb", "a\0b", "a+$^()|", "**/**", "a*b*c",
    ] {
        assert!(Pattern::parse(pattern).is_ok(), "{pattern:?}");
    }
}

#[test]
fn candidate_validation_is_structural_not_pattern_grammar() {
    for path in [
        "", "/a", "./a", "a/", ".", "..", "a/./b", "a/../b", "a//b", "a\\b",
    ] {
        assert!(validate_structure(path).is_err(), "{path:?}");
    }
    for path in ["?", "[a]", "{a}", "!a", "*", "**", "a\tb", "a\nb"] {
        assert!(validate_structure(path).is_ok(), "{path:?}");
    }
}

#[test]
fn raw_nul_parser_is_strict_lossless_and_sorted() {
    assert!(parse_paths(vec![]).unwrap().is_empty());
    for raw in [b"a".as_slice(), b"a\0b", b"\0", b"a\0\0"] {
        assert!(matches!(
            parse_paths(raw.to_vec()),
            Err(WriteScopeVerificationError::MalformedGitOutput { .. })
        ));
    }
    assert!(
        matches!(parse_paths(vec![b'a', 0xff, 0]), Err(WriteScopeVerificationError::InvalidUtf8Candidate { bytes }) if bytes == [b'a', 0xff])
    );
    assert!(
        matches!(parse_paths(b"a\\b\0".to_vec()), Err(WriteScopeVerificationError::MalformedCandidatePath { path, .. }) if path == "a\\b")
    );
    assert_eq!(
        parse_paths(b"z\0a\nb\0a b\0a\tb\0z\0".to_vec()).unwrap(),
        strings(&["a\tb", "a\nb", "a b", "z"])
    );
}

#[test]
fn malformed_entry_invalidates_either_whole_policy_even_on_empty_diff() {
    let repo = TestRepo::new("scope-policy");
    let sha = repo.head_sha();
    for set in [
        WriteScopePatternSet::Allowed,
        WriteScopePatternSet::Forbidden,
    ] {
        let bad = strings(&["**", "src/**x", "Cargo.toml"]);
        let good = strings(&["**"]);
        let (allowed, forbidden) = if set == WriteScopePatternSet::Allowed {
            (&bad, &good)
        } else {
            (&good, &bad)
        };
        assert!(
            matches!(verify_write_scope(repo.path(), &sha, &sha, allowed, forbidden), Err(WriteScopeVerificationError::InvalidPattern { set: actual, index: 1, pattern, reason: _ }) if actual == set && pattern == "src/**x")
        );
    }
}

#[test]
fn additions_modifications_deletions_and_empty_policies() {
    let repo = TestRepo::new("scope-changes");
    let seed = repo.head_sha();
    let added = repo.commit_file("src/lib.rs", "one");
    let modified = repo.commit_file("src/lib.rs", "two");
    std::fs::remove_file(repo.path().join("src/lib.rs")).unwrap();
    let deleted = commit_all(&repo);
    for (base, candidate) in [(&seed, &added), (&added, &modified), (&modified, &deleted)] {
        let result = verify(&repo, base, candidate, &["src/*.rs"], &[]).unwrap();
        assert_eq!(result.verification().as_str(), "PASS");
        assert_eq!(result.changed_paths(), strings(&["src/lib.rs"]));
        assert_eq!(result.baseline_sha().as_str(), base);
        assert_eq!(result.candidate_sha().as_str(), candidate);
        assert!(result.unauthorized_paths().is_empty());
        assert!(result.forbidden_matches().is_empty());
        let empty = verify(&repo, base, candidate, &[], &[]).unwrap();
        assert_eq!(empty.verification().as_str(), "FAIL");
        assert_eq!(empty.unauthorized_paths(), strings(&["src/lib.rs"]));
    }
    let empty = verify(&repo, &deleted, &deleted, &[], &[]).unwrap();
    assert_eq!(empty.verification(), WriteScopeVerificationStatus::Pass);
    assert!(empty.changed_paths().is_empty());
}

#[test]
fn forbidden_precedence_all_matches_and_deterministic_evidence() {
    let repo = TestRepo::new("scope-forbidden");
    let base = repo.head_sha();
    repo.commit_file("z.rs", "z");
    repo.commit_file("a.rs", "a");
    let candidate = repo.commit_file("outside", "x");
    let result = verify(
        &repo,
        &base,
        &candidate,
        &["*.rs"],
        &["z*", "**", "a*", "**"],
    )
    .unwrap();
    assert_eq!(result.verification(), WriteScopeVerificationStatus::Fail);
    assert_eq!(
        result.changed_paths(),
        strings(&["a.rs", "outside", "z.rs"])
    );
    assert_eq!(result.unauthorized_paths(), strings(&["outside"]));
    let evidence: Vec<_> = result
        .forbidden_matches()
        .iter()
        .map(|item| (item.path(), item.pattern()))
        .collect();
    assert_eq!(
        evidence,
        [
            ("a.rs", "**"),
            ("a.rs", "a*"),
            ("outside", "**"),
            ("z.rs", "**"),
            ("z.rs", "z*")
        ]
    );
    let reordered = verify(&repo, &base, &candidate, &["*.rs"], &["a*", "**", "z*"]).unwrap();
    assert_eq!(result, reordered);
    let no_allowed = verify(&repo, &base, &candidate, &[], &[]).unwrap();
    assert_eq!(no_allowed.unauthorized_paths(), result.changed_paths());
}

#[test]
fn renames_check_both_identities_in_all_security_directions() {
    for (old, new, pass) in [
        ("src/old.rs", "src/new.rs", true),
        ("src/old.rs", "secret/new.rs", false),
        ("secret/old.rs", "src/new.rs", false),
        ("src/old.rs", "docs/escaped.rs", false),
        ("docs/escaped.rs", "src/new.rs", false),
    ] {
        let repo = TestRepo::new("scope-renames");
        let base = repo.commit_file(old, "identical rename contents");
        git(repo.path(), &["config", "diff.renames", "true"]);
        std::fs::create_dir_all(repo.path().join(new).parent().unwrap()).unwrap();
        std::fs::rename(repo.path().join(old), repo.path().join(new)).unwrap();
        let candidate = commit_all(&repo);
        let result = verify(
            &repo,
            &base,
            &candidate,
            &["src/**", "secret/**"],
            &["secret/**"],
        )
        .unwrap();
        assert_eq!(
            result.verification() == WriteScopeVerificationStatus::Pass,
            pass,
            "{old} -> {new}"
        );
        let mut expected = strings(&[old, new]);
        expected.sort();
        assert_eq!(result.changed_paths(), expected);
        for path in [old, new] {
            if path.starts_with("docs/") {
                assert!(result.unauthorized_paths().contains(&path.to_owned()));
            }
            if path.starts_with("secret/") {
                assert!(
                    result
                        .forbidden_matches()
                        .iter()
                        .any(|item| item.path() == path)
                );
            }
        }
    }
}

#[test]
fn copy_destinations_are_checked() {
    let repo = TestRepo::new("scope-copy");
    let base = repo.commit_file("src/source", "copied content");
    std::fs::copy(repo.path().join("src/source"), repo.path().join("src/copy")).unwrap();
    let inside = commit_all(&repo);
    let result = verify(&repo, &base, &inside, &["src/**"], &[]).unwrap();
    assert_eq!(result.verification(), WriteScopeVerificationStatus::Pass);
    assert_eq!(result.changed_paths(), strings(&["src/copy"]));
    std::fs::copy(repo.path().join("src/source"), repo.path().join("outside")).unwrap();
    let outside = commit_all(&repo);
    let result = verify(&repo, &inside, &outside, &["src/**"], &[]).unwrap();
    assert_eq!(result.unauthorized_paths(), strings(&["outside"]));
}

#[test]
fn git_filenames_preserve_unicode_spaces_tabs_newlines_and_pattern_characters() {
    let repo = TestRepo::new("scope-raw-names");
    git(repo.path(), &["config", "core.precomposeunicode", "false"]);
    let base = repo.head_sha();
    let names = [
        "space name",
        "tab\tname",
        "line\nname",
        "nested/猫🦀",
        "?[]{}!*",
        ".env",
    ];
    for name in names {
        repo.commit_file(name, "content");
    }
    let result = verify(&repo, &base, &repo.head_sha(), &["**"], &[]).unwrap();
    let mut expected = strings(&names);
    expected.sort();
    assert_eq!(result.changed_paths(), expected);
    assert_eq!(result.verification(), WriteScopeVerificationStatus::Pass);
}

#[cfg(unix)]
#[test]
fn invalid_utf8_git_filename_fails_with_exact_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let repo = TestRepo::new("scope-invalid-utf8");
    let base = repo.head_sha();
    let bytes = vec![b'b', b'a', b'd', 0xff];
    let name = std::ffi::OsString::from_vec(bytes.clone());
    // macOS filesystems reject invalid UTF-8. Git's index admits the exact
    // OsString path bytes without requiring a filesystem representation.
    std::fs::write(repo.path().join("blob-input"), "content").unwrap();
    let blob =
        String::from_utf8(git(repo.path(), &["hash-object", "-w", "blob-input"]).stdout).unwrap();
    let output = crate::git::prepared_command(
        repo.path(),
        "fixture raw index path",
        &[
            OsStr::new("update-index"),
            OsStr::new("--add"),
            OsStr::new("--cacheinfo"),
            OsStr::new("100644"),
            OsStr::new(blob.trim()),
            &name,
        ],
    )
    .unwrap()
    .output()
    .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    git(repo.path(), &["commit", "--quiet", "-m", "raw pathname"]);
    let candidate = repo.head_sha();
    assert!(
        matches!(verify(&repo, &base, &candidate, &["**"], &[]), Err(WriteScopeVerificationError::InvalidUtf8Candidate { bytes: observed }) if observed == bytes)
    );
}

#[cfg(unix)]
#[test]
fn backslash_git_filename_fails_closed() {
    let repo = TestRepo::new("scope-backslash");
    let base = repo.head_sha();
    let candidate = repo.commit_file("bad\\name", "content");
    assert!(
        matches!(verify(&repo, &base, &candidate, &["**"], &[]), Err(WriteScopeVerificationError::MalformedCandidatePath { path, .. }) if path == "bad\\name")
    );
}

#[test]
fn exact_sha_syntax_and_commit_objects_are_required_independently() {
    let repo = TestRepo::new("scope-shas");
    let sha = repo.head_sha();
    for invalid in [
        "",
        "HEAD",
        "main",
        "abcdef",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "gggggggggggggggggggggggggggggggggggggggg",
    ] {
        assert!(matches!(
            verify(&repo, invalid, &sha, &[], &[]),
            Err(WriteScopeVerificationError::InvalidBaselineSha)
        ));
        assert!(matches!(
            verify(&repo, &sha, invalid, &[], &[]),
            Err(WriteScopeVerificationError::InvalidCandidateSha)
        ));
    }
    let tree = String::from_utf8(git(repo.path(), &["rev-parse", "HEAD^{tree}"]).stdout).unwrap();
    for unavailable in ["0000000000000000000000000000000000000000", tree.trim()] {
        assert!(matches!(
            verify(&repo, unavailable, &sha, &[], &[]),
            Err(WriteScopeVerificationError::CommitUnavailable {
                operation: WriteScopeGitOperation::BaselineCommit,
                ..
            })
        ));
        assert!(matches!(
            verify(&repo, &sha, unavailable, &[], &[]),
            Err(WriteScopeVerificationError::CommitUnavailable {
                operation: WriteScopeGitOperation::CandidateCommit,
                ..
            })
        ));
    }
    // A tag object's exact SHA must not pass by peeling to a commit.
    let tag_data = format!(
        "object {sha}\ntype commit\ntag fixture\ntagger Test <test@example.invalid> 1 +0000\n\nfixture\n"
    );
    std::fs::write(repo.path().join("tag-data"), tag_data).unwrap();
    let tag =
        String::from_utf8(git(repo.path(), &["hash-object", "-t", "tag", "-w", "tag-data"]).stdout)
            .unwrap();
    assert!(matches!(
        verify(&repo, &sha, tag.trim(), &[], &[]),
        Err(WriteScopeVerificationError::CommitUnavailable {
            operation: WriteScopeGitOperation::CandidateCommit,
            ..
        })
    ));
}

#[test]
fn invalid_repository_and_preparation_are_typed_errors() {
    let temp = TempDir::new("scope-no-repo");
    let sha = "0000000000000000000000000000000000000000";
    assert!(matches!(
        verify_write_scope(temp.path(), sha, sha, &[], &[]),
        Err(WriteScopeVerificationError::CommitUnavailable { .. })
    ));
    assert!(matches!(
        verify_write_scope(&temp.path().join("missing"), sha, sha, &[], &[]),
        Err(WriteScopeVerificationError::GitPreparation { .. })
    ));
}

#[test]
fn natural_diff_failure_after_valid_commit_checks_is_typed() {
    let repo = TestRepo::new("scope-diff-failure");
    let base = repo.head_sha();
    let candidate = repo.commit_file("file", "content");
    let tree = String::from_utf8(git(repo.path(), &["rev-parse", "HEAD^{tree}"]).stdout).unwrap();
    let tree = tree.trim();
    // Remove only this throwaway fixture's tree object, leaving both commit
    // objects intact. cat-file -t still succeeds; diff must fail closed.
    std::fs::remove_file(
        repo.path()
            .join(".git/objects")
            .join(&tree[..2])
            .join(&tree[2..]),
    )
    .unwrap();
    verify_commit(
        repo.path(),
        &CommitSha::parse(&base).unwrap(),
        WriteScopeGitOperation::BaselineCommit,
    )
    .unwrap();
    verify_commit(
        repo.path(),
        &CommitSha::parse(&candidate).unwrap(),
        WriteScopeGitOperation::CandidateCommit,
    )
    .unwrap();
    assert!(matches!(
        verify(&repo, &base, &candidate, &["**"], &[]),
        Err(WriteScopeVerificationError::GitCommandFailed {
            operation: WriteScopeGitOperation::ChangedPaths,
            ..
        })
    ));
}

#[test]
fn hostile_diff_config_cannot_hide_paths_or_invoke_helpers() {
    let repo = TestRepo::new("scope-hostile-config");
    repo.commit_file(".gitattributes", "*.rs diff=hostile\n");
    let base = repo.commit_file("old.rs", "old content");
    std::fs::rename(repo.path().join("old.rs"), repo.path().join("new.rs")).unwrap();
    let candidate = commit_all(&repo);
    git(repo.path(), &["config", "diff.renames", "true"]);
    git(
        repo.path(),
        &[
            "config",
            "diff.external",
            "/nonexistent-write-scope-external-diff",
        ],
    );
    git(
        repo.path(),
        &[
            "config",
            "diff.hostile.command",
            "/nonexistent-write-scope-diff-driver",
        ],
    );
    git(
        repo.path(),
        &[
            "config",
            "diff.hostile.textconv",
            "/nonexistent-write-scope-textconv",
        ],
    );
    let result = verify(&repo, &base, &candidate, &["*.rs"], &[]).unwrap();
    assert_eq!(result.verification(), WriteScopeVerificationStatus::Pass);
    assert_eq!(result.changed_paths(), strings(&["new.rs", "old.rs"]));
    assert!(
        !crate::test_support::git_raw(
            repo.path(),
            &["diff", "--ext-diff", &base, &candidate, "--"]
        )
        .status
        .success()
    );
}
