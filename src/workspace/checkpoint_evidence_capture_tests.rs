use super::*;
use crate::test_support::{TempDir, TestRepo, git};
use crate::{WorkspaceCheckpointCheckSource, WorkspaceCheckpointRefType};
use std::fs;
use std::io::Cursor;

fn request(directory: &Path) -> WorkspaceCheckpointEvidenceCaptureRequest<'_> {
    WorkspaceCheckpointEvidenceCaptureRequest {
        directory,
        checkpoint_id: "checkpoint".into(),
        workspace_id: "workspace".into(),
        task_id: Some("task".into()),
        attempt_id: Some("attempt".into()),
        kind: WorkspaceCheckpointKind::RecoveryCapture,
        base_sha: None,
        dirty_diff_ref: None,
        executed_checks: Vec::new(),
    }
}

fn capture(directory: &Path) -> WorkspaceCheckpointCaptureCore {
    capture_workspace_checkpoint_evidence(request(directory)).unwrap()
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut names: Vec<_> = names.iter().map(|name| (*name).to_owned()).collect();
    names.sort();
    names
}

fn raw(bytes: &[u8]) -> RawStream {
    drain(
        Cursor::new(bytes),
        RawStream::new(Observation::UntrackedPaths).unwrap(),
        Observation::UntrackedPaths,
        Stream::Stdout,
    )
    .unwrap()
}

fn paths(bytes: &[u8]) -> Result<Vec<String>, E> {
    parse_paths(raw(bytes), Observation::UntrackedPaths)
}

#[test]
fn clean_exact_current_head_base_and_metadata() {
    let repo = TestRepo::new("checkpoint-clean");
    let old_head = repo.head_sha();
    repo.commit_file("tracked", "original");
    let mut input = request(repo.path());
    input.base_sha = Some(&old_head);
    let core = capture_workspace_checkpoint_evidence(input).unwrap();
    assert_eq!(core.head_sha().as_str(), repo.head_sha());
    assert_ne!(core.head_sha().as_str(), old_head);
    assert_eq!(core.base_sha().unwrap().as_str(), old_head);
    assert_eq!(core.checkpoint_id(), "checkpoint");
    assert_eq!(core.workspace_id(), "workspace");
    assert_eq!(core.task_id(), Some("task"));
    assert_eq!(core.attempt_id(), Some("attempt"));
    assert_eq!(core.kind(), WorkspaceCheckpointKind::RecoveryCapture);
    assert_eq!(core.recovery_decision(), None);
    assert_eq!(core.recovery_rationale(), None);
    assert!(core.modified_files().is_empty());
    assert!(core.untracked_files().is_empty());
    // Syntactic validation only: neither existence nor ancestry is required.
    let mut input = request(repo.path());
    input.base_sha = Some("0000000000000000000000000000000000000000");
    assert!(capture_workspace_checkpoint_evidence(input).is_ok());
}

#[test]
fn malformed_base_is_rejected_without_coercion() {
    let repo = TestRepo::new("checkpoint-bad-base");
    for base in [
        "",
        "HEAD",
        "main",
        "1234567",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "gggggggggggggggggggggggggggggggggggggggg",
        "0123456789abcdef0123456789abcdef01234567\n",
    ] {
        let mut input = request(repo.path());
        input.base_sha = Some(base);
        assert!(matches!(
            capture_workspace_checkpoint_evidence(input),
            Err(E::InvalidBaseSha)
        ));
    }
}

#[test]
fn unstaged_staged_added_deleted_and_duplicate_union() {
    let repo = TestRepo::new("checkpoint-dirty-matrix");
    for name in ["z", "a", "deleted"] {
        repo.commit_file(name, "original");
    }
    fs::write(repo.path().join("z"), "unstaged").unwrap();
    assert_eq!(capture(repo.path()).modified_files(), &["z"]);
    fs::write(repo.path().join("a"), "staged").unwrap();
    git(repo.path(), &["add", "a"]);
    assert_eq!(capture(repo.path()).modified_files(), &["a", "z"]);
    fs::write(repo.path().join("a"), "also unstaged").unwrap();
    fs::write(repo.path().join("added"), "new staged file").unwrap();
    git(repo.path(), &["add", "added"]);
    fs::remove_file(repo.path().join("deleted")).unwrap();
    let core = capture(repo.path());
    assert_eq!(core.modified_files(), &["a", "added", "deleted", "z"]);
    assert!(core.untracked_files().is_empty());
    assert_eq!(core, capture(repo.path()));
    git(repo.path(), &["add", "--all"]);
    assert_eq!(capture(repo.path()).modified_files(), core.modified_files());
}

#[test]
fn opposing_staged_and_unstaged_changes_do_not_cancel_evidence() {
    let repo = TestRepo::new("checkpoint-opposing");
    repo.commit_file("file", "original");
    fs::write(repo.path().join("file"), "staged").unwrap();
    git(repo.path(), &["add", "file"]);
    fs::write(repo.path().join("file"), "original").unwrap();
    assert_eq!(capture(repo.path()).modified_files(), &["file"]);
}

#[test]
fn untracked_and_ignored_paths_are_truthful() {
    let repo = TestRepo::new("checkpoint-untracked");
    repo.commit_file(".gitignore", "*.ignored\n");
    fs::write(repo.path().join("z"), "new").unwrap();
    assert_eq!(capture(repo.path()).untracked_files(), &["z"]);
    fs::write(repo.path().join("a"), "new").unwrap();
    fs::write(repo.path().join("secret.ignored"), "ignored").unwrap();
    fs::write(repo.path().join(".gitignore"), "*.ignored\n# dirty\n").unwrap();
    let core = capture(repo.path());
    assert_eq!(core.untracked_files(), &["a", "z"]);
    assert_eq!(core.modified_files(), &[".gitignore"]);
}

#[test]
fn subdirectory_and_hostile_relative_config_keep_all_root_identities() {
    let repo = TestRepo::new_nested("checkpoint-root", "repo\n with space");
    for path in ["outside.txt", "subdir/inside.txt"] {
        repo.commit_file(path, "original");
    }
    git(repo.path(), &["config", "diff.relative", "true"]);
    for path in ["outside.txt", "subdir/inside.txt"] {
        fs::write(repo.path().join(path), "staged").unwrap();
    }
    git(repo.path(), &["add", "--all"]);
    for path in ["outside.txt", "subdir/inside.txt"] {
        fs::write(repo.path().join(path), "unstaged").unwrap();
    }
    for path in ["new-outside", "subdir/new-inside"] {
        fs::write(repo.path().join(path), "new").unwrap();
    }
    let core = capture(&repo.path().join("subdir"));
    assert_eq!(core.modified_files(), &["outside.txt", "subdir/inside.txt"]);
    assert_eq!(
        core.untracked_files(),
        &["new-outside", "subdir/new-inside"]
    );
    assert_eq!(core, capture(repo.path()));
}

#[test]
fn raw_special_names_survive_both_path_classes() {
    let repo = TestRepo::new("checkpoint-special-names");
    git(repo.path(), &["config", "core.precomposeunicode", "false"]);
    let names = [
        "space name",
        "tab\tname",
        "line\nname",
        "猫🦀",
        "?[]{}!*",
        "back\\slash",
    ];
    for name in names {
        fs::write(repo.path().join(name), "new").unwrap();
    }
    assert_eq!(capture(repo.path()).untracked_files(), sorted(&names));
    git(repo.path(), &["add", "--all"]);
    assert_eq!(capture(repo.path()).modified_files(), sorted(&names));
    git(repo.path(), &["commit", "--quiet", "-m", "names"]);
    for name in names {
        fs::write(repo.path().join(name), "changed").unwrap();
    }
    assert_eq!(capture(repo.path()).modified_files(), sorted(&names));
}

#[cfg(unix)]
#[test]
fn real_git_invalid_utf8_index_path_fails_closed() {
    use std::os::unix::ffi::OsStringExt;
    let repo = TestRepo::new("checkpoint-invalid-utf8");
    repo.commit_file("blob-input", "content");
    let blob = String::from_utf8(git(repo.path(), &["hash-object", "blob-input"]).stdout).unwrap();
    // The index admits raw names even on macOS filesystems that reject them.
    let name = std::ffi::OsString::from_vec(vec![b'x', 0xff]);
    let output = crate::git::prepared_command(
        repo.path(),
        "fixture raw name",
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
    assert!(output.status.success());
    assert!(matches!(
        capture_workspace_checkpoint_evidence(request(repo.path())),
        Err(E::InvalidUtf8 {
            observation: Observation::TrackedStatus
        })
    ));
}

#[test]
fn preparation_and_git_failures_are_typed() {
    let temp = TempDir::new("checkpoint-no-repo");
    assert!(matches!(
        capture_workspace_checkpoint_evidence(request(&temp.path().join("missing"))),
        Err(E::GitPreparation { .. })
    ));
    assert!(matches!(
        capture_workspace_checkpoint_evidence(request(temp.path())),
        Err(E::GitCommandFailed {
            observation: Observation::WorktreeRoot,
            ..
        })
    ));
    git(temp.path(), &["init", "--quiet"]);
    assert!(matches!(
        capture_workspace_checkpoint_evidence(request(temp.path())),
        Err(E::GitCommandFailed {
            observation: Observation::Head,
            ..
        })
    ));
}

#[test]
fn head_success_followed_by_status_failure_returns_no_partial_core() {
    let repo = TestRepo::new("checkpoint-broken-tree");
    repo.commit_file("file", "original");
    let tree = String::from_utf8(git(repo.path(), &["rev-parse", "HEAD^{tree}"]).stdout).unwrap();
    let tree = tree.trim();
    fs::remove_file(
        repo.path()
            .join(".git/objects")
            .join(&tree[..2])
            .join(&tree[2..]),
    )
    .unwrap();
    assert_eq!(
        parse_head(
            observe(
                repo.path(),
                Observation::Head,
                &["rev-parse", "--verify", "HEAD^{commit}"]
            )
            .unwrap()
        )
        .unwrap()
        .as_str(),
        repo.head_sha()
    );
    assert!(matches!(
        capture_workspace_checkpoint_evidence(request(repo.path())),
        Err(E::GitCommandFailed {
            observation: Observation::TrackedStatus,
            ..
        })
    ));
}

#[test]
fn inert_checks_and_references_are_preserved_without_execution_or_network() {
    let repo = TestRepo::new("checkpoint-inert");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/never-resolve", listener.local_addr().unwrap());
    git(repo.path(), &["config", "remote.origin.url", &url]);
    let old_sha = CommitSha::parse("0123456789abcdef0123456789abcdef01234567").unwrap();
    let marker = repo.path().join("must-not-exist");
    for (ref_type, target) in [
        (WorkspaceCheckpointRefType::Url, url.as_str()),
        (
            WorkspaceCheckpointRefType::RepoPath,
            "nonexistent-evidence-file",
        ),
        (
            WorkspaceCheckpointRefType::ArtifactId,
            "nonexistent-artifact",
        ),
        (WorkspaceCheckpointRefType::StateQuery, "nonexistent-query"),
    ] {
        let reference = WorkspaceCheckpointRef::new(
            ref_type,
            target,
            Some(" supplied digest ".into()),
            Some(" section ".into()),
        )
        .unwrap();
        let mut checks = Vec::new();
        for source in WorkspaceCheckpointCheckSource::ALL {
            checks.push(
                WorkspaceCheckpointExecutedCheckCore::new(
                    source,
                    vec![
                        "/usr/bin/touch".into(),
                        marker.to_str().unwrap().into(),
                        "".into(),
                        "x;y\t$HOME\n猫".into(),
                    ],
                    -7,
                    old_sha.clone(),
                    Some(true),
                    Some(reference.clone()),
                )
                .unwrap(),
            );
        }
        let mut input = request(repo.path());
        input.dirty_diff_ref = Some(reference.clone());
        input.executed_checks = checks.clone();
        let core = capture_workspace_checkpoint_evidence(input).unwrap();
        assert_eq!(core.executed_checks(), checks);
        assert_eq!(core.dirty_diff_ref(), Some(&reference));
        assert_ne!(core.head_sha(), &old_sha);
        assert_eq!(core.recovery_decision(), None);
        assert!(!marker.exists());
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.push((
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort();
    files
}

#[test]
fn no_mutation_and_capture_survives_later_fixture_cleanup() {
    let repo = TestRepo::new("checkpoint-pre-cleanup");
    repo.commit_file("tracked", "original");
    fs::write(repo.path().join("tracked"), "dirty").unwrap();
    fs::write(repo.path().join("untracked"), "untracked").unwrap();
    let before = snapshot(repo.path());
    let core = capture(repo.path());
    assert_eq!(
        snapshot(repo.path()),
        before,
        "includes HEAD, index, config and worktree bytes"
    );
    // Only test code changes this throwaway fixture, strictly after capture.
    fs::write(repo.path().join("tracked"), "original").unwrap();
    fs::remove_file(repo.path().join("untracked")).unwrap();
    assert!(capture(repo.path()).modified_files().is_empty());
    assert_eq!(core.modified_files(), &["tracked"]);
    assert_eq!(core.untracked_files(), &["untracked"]);
}

#[test]
fn linked_worktree_head_index_and_untracked_state_do_not_leak() {
    let repo = TestRepo::new_nested("checkpoint-worktrees", "main");
    repo.commit_file("tracked", "original");
    let a = repo.root.path().join("a");
    let b = repo.root.path().join("b");
    git(
        repo.path(),
        &["worktree", "add", "--quiet", "-b", "a", a.to_str().unwrap()],
    );
    git(
        repo.path(),
        &["worktree", "add", "--quiet", "-b", "b", b.to_str().unwrap()],
    );
    git(&b, &["commit", "--quiet", "--allow-empty", "-m", "B head"]);
    for (root, name) in [(&a, "a-only"), (&b, "b-only")] {
        fs::write(root.join(name), "new").unwrap();
        fs::write(root.join("tracked"), name).unwrap();
    }
    git(&b, &["add", "b-only"]);
    let before = snapshot(repo.root.path());
    let core_a = capture(&a);
    let core_b = capture(&b);
    assert_eq!(core_a.head_sha().as_str(), repo.head_sha());
    assert_ne!(core_a.head_sha(), core_b.head_sha());
    assert_eq!(core_a.modified_files(), &["tracked"]);
    assert_eq!(core_a.untracked_files(), &["a-only"]);
    assert_eq!(core_b.modified_files(), &["b-only", "tracked"]);
    assert!(core_b.untracked_files().is_empty());
    assert_eq!(snapshot(repo.root.path()), before);
}

#[test]
fn hostile_helpers_and_rename_config_cannot_hide_paths() {
    let repo = TestRepo::new("checkpoint-hostile-helpers");
    repo.commit_file(".gitattributes", "*.rs diff=hostile\n");
    repo.commit_file("old.rs", "content");
    fs::rename(repo.path().join("old.rs"), repo.path().join("new.rs")).unwrap();
    git(repo.path(), &["add", "--all"]);
    for key in [
        "diff.external",
        "diff.hostile.command",
        "diff.hostile.textconv",
        "core.fsmonitor",
    ] {
        git(
            repo.path(),
            &["config", key, "/nonexistent-checkpoint-helper"],
        );
    }
    git(repo.path(), &["config", "diff.renames", "true"]);
    let core = capture(repo.path());
    assert_eq!(core.modified_files(), &["new.rs", "old.rs"]);
}

#[test]
fn invalid_core_metadata_remains_typed() {
    let repo = TestRepo::new("checkpoint-core-error");
    let mut input = request(repo.path());
    input.checkpoint_id.clear();
    assert!(matches!(
        capture_workspace_checkpoint_evidence(input),
        Err(E::Core(
            WorkspaceCheckpointCaptureCoreError::EmptyCheckpointId
        ))
    ));
}

#[test]
fn bound_zero_below_and_exact_limit_are_complete_valid_nul_data() {
    for length in [
        0,
        2,
        STREAM_CAPTURE_LIMIT_BYTES as usize - 2,
        STREAM_CAPTURE_LIMIT_BYTES as usize,
    ] {
        let bytes = b"p\0".repeat(length / 2);
        assert_eq!(bytes.len(), length);
        let retained = raw(&bytes);
        assert_eq!(retained.total_bytes, length as u64);
        assert!(!retained.truncated);
        assert_eq!(retained.bytes, bytes);
        let parsed = parse_paths(retained, Observation::UntrackedPaths).unwrap();
        assert_eq!(parsed.len(), length / 2);
        assert!(parsed.iter().all(|path| path == "p"));
    }
}

fn assert_oversize(result: Result<Vec<String>, E>, observation: Observation, total: u64) {
    assert!(
        matches!(result, Err(E::GitEvidenceTooLarge { observation: actual, total_bytes, limit_bytes })
        if actual == observation && total_bytes == total && limit_bytes == STREAM_CAPTURE_LIMIT_BYTES)
    );
}

#[test]
fn bound_one_over_and_much_larger_are_deterministic_typed_failures() {
    for length in [
        STREAM_CAPTURE_LIMIT_BYTES + 1,
        STREAM_CAPTURE_LIMIT_BYTES * 8,
    ] {
        let retained = drain(
            io::repeat(b'x').take(length),
            RawStream::new(Observation::UntrackedPaths).unwrap(),
            Observation::UntrackedPaths,
            Stream::Stdout,
        )
        .unwrap();
        assert_eq!(retained.bytes.len() as u64, STREAM_CAPTURE_LIMIT_BYTES);
        assert_eq!(retained.total_bytes, length);
        assert!(retained.truncated);
        assert_oversize(
            parse_paths(retained, Observation::UntrackedPaths),
            Observation::UntrackedPaths,
            length,
        );
    }
}

#[test]
fn bound_precedes_decode_framing_filtering_and_deduplication() {
    for record in [b"p\0".as_slice(), b"\0\0", b"\xff\0"] {
        let bytes = record.repeat(STREAM_CAPTURE_LIMIT_BYTES as usize / 2 + 1);
        assert_oversize(
            paths(&bytes),
            Observation::UntrackedPaths,
            bytes.len() as u64,
        );
    }
}

#[test]
fn bound_truncated_head_tail_or_partial_retention_never_parses() {
    for (total_bytes, truncated) in [
        (4, true),
        (STREAM_CAPTURE_LIMIT_BYTES + 1, false),
        (STREAM_CAPTURE_LIMIT_BYTES + 1, true),
    ] {
        // Inject a plausible retained head+tail that itself is valid NUL data.
        let retained = RawStream {
            bytes: b"a\0z\0".to_vec(),
            total_bytes,
            truncated,
        };
        assert_oversize(
            parse_paths(retained, Observation::StagedPaths),
            Observation::StagedPaths,
            total_bytes,
        );
    }
    assert!(matches!(
        parse_paths(
            RawStream {
                bytes: b"a\0".to_vec(),
                total_bytes: 4,
                truncated: false
            },
            Observation::StagedPaths
        ),
        Err(E::MalformedEvidence { .. })
    ));
}

#[test]
fn bound_valid_nul_paths_are_exact_and_bad_framing_or_utf8_fails() {
    assert_eq!(
        paths("z\0space name\0tab\tname\0line\nname\0猫\0".as_bytes()).unwrap(),
        ["z", "space name", "tab\tname", "line\nname", "猫"]
    );
    for bytes in [b"a".as_slice(), b"a\0tail", b"\0", b"a\0\0"] {
        assert!(matches!(paths(bytes), Err(E::MalformedEvidence { .. })));
    }
    assert!(matches!(
        paths(b"valid\0\xff\0"),
        Err(E::InvalidUtf8 { .. })
    ));
}

#[test]
fn bound_checked_counter_overflow_fails_without_wrapping() {
    for stream in [Stream::Stdout, Stream::Stderr] {
        let mut retained = RawStream::new(Observation::UntrackedPaths).unwrap();
        retained.total_bytes = u64::MAX;
        assert!(
            matches!(retained.push(b"x", Observation::UntrackedPaths, stream),
            Err(E::StreamCountOverflow { counted: u64::MAX, chunk: 1, stream: actual, .. }) if actual == stream)
        );
        assert_eq!(retained.total_bytes, u64::MAX);
        assert!(retained.bytes.is_empty());
    }
}

#[test]
fn bound_read_failure_is_typed_and_interrupted_reads_retry() {
    struct Reader {
        calls: usize,
    }
    impl Read for Reader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.calls += 1;
            match self.calls {
                1 => Err(io::ErrorKind::Interrupted.into()),
                2 => {
                    buffer[0] = b'x';
                    Ok(1)
                }
                _ => Err(io::Error::other("injected read failure")),
            }
        }
    }
    assert!(matches!(
        drain(
            Reader { calls: 0 },
            RawStream::new(Observation::Head).unwrap(),
            Observation::Head,
            Stream::Stdout
        ),
        Err(E::StreamRead {
            observation: Observation::Head,
            stream: Stream::Stdout,
            ..
        })
    ));
}

#[test]
fn malformed_observed_head_and_root_are_not_normalized() {
    for sha in [
        "HEAD\n",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
        "0123456789abcdef0123456789abcdef01234567",
        "0123456789abcdef0123456789abcdef01234567\n\n",
    ] {
        assert!(matches!(
            parse_head(raw(sha.as_bytes())),
            Err(E::InvalidHeadSha)
        ));
    }
    for root in [
        b"relative\n".as_slice(),
        b"/missing-terminator",
        b"/bad\0root\n",
    ] {
        assert!(matches!(
            parse_root(raw(root)),
            Err(E::MalformedEvidence { .. })
        ));
    }
    assert!(matches!(
        parse_root(raw(b"/bad\xff\n")),
        Err(E::InvalidUtf8 { .. })
    ));
}

#[test]
fn bound_real_git_untracked_staged_and_unstaged_each_fail_closed() {
    let repo = TestRepo::new("checkpoint-real-overflow");
    // Each actual filename is 240 bytes, plus its NUL. A fixed query must fail;
    // splitting this set into pages would wrongly make this test succeed.
    let count = STREAM_CAPTURE_LIMIT_BYTES as usize / 241 + 1;
    let names: Vec<_> = (0..count)
        .map(|i| format!("{i:08}{}", "x".repeat(232)))
        .collect();
    for name in &names {
        fs::write(repo.path().join(name), "content").unwrap();
    }
    for observation in [
        Observation::UntrackedPaths,
        Observation::StagedPaths,
        Observation::UnstagedPaths,
    ] {
        let result = capture_workspace_checkpoint_evidence(request(repo.path()));
        // Tracked status has three extra raw bytes (XY SP) per path. The
        // complete raw bound still applies before stripping those prefixes.
        let (expected, total) = if observation == Observation::UntrackedPaths {
            (Observation::UntrackedPaths, count * 241)
        } else {
            (Observation::TrackedStatus, count * 244)
        };
        assert!(
            matches!(result, Err(E::GitEvidenceTooLarge { observation: actual, total_bytes, limit_bytes })
            if actual == expected && total_bytes == total as u64 && limit_bytes == STREAM_CAPTURE_LIMIT_BYTES)
        );
        match observation {
            Observation::UntrackedPaths => {
                git(repo.path(), &["add", "--all"]);
            }
            Observation::StagedPaths => {
                git(
                    repo.path(),
                    &["commit", "--quiet", "-m", "overflow fixture"],
                );
                for name in &names {
                    fs::remove_file(repo.path().join(name)).unwrap();
                }
            }
            _ => {}
        }
    }
}

#[test]
fn bound_real_git_large_stderr_is_separate_and_does_not_deadlock() {
    let repo = TestRepo::new("checkpoint-stderr-bound");
    // Git's unknown-command diagnostic is sourced from an on-disk alias key,
    // avoiding OS argv limits. Invalid config itself generates long stderr.
    let invalid = "x".repeat(STREAM_CAPTURE_LIMIT_BYTES as usize * 2);
    fs::write(
        repo.path().join(".git/config"),
        format!("[invalid{invalid}\n"),
    )
    .unwrap();
    let error = capture_workspace_checkpoint_evidence(request(repo.path())).unwrap_err();
    match error {
        E::GitCommandFailed {
            stderr,
            stderr_total_bytes,
            stderr_truncated,
            ..
        } => {
            assert!(stderr.len() as u64 <= STREAM_CAPTURE_LIMIT_BYTES);
            assert!(stderr_total_bytes > 0);
            assert_eq!(
                stderr_truncated,
                stderr_total_bytes > STREAM_CAPTURE_LIMIT_BYTES
            );
        }
        other => panic!("unexpected: {other:?}"),
    }
    // Independently exercise draining both stream labels past the exact limit;
    // Git versions differ in how much invalid config they print.
    for stream in [Stream::Stdout, Stream::Stderr] {
        let retained = drain(
            io::repeat(0xff).take(STREAM_CAPTURE_LIMIT_BYTES * 2),
            RawStream::new(Observation::Head).unwrap(),
            Observation::Head,
            stream,
        )
        .unwrap();
        assert_eq!(retained.bytes.len() as u64, STREAM_CAPTURE_LIMIT_BYTES);
        assert_eq!(retained.total_bytes, STREAM_CAPTURE_LIMIT_BYTES * 2);
        assert!(retained.truncated);
    }
}

#[cfg(unix)]
fn index_snapshot(path: &Path) -> (Vec<u8>, [u64; 3], [i64; 4]) {
    use std::os::unix::fs::MetadataExt;
    let bytes = fs::read(path).unwrap();
    let metadata = fs::metadata(path).unwrap();
    (
        bytes,
        [metadata.dev(), metadata.ino(), metadata.len()],
        [
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        ],
    )
}

#[cfg(unix)]
#[test]
fn accurate_status_preserves_index_bytes_and_metadata_under_hostile_config() {
    use crate::WorkspaceProvisionRequest;
    use std::time::{Duration, UNIX_EPOCH};
    for linked in [false, true] {
        for refresh in ["true", "false"] {
            let repo = TestRepo::new_nested("checkpoint-index-immutability", "repo");
            repo.commit_file("tracked", "original");
            let handle = linked.then(|| {
                WorkspaceProvisionRequest::new(
                    repo.path(),
                    "workspace",
                    None,
                    "task-branch",
                    fs::canonicalize(repo.root.path()).unwrap().join("linked"),
                    &repo.head_sha(),
                )
                .unwrap()
                .provision()
                .unwrap()
            });
            let root = handle.as_ref().map_or(repo.path(), |h| h.worktree_path());
            for (key, value) in [
                ("diff.autoRefreshIndex", refresh),
                ("diff.relative", "true"),
                ("diff.renames", "true"),
                ("status.relativePaths", "true"),
                ("status.renames", "copies"),
                ("status.showUntrackedFiles", "all"),
                ("status.showStash", "true"),
                ("color.status", "always"),
            ] {
                git(root, &["config", key, value]);
            }
            // Resolve the correct main/linked index ONCE, in fixture code.
            let index =
                String::from_utf8(git(root, &["rev-parse", "--git-path", "index"]).stdout).unwrap();
            let index = root.join(index.strip_suffix('\n').unwrap());
            let assert_capture = |modified: &[&str], untracked: &[&str]| {
                let before = index_snapshot(&index);
                let result = capture(root);
                let after = index_snapshot(&index);
                assert!(
                    before.0 == after.0,
                    "index bytes changed: linked={linked}, refresh={refresh}"
                );
                assert_eq!(before.1, after.1, "index device/inode/size changed");
                assert_eq!(before.2, after.2, "index mtime/ctime changed");
                assert_eq!(result.modified_files(), modified);
                assert_eq!(result.untracked_files(), untracked);
            };
            // An old mtime deterministically differs from cached stat data;
            // no sleep or timestamp-resolution assumption is needed.
            fs::File::options()
                .write(true)
                .open(root.join("tracked"))
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1)))
                .unwrap();
            assert_capture(&[], &[]);
            fs::write(root.join("tracked"), "unstaged").unwrap();
            assert_capture(&["tracked"], &[]);
            git(root, &["add", "tracked"]);
            assert_capture(&["tracked"], &[]);
            fs::write(root.join("tracked"), "after staging").unwrap();
            assert_capture(&["tracked"], &[]);
            fs::write(root.join("loose"), "untracked").unwrap();
            assert_capture(&["tracked"], &["loose"]);
        }
    }
}

#[test]
fn tracked_status_parser_preserves_raw_names_and_rejects_unexpected_formats() {
    assert_eq!(
        parse_tracked_status(raw(b" M space \0A  tab\tline\n\0UU conflict\0")).unwrap(),
        ["space ", "tab\tline\n", "conflict"]
    );
    for status in [
        " M", " T", " A", " D", "M ", "MM", "MT", "MD", "T ", "A ", "AD", "D ", "DD", "AU", "UD",
        "UA", "DU", "AA", "UU",
    ] {
        assert_eq!(
            parse_tracked_status(raw(format!("{status} name\0").as_bytes())).unwrap(),
            ["name"]
        );
    }
    for bytes in [
        b" M \0".as_slice(),
        b"M path\0",
        b" Mxname\0",
        b"   name\0",
        b"?? new\0",
        b"!! ignored\0",
        b"R  to\0from\0",
        b"C  copy\0from\0",
        b"## branch\0",
        b"ZZ name\0",
        b" M name",
    ] {
        assert!(matches!(
            parse_tracked_status(raw(bytes)),
            Err(E::MalformedEvidence { .. })
        ));
    }
    assert!(matches!(
        parse_tracked_status(raw(b" M \xff\0")),
        Err(E::InvalidUtf8 { .. })
    ));
}

#[test]
fn tracked_status_bound_precedes_prefix_removal_and_deduplication() {
    let exact = b" M p\0".repeat(STREAM_CAPTURE_LIMIT_BYTES as usize / 5);
    let mut exact = exact;
    // Add the remainder to the final path, preserving complete framing.
    let remainder = STREAM_CAPTURE_LIMIT_BYTES as usize - exact.len();
    exact.splice(exact.len() - 1..exact.len() - 1, vec![b'x'; remainder]);
    assert_eq!(exact.len() as u64, STREAM_CAPTURE_LIMIT_BYTES);
    assert!(parse_tracked_status(raw(&exact)).is_ok());
    exact.insert(exact.len() - 1, b'x');
    assert!(matches!(
        parse_tracked_status(raw(&exact)),
        Err(E::GitEvidenceTooLarge {
            observation: Observation::TrackedStatus,
            ..
        })
    ));
    // Even though deduplication or stripping XY would make it fit, the raw
    // stream is oversized and cannot be admitted.
    let duplicate = b" M p\0".repeat(STREAM_CAPTURE_LIMIT_BYTES as usize / 5 + 1);
    assert!(matches!(
        parse_tracked_status(raw(&duplicate)),
        Err(E::GitEvidenceTooLarge { .. })
    ));
    assert!(matches!(
        parse_tracked_status(RawStream {
            bytes: b" M p\0".to_vec(),
            total_bytes: 6,
            truncated: false
        }),
        Err(E::MalformedEvidence { .. })
    ));
}

#[test]
fn intent_to_add_and_unmerged_paths_remain_truthful() {
    let repo = TestRepo::new("checkpoint-status-unmerged");
    repo.commit_file("conflict", "base\n");
    fs::write(repo.path().join("intent"), "intent").unwrap();
    git(repo.path(), &["add", "-N", "intent"]);
    assert_eq!(capture(repo.path()).modified_files(), &["intent"]);
    git(repo.path(), &["add", "intent"]);
    git(repo.path(), &["commit", "--quiet", "-m", "intent fixture"]);
    git(repo.path(), &["checkout", "--quiet", "-b", "other"]);
    repo.commit_file("conflict", "other\n");
    git(repo.path(), &["checkout", "--quiet", "main"]);
    repo.commit_file("conflict", "main\n");
    assert!(
        !crate::test_support::git_raw(repo.path(), &["merge", "--no-edit", "other"])
            .status
            .success()
    );
    let before = index_snapshot(&repo.path().join(".git/index"));
    assert_eq!(capture(repo.path()).modified_files(), &["conflict"]);
    assert_eq!(index_snapshot(&repo.path().join(".git/index")), before);
}

#[test]
fn configured_filters_fail_closed_without_executing_them() {
    let repo = TestRepo::new("checkpoint-filter-guard");
    repo.commit_file("tracked", "original");
    repo.commit_file(".gitattributes", "tracked filter=untrusted\n");
    fs::write(repo.path().join("tracked"), "changed").unwrap();
    for key in ["filter.untrusted.clean", "filter.untrusted.process"] {
        git(
            repo.path(),
            &["config", key, "/nonexistent-checkpoint-filter"],
        );
        let before = snapshot(repo.path());
        assert!(matches!(
            capture_workspace_checkpoint_evidence(request(repo.path())),
            Err(E::UnsupportedConfiguredFilters)
        ));
        assert_eq!(snapshot(repo.path()), before);
    }
}
