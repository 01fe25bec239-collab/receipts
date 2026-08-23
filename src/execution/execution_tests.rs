//! Tests for the argv-only process-runner foundation.
//!
//! Every behavioral proof runs a real child through the public runner:
//! either an absolute OS utility or this very test binary acting as a
//! controlled child helper via libtest's filtered ignored-test mechanism.
//! No shell is ever involved, no output is captured anywhere, and no
//! process-global state is mutated by any test.

use std::fs;
use std::path::{Path, PathBuf};

use super::runner::{prepared_command, spawn_failed, validated_executable, wait_failed};
use crate::execution::{ExecutionError, ProcessRunOutcome, ProcessRunRequest, run};

/// A temporary directory removed on drop.
struct TempDir {
    root: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "receipts-execution-test-{tag}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temporary test directory creation");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn first_existing(candidates: &[&str]) -> PathBuf {
    candidates
        .iter()
        .map(Path::new)
        .find(|candidate| candidate.is_file())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| panic!("expected one of {candidates:?} to exist on this platform"))
}

fn each_existing(candidates: &[&str]) -> Vec<PathBuf> {
    candidates
        .iter()
        .map(Path::new)
        .filter(|candidate| candidate.is_file())
        .map(Path::to_path_buf)
        .collect()
}

fn request(
    executable: impl Into<PathBuf>,
    arguments: &[&str],
    workspace_root: &Path,
    cwd: &Path,
) -> ProcessRunRequest {
    ProcessRunRequest::new(executable, arguments.iter().copied(), workspace_root, cwd)
        .expect("structural request construction")
}

fn run_request(built: &ProcessRunRequest) -> ProcessRunOutcome {
    run(built).expect("runner invocation should succeed")
}

fn assert_cwd_outside(error: &ExecutionError) {
    assert!(
        matches!(error, ExecutionError::CwdOutsideWorkspace { .. }),
        "expected CwdOutsideWorkspace, got: {error:?}"
    );
}

// --- Controlled child helpers ------------------------------------------
//
// The following three tests are never executed by the parent suite; the
// runner re-invokes this same test binary with a name filter and
// `--ignored` so exactly one probe runs as the child. Each probe reports
// its verdict through its own exit code, which the parent observes as
// typed outcome metadata — no output capture of any kind is needed.

#[test]
#[ignore]
fn execution_env_probe_child_exits_zero_only_when_environment_is_empty() {
    if std::env::vars_os().next().is_some() {
        std::process::exit(7);
    }
}

#[test]
#[ignore]
fn execution_stdin_probe_child_exits_zero_on_immediate_eof() {
    use std::io::Read as _;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut byte = [0u8; 1];
        // Test-side guard only: bounds how long the probe waits for EOF so
        // the suite cannot hang if stdin were ever interactive. The runner
        // itself has no timeout machinery.
        let _ = sender.send(std::io::stdin().read(&mut byte));
    });
    match receiver.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(Ok(0)) => {}
        _ => std::process::exit(11),
    }
}

#[test]
#[ignore]
fn execution_exit_probe_child_exits_seven() {
    std::process::exit(7);
}

fn run_probe(probe_name: &str, workspace_root: &Path) -> ProcessRunOutcome {
    let executable = std::env::current_exe().expect("current test executable path");
    run_request(&request(
        executable,
        &[probe_name, "--ignored"],
        workspace_root,
        workspace_root,
    ))
}

// --- T1 -----------------------------------------------------------------

#[test]
fn ex01_absolute_executable_success_reports_success_metadata() {
    let workspace = TempDir::new("ex01");
    let outcome = run_request(&request(
        first_existing(&["/usr/bin/true", "/bin/true"]),
        &[],
        workspace.path(),
        workspace.path(),
    ));
    assert!(outcome.success());
    assert_eq!(outcome.exit_code(), Some(0));
}

// --- T2 -----------------------------------------------------------------

#[test]
fn ex02_nonzero_child_exit_is_typed_outcome_not_runner_failure() {
    let workspace = TempDir::new("ex02");

    let false_exit = run_request(&request(
        first_existing(&["/usr/bin/false", "/bin/false"]),
        &[],
        workspace.path(),
        workspace.path(),
    ));
    assert!(!false_exit.success());
    assert_eq!(false_exit.exit_code(), Some(1));

    let seven_exit = run_probe("execution_exit_probe_child_exits_seven", workspace.path());
    assert!(!seven_exit.success());
    assert_eq!(seven_exit.exit_code(), Some(7));
}

// --- T3 -----------------------------------------------------------------

#[test]
fn ex03_relative_executable_rejected_before_spawn() {
    let error = ProcessRunRequest::new("something", std::iter::empty::<&str>(), "/tmp", "/tmp")
        .expect_err("relative executable must be rejected");
    assert!(matches!(
        error,
        ExecutionError::ExecutablePathNotAbsolute { .. }
    ));
}

// --- T4 -----------------------------------------------------------------

#[test]
fn ex04_nonexistent_absolute_executable_fails_closed() {
    let workspace = TempDir::new("ex04");
    let missing = workspace.path().join("does-not-exist");
    let error = run(&request(missing, &[], workspace.path(), workspace.path()))
        .expect_err("nonexistent executable must be rejected");
    assert!(matches!(error, ExecutionError::ExecutableNotFound { .. }));
}

// --- T5 -----------------------------------------------------------------

#[test]
fn ex05_non_executable_regular_file_rejected_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let workspace = TempDir::new("ex05");
    let plain = workspace.path().join("plain-file");
    fs::write(&plain, b"data").expect("write plain regular file");
    fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).expect("clear executable bits");

    assert_eq!(
        fs::metadata(&plain).unwrap().permissions().mode() & 0o111,
        0
    );
    let error = run(&request(&plain, &[], workspace.path(), workspace.path()))
        .expect_err("regular file without executable bits must be rejected");
    assert!(matches!(
        error,
        ExecutionError::ExecutableNotExecutable { .. }
    ));
}

// --- T6 -----------------------------------------------------------------

#[test]
fn ex06_nested_directory_inside_workspace_accepted() {
    let workspace = TempDir::new("ex06");
    let nested = workspace.path().join("deep").join("deeper");
    fs::create_dir_all(&nested).expect("nested directory creation");

    let outcome = run_request(&request(
        first_existing(&["/usr/bin/true", "/bin/true"]),
        &[],
        workspace.path(),
        &nested,
    ));
    assert!(outcome.success());
}

// --- T7 -----------------------------------------------------------------

#[test]
fn ex07_sibling_workspace_rejected() {
    let inside = TempDir::new("ex07-inside");
    let outside = TempDir::new("ex07-outside");
    let error = run(&request(
        first_existing(&["/usr/bin/true", "/bin/true"]),
        &[],
        inside.path(),
        outside.path(),
    ))
    .expect_err("cwd outside the workspace must be rejected");
    assert_cwd_outside(&error);
}

// --- T8 -----------------------------------------------------------------

#[test]
fn ex08_lexical_prefix_confusion_rejected() {
    let base = TempDir::new("ex08");
    let workspace = base.path().join("ws");
    let lookalike = base.path().join("ws-more");
    fs::create_dir_all(&workspace).expect("workspace directory creation");
    fs::create_dir_all(&lookalike).expect("lookalike directory creation");

    // "ws-more" shares a textual prefix with "ws" but is not contained in
    // it; component-wise comparison must reject it.
    let error = run(&request(
        first_existing(&["/usr/bin/true", "/bin/true"]),
        &[],
        &workspace,
        &lookalike,
    ))
    .expect_err("textual-prefix sibling must not count as containment");
    assert_cwd_outside(&error);
}

#[test]
fn ex09_file_inside_workspace_rejected_as_working_directory() {
    let workspace = TempDir::new("ex09");
    let file = workspace.path().join("plain.txt");
    fs::write(&file, b"not a directory").expect("write file");

    let error = run(&request(
        first_existing(&["/usr/bin/true", "/bin/true"]),
        &[],
        workspace.path(),
        &file,
    ))
    .expect_err("a regular file cannot serve as working directory");
    assert!(matches!(error, ExecutionError::CwdNotADirectory { .. }));
}

// --- T9 -----------------------------------------------------------------

#[test]
fn ex10_symlink_escape_from_workspace_rejected() {
    let base = TempDir::new("ex10");
    let workspace = base.path().join("ws");
    let outside = base.path().join("outside");
    fs::create_dir_all(&workspace).expect("workspace directory creation");
    fs::create_dir_all(&outside).expect("outside directory creation");

    let link = workspace.join("inside-link");
    std::os::unix::fs::symlink(&outside, &link).expect("test symlink creation");

    let error = run(&request(
        first_existing(&["/usr/bin/true", "/bin/true"]),
        &[],
        &workspace,
        &link,
    ))
    .expect_err("symlink resolving outside the workspace must be rejected");
    assert_cwd_outside(&error);
}

// --- T10 ----------------------------------------------------------------

#[test]
fn ex11_dotdot_escape_rejected_after_realpath_but_inside_normalization_accepted() {
    use std::os::unix::fs::symlink;
    let base = TempDir::new("ex11");
    let workspace = base.path().join("ws");
    fs::create_dir_all(workspace.join("nested")).expect("nested directory creation");
    fs::create_dir_all(base.path().join("beyond")).expect("beyond directory creation");

    // Resolves through realpath to <base>/beyond — outside the workspace.
    let escaping = workspace
        .join("nested")
        .join("..")
        .join("..")
        .join("beyond");
    let error = run(&request(
        first_existing(&["/usr/bin/true", "/bin/true"]),
        &[],
        &workspace,
        &escaping,
    ))
    .expect_err(".. chain resolving past the workspace root must be rejected");
    assert_cwd_outside(&error);

    // A `..` hop that stays inside after normalization is fine.
    fs::create_dir_all(workspace.join("kept")).expect("kept directory creation");
    symlink(
        workspace.join("kept"),
        workspace.join("nested").join("alias"),
    )
    .expect("inside alias symlink");
    let staying = workspace.join("nested").join("..").join("kept");
    let outcome = run_request(&request(
        first_existing(&["/usr/bin/true", "/bin/true"]),
        &[],
        &workspace,
        &staying,
    ));
    assert!(outcome.success());
}

// --- T11 ----------------------------------------------------------------

#[test]
fn ex12_argv_value_with_spaces_and_metacharacters_arrives_as_one_element() {
    let workspace = TempDir::new("ex12");
    let utility = first_existing(&["/usr/bin/test", "/bin/test"]);
    // Shells would split on spaces and expand $HOME, *, $( ), backticks,
    // quotes, and pipes. The runner must deliver the value verbatim as one
    // argv element, so `test VALUE = VALUE` compares equal literals.
    let magic = "hello world;$HOME*|&><'\"\\ \t$(pwd)`pwd`";
    let outcome = run_request(&request(
        &utility,
        &[magic, "=", magic],
        workspace.path(),
        workspace.path(),
    ));
    assert!(outcome.success());
    assert_eq!(outcome.exit_code(), Some(0));

    // Negative control: the comparison genuinely discriminates, so a
    // mangled or expanded argument could not have passed above.
    let mismatch = run_request(&request(
        utility,
        &[magic, "=", "deliberately-different"],
        workspace.path(),
        workspace.path(),
    ));
    assert!(!mismatch.success());
    assert_eq!(mismatch.exit_code(), Some(1));
}

// --- T12 ----------------------------------------------------------------

#[test]
fn ex13_common_shell_executables_rejected() {
    let workspace = TempDir::new("ex13");

    // Some hosts expose several spellings for the same interpreter; every
    // spelling present is checked independently.
    let mut shells = vec![first_existing(&["/bin/sh"])];
    shells.extend(each_existing(&[
        "/bin/bash",
        "/usr/bin/bash",
        "/opt/homebrew/bin/bash",
        "/bin/zsh",
        "/usr/bin/zsh",
    ]));
    for shell in &shells {
        let error = run(&request(shell, &[], workspace.path(), workspace.path()))
            .expect_err("recognized shells must be rejected before spawn");
        match error {
            ExecutionError::ShellExecutableRejected { name } => {
                assert!(["sh", "bash", "zsh"].contains(&name.as_str()));
            }
            other => panic!("expected ShellExecutableRejected, got: {other:?}"),
        }
    }

    // A symlink aliased onto a shell is rejected too: the canonicalized
    // basename check catches aliases, not just literal spellings.
    let alias = workspace.path().join("innocent-helper");
    std::os::unix::fs::symlink("/bin/sh", &alias).expect("shell alias symlink");
    let error = run(&request(alias, &[], workspace.path(), workspace.path()))
        .expect_err("symlink aliased onto a shell must be rejected");
    assert!(matches!(
        error,
        ExecutionError::ShellExecutableRejected { .. }
    ));
}

// --- T13 ----------------------------------------------------------------

#[test]
fn ex14_hostile_parent_environment_is_not_inherited() {
    let workspace = TempDir::new("ex14");
    // Behavioral evidence: the child IS this foundation's production
    // construction (env_clear, nothing added back). If any variable leaked
    // into the child, the probe would exit 7 instead of 0.
    let outcome = run_probe(
        "execution_env_probe_child_exits_zero_only_when_environment_is_empty",
        workspace.path(),
    );
    assert!(
        outcome.success(),
        "child observed a non-empty environment: {outcome:?}"
    );
    assert_eq!(outcome.exit_code(), Some(0));

    // Secondary structural evidence: the prepared command starts from an
    // empty environment by construction.
    let command = prepared_command(Path::new("/usr/bin/true"), Path::new("/"));
    assert_eq!(command.get_envs().count(), 0);
    assert!(!command.get_program().to_string_lossy().is_empty());
}

// --- T14 ----------------------------------------------------------------

#[test]
fn ex15_child_stdin_is_null_not_interactive() {
    let workspace = TempDir::new("ex15");
    // The probe reads stdin from another thread while bounding the wait;
    // with Stdio::null the read observes immediate EOF and exits 0.
    let outcome = run_probe(
        "execution_stdin_probe_child_exits_zero_on_immediate_eof",
        workspace.path(),
    );
    assert!(
        outcome.success(),
        "child did not observe immediate EOF on stdin: {outcome:?}"
    );
    assert_eq!(outcome.exit_code(), Some(0));
}

// --- T15 ----------------------------------------------------------------

#[test]
fn ex16_outcome_carries_no_output_payload() {
    let workspace = TempDir::new("ex16");
    // The child writes to stdout/stderr; both are null at the boundary and
    // the entire result value equals pure exit metadata — nothing else to
    // inspect exists on the type.
    let outcome = run_request(&request(
        first_existing(&["/bin/echo", "/usr/bin/echo"]),
        &["stdout-payload", "stderr-payload"],
        workspace.path(),
        workspace.path(),
    ));
    assert_eq!(outcome, ProcessRunOutcome::new(true, Some(0)));
    assert_ne!(outcome, ProcessRunOutcome::new(false, Some(0)));
}

// --- T16 ----------------------------------------------------------------

#[test]
fn ex17_executable_canonicalized_through_symlink() {
    let workspace = TempDir::new("ex17");
    let target = first_existing(&["/usr/bin/true", "/bin/true"]);
    let canonical_target =
        std::fs::canonicalize(&target).expect("utility resolves to a canonical path");
    let alias = workspace.path().join("alias-to-true");
    std::os::unix::fs::symlink(&target, &alias).expect("executable alias symlink");

    // Internal evidence: validation yields the canonical form, not the
    // lexical alias.
    let validated = validated_executable(&alias).expect("aliased executable validates");
    assert_eq!(validated, canonical_target);

    // Behavioral evidence: execution through the alias succeeds.
    let outcome = run_request(&request(alias, &[], workspace.path(), workspace.path()));
    assert!(outcome.success());
    assert_eq!(outcome.exit_code(), Some(0));
}

// --- T17 ----------------------------------------------------------------

#[test]
fn ex18_spawn_failure_is_typed_and_distinct_from_wait_failure() {
    use std::ffi::OsString;

    // A real, deterministic, race-free spawn failure. Every
    // validation-adjacent failure mode (missing file, non-regular file,
    // missing permission bits, unresolvable path) is already closed by the
    // runner's pre-spawn checks, and deleting a validated executable
    // between validation and spawn would be a manufactured race, so that
    // case is deliberately not forced here. Exec-format damage cannot
    // produce a typed spawn error either: the platform exec wrapper falls
    // back to interpreting such files as shell scripts (observed as a
    // plain non-zero child exit), which is exactly the ambient shell
    // behavior this foundation refuses to rely on or emulate.
    //
    // What remains is kernel-side argv overflow: arguments whose total
    // size exceeds ARG_MAX (and, on some kernels, a single argument over
    // MAX_ARG_STRLEN) fail deterministically at exec time with E2BIG,
    // which surfaces through the typed spawn boundary before any code in
    // the child runs.
    let workspace = TempDir::new("ex18");
    let chunk = "x".repeat(200_000);
    let mut oversized = Vec::new();
    for _ in 0..40 {
        oversized.push(OsString::from(chunk.as_str()));
    }
    let built = ProcessRunRequest::new(
        first_existing(&["/usr/bin/true", "/bin/true"]),
        oversized,
        workspace.path(),
        workspace.path(),
    )
    .expect("oversized argv is structurally valid");

    let error =
        run(&built).expect_err("argv exceeding kernel limits must surface as a spawn error");
    match error {
        ExecutionError::ProcessSpawnFailed { detail } => {
            assert!(!detail.is_empty());
        }
        other => panic!("expected ProcessSpawnFailed, got: {other:?}"),
    }

    // Mapping coverage: io errors map to distinct typed variants at the
    // two boundaries.
    let spawn_error = spawn_failed(std::io::Error::other("boom"));
    let wait_error = wait_failed(std::io::Error::other("boom"));
    assert!(matches!(
        spawn_error,
        ExecutionError::ProcessSpawnFailed { .. }
    ));
    assert!(matches!(
        wait_error,
        ExecutionError::ProcessWaitFailed { .. }
    ));
    assert_ne!(spawn_error.to_string(), wait_error.to_string());
}

// --- Request shape ------------------------------------------------------

#[test]
fn ex19_request_accessors_round_trip_discrete_arguments() {
    let arguments = ["first arg", "second;arg", "third$arg"];
    let built = request(
        "/absolute/helper",
        &arguments,
        Path::new("/ws"),
        Path::new("/ws/cwd"),
    );
    assert_eq!(built.executable(), Path::new("/absolute/helper"));
    assert_eq!(built.workspace_root(), Path::new("/ws"));
    assert_eq!(built.cwd(), Path::new("/ws/cwd"));
    let returned: Vec<String> = built
        .arguments()
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    assert_eq!(returned, arguments);

    // Absolute-path syntax is enforced structurally for every path input.
    assert!(
        ProcessRunRequest::new("relative-helper", std::iter::empty::<&str>(), "/ws", "/ws")
            .is_err()
    );
    assert!(
        ProcessRunRequest::new("/helper", std::iter::empty::<&str>(), "relative-ws", "/ws")
            .is_err()
    );
    assert!(ProcessRunRequest::new("/helper", std::iter::empty::<&str>(), "/ws", "../ws").is_err());

    // An empty argv is legal: some programs take none.
    assert!(
        request(
            first_existing(&["/usr/bin/true", "/bin/true"]),
            &[],
            Path::new("/"),
            Path::new("/")
        )
        .arguments()
        .is_empty()
    );

    // Command construction never mutates through a public surface: the
    // builder is internal and takes only validated values.
    let command = prepared_command(Path::new("/usr/bin/true"), Path::new("/"));
    assert_eq!(
        command.get_program(),
        Path::new("/usr/bin/true").as_os_str()
    );
}
