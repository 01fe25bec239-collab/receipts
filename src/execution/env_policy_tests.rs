//! WE-W0-2 / H2 child-environment policy tests.
//!
//! Parent values come from an injected synthetic snapshot source, so no test
//! mutates the process environment. Child delivery is proven by re-invoking
//! this test binary as one ignored probe that compares its whole environment
//! with `NAME=VALUE` expectations passed as discrete argv values and reports
//! only through its exit code. Values here are synthetic sentinels.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{
    BASE_NAMES, ChildEnvAuthMode, ChildEnvPolicy, ChildEnvProvider, MAX_PROJECT_ENV_BYTES,
    NETWORK_NAMES, ProjectEnvConfirmation,
};
use crate::execution::runner::request_command;
use crate::execution::{
    ExecutionError, ProcessRunRequest, ProcessTimeoutPolicy, run, run_with_timeout,
    run_with_timeout_and_capture, start_live_process_attempt,
};

use ChildEnvAuthMode::{ApiKey, Subscription};
use ChildEnvProvider::{Claude, Codex};

const PROBE: &str = "execution::env_policy::env_policy_tests::env_policy_probe_child_exact_env";
const SECRET: &str = "receipts-synthetic-secret-sentinel";

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "receipts-env-policy-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }

    fn file(&self, bytes: &[u8]) -> PathBuf {
        let path = self.0.join("project.env");
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn parent(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let map: BTreeMap<String, OsString> = pairs
        .iter()
        .map(|(name, value)| (name.to_string(), OsString::from(value)))
        .collect();
    move |name| map.get(name).cloned()
}

fn build(
    provider: ChildEnvProvider,
    mode: ChildEnvAuthMode,
    pairs: &[(&str, &str)],
) -> ChildEnvPolicy {
    ChildEnvPolicy::build(provider, mode, None, parent(pairs)).unwrap()
}

fn confirm(bytes: &[u8], names: &[&str], network: bool) -> ProjectEnvConfirmation {
    ProjectEnvConfirmation::new(bytes, names.iter().copied(), network).unwrap()
}

/// Confirms `bytes`/`names`, writes the file and builds a Claude subscription policy.
fn with_file(
    bytes: &[u8],
    names: &[&str],
    network: bool,
    pairs: &[(&str, &str)],
) -> Result<ChildEnvPolicy, ExecutionError> {
    let confirmation = ProjectEnvConfirmation::new(bytes, names.iter().copied(), network)?;
    let ws = Workspace::new();
    let path = ws.file(bytes);
    ChildEnvPolicy::build(
        Claude,
        Subscription,
        Some((&path, &confirmation)),
        parent(pairs),
    )
}

fn assert_entries(policy: &ChildEnvPolicy, expected: &[(&str, &str)]) {
    let expected: BTreeMap<String, OsString> = expected
        .iter()
        .map(|(name, value)| (name.to_string(), OsString::from(value)))
        .collect();
    assert_eq!(policy.entries(), &expected, "{policy:?}");
}

fn probe_request(
    ws: &Workspace,
    policy: Option<ChildEnvPolicy>,
    expected: &[(&str, &str)],
) -> ProcessRunRequest {
    let mut argv = vec![PROBE.to_owned(), "--exact".into(), "--ignored".into()];
    argv.extend(
        expected
            .iter()
            .map(|(name, value)| format!("{name}={value}")),
    );
    let request =
        ProcessRunRequest::new(std::env::current_exe().unwrap(), argv, &ws.0, &ws.0).unwrap();
    match policy {
        Some(policy) => request.with_env_policy(policy),
        None => request,
    }
}

/// Child exit codes from all four production runner paths.
fn exit_codes_on_all_paths(request: &ProcessRunRequest) -> [Option<i32>; 4] {
    let timeout =
        ProcessTimeoutPolicy::new(Duration::from_secs(60), Duration::from_secs(5)).unwrap();
    [
        run(request).unwrap().exit_code(),
        run_with_timeout(request, &timeout).unwrap().exit_code(),
        run_with_timeout_and_capture(request, &timeout)
            .unwrap()
            .outcome()
            .exit_code(),
        start_live_process_attempt(request, &timeout)
            .unwrap()
            .wait_collect()
            .unwrap()
            .exit_code(),
    ]
}

fn assert_child_sees_exactly(policy: Option<ChildEnvPolicy>, expected: &[(&str, &str)]) {
    let ws = Workspace::new();
    let request = probe_request(&ws, policy, expected);
    assert_eq!(exit_codes_on_all_paths(&request), [Some(0); 4]);
}

// --- Child probe -------------------------------------------------------------

#[test]
#[ignore]
fn env_policy_probe_child_exact_env() {
    let expected: BTreeMap<OsString, OsString> = std::env::args_os()
        .skip(1)
        .filter_map(|arg| {
            let arg = arg.into_string().ok()?;
            let (name, value) = arg.split_once('=')?;
            Some((name.into(), value.into()))
        })
        .collect();
    let actual: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    if actual != expected {
        std::process::exit(3);
    }
}

// --- Required named behavior -----------------------------------------------

#[test]
fn default_policy_is_empty() {
    let ws = Workspace::new();
    let request = probe_request(&ws, None, &[]);
    assert!(request.env_policy().is_none());
    assert_eq!(
        request_command(Path::new("/usr/bin/true"), Path::new("/"), &request)
            .get_envs()
            .count(),
        0
    );
    assert_child_sees_exactly(None, &[]);
}

#[test]
fn allowlisted_home_reaches_child() {
    let policy = build(Claude, Subscription, &[("HOME", "/synthetic/home")]);
    assert_entries(&policy, &[("HOME", "/synthetic/home")]);
    assert_child_sees_exactly(Some(policy), &[("HOME", "/synthetic/home")]);
}

fn assert_refused(name: &str) {
    let line = format!("{name}=/synthetic/{SECRET}\n");
    match with_file(line.as_bytes(), &[], false, &[]) {
        Err(ExecutionError::ChildEnvNameRefused { name: refused }) => assert_eq!(refused, name),
        other => panic!("{name}: expected refusal, got {other:?}"),
    }
    // A confirmed extra cannot bypass the refusal either.
    match with_file(line.as_bytes(), &[name], false, &[]) {
        Err(ExecutionError::ChildEnvNameRefused { .. }) => {}
        other => panic!("{name}: expected refusal, got {other:?}"),
    }
    match ProjectEnvConfirmation::new(line.as_bytes(), [name], false) {
        Err(ExecutionError::ChildEnvNameRefused { name: refused }) => assert_eq!(refused, name),
        other => panic!("{name}: expected refusal, got {other:?}"),
    }
}

#[test]
fn ld_preload_refused_at_construction() {
    assert_refused("LD_PRELOAD");
}

#[test]
fn dyld_insert_libraries_refused() {
    assert_refused("DYLD_INSERT_LIBRARIES");
}

#[test]
fn node_options_refused() {
    assert_refused("NODE_OPTIONS");
}

#[test]
fn remaining_refusal_names_and_patterns_refused() {
    for name in [
        "LD_LIBRARY_PATH",
        "LD_",
        "DYLD_LIBRARY_PATH",
        "DYLD_",
        "BASH_ENV",
        "ENV",
        "PROMPT_COMMAND",
        "PYTHONPATH",
        "PERL5OPT",
        "RUBYOPT",
        "GIT_DIR",
        "GIT_SSH_COMMAND",
        "GIT_",
    ] {
        assert_refused(name);
    }
    // Patterns are exact prefixes/names, not substrings.
    let bytes = b"LDX=1\nENVIRONMENT_X=2\nMY_GIT_X=3\n";
    let names = ["LDX", "ENVIRONMENT_X", "MY_GIT_X"];
    assert_entries(
        &with_file(bytes, &names, false, &[]).unwrap(),
        &[("ENVIRONMENT_X", "2"), ("LDX", "1"), ("MY_GIT_X", "3")],
    );
}

#[test]
fn non_allowlisted_parent_var_absent_in_child() {
    // Every name the parent is asked about has a value, including names that
    // are never admitted (other provider, network, refused, arbitrary).
    let all = |_: &str| Some(OsString::from("/synthetic/value"));
    let policy = ChildEnvPolicy::build(Claude, Subscription, None, all).unwrap();
    let mut expected: Vec<(&str, &str)> = BASE_NAMES
        .iter()
        .chain(&["PATH", "CLAUDE_CONFIG_DIR"])
        .map(|name| (*name, "/synthetic/value"))
        .collect();
    expected.sort();
    assert_entries(&policy, &expected);
    assert_child_sees_exactly(Some(policy), &expected);
}

#[test]
fn provider_key_not_passed_to_other_provider() {
    let pairs = [
        ("CLAUDE_CONFIG_DIR", "/synthetic/claude"),
        ("ANTHROPIC_API_KEY", "synthetic-anthropic"),
        ("CODEX_HOME", "/synthetic/codex"),
        ("OPENAI_API_KEY", "synthetic-openai"),
    ];
    let codex = build(Codex, ApiKey, &pairs);
    let codex_expected = [
        ("CODEX_HOME", "/synthetic/codex"),
        ("OPENAI_API_KEY", "synthetic-openai"),
    ];
    assert_entries(&codex, &codex_expected);
    assert_entries(
        &build(Claude, ApiKey, &pairs),
        &[
            ("ANTHROPIC_API_KEY", "synthetic-anthropic"),
            ("CLAUDE_CONFIG_DIR", "/synthetic/claude"),
        ],
    );
    assert_child_sees_exactly(Some(codex), &codex_expected);

    // A confirmed extra cannot smuggle any provider name to either provider.
    for name in [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
    ] {
        let line = format!("{name}=x\n");
        assert!(matches!(
            with_file(line.as_bytes(), &[name], false, &[]),
            Err(ExecutionError::ChildEnvNameReserved { .. })
        ));
    }
}

// --- Further policy coverage -------------------------------------------------

#[test]
fn api_key_only_in_explicit_api_key_mode() {
    let pairs = [
        ("CLAUDE_CONFIG_DIR", "/synthetic/claude"),
        ("ANTHROPIC_API_KEY", "synthetic-anthropic"),
        ("CODEX_HOME", "/synthetic/codex"),
        ("OPENAI_API_KEY", "synthetic-openai"),
    ];
    // Present keys never select API-key mode.
    assert_entries(
        &build(Claude, Subscription, &pairs),
        &[("CLAUDE_CONFIG_DIR", "/synthetic/claude")],
    );
    assert_entries(
        &build(Codex, Subscription, &pairs),
        &[("CODEX_HOME", "/synthetic/codex")],
    );
    // Explicit API-key mode with no parent key stays absent, with no fallback.
    assert_entries(&build(Codex, ApiKey, &[]), &[]);
    assert_entries(&build(Claude, ApiKey, &[]), &[]);
}

#[test]
fn path_is_filtered_to_absolute_entries() {
    let filtered = build(
        Claude,
        Subscription,
        &[("PATH", "/usr/bin::.:relative/bin:/opt/tool/bin:./x:")],
    );
    assert_entries(&filtered, &[("PATH", "/usr/bin:/opt/tool/bin")]);
    for all_rejected in ["", ".", "::.:relative:./x"] {
        assert_entries(&build(Claude, Subscription, &[("PATH", all_rejected)]), &[]);
    }
}

#[test]
fn nul_parent_value_refused_by_name_only() {
    let error = ChildEnvPolicy::build(Claude, Subscription, None, |name| {
        (name == "HOME").then(|| OsString::from(format!("/{SECRET}\0x")))
    })
    .unwrap_err();
    assert!(matches!(&error, ExecutionError::ChildEnvValueInvalid { name } if name == "HOME"));
    assert!(!format!("{error} {error:?}").contains(SECRET));
}

#[test]
fn network_names_need_project_directive_and_confirmation() {
    let pairs: Vec<(&str, &str)> = NETWORK_NAMES
        .iter()
        .map(|name| (*name, "http://synthetic.invalid"))
        .collect();
    // No project file: absent even though the parent has all five.
    assert_entries(&build(Claude, Subscription, &pairs), &[]);
    // Project file without the directive, approval off: absent.
    assert_entries(
        &with_file(b"A=1\n", &["A"], false, &pairs).unwrap(),
        &[("A", "1")],
    );
    // Both present: all five admitted from the parent snapshot.
    let mut expected = pairs.clone();
    expected.push(("A", "1"));
    expected.sort();
    assert_entries(
        &with_file(b"network=on\nA=1\n", &["A"], true, &pairs).unwrap(),
        &expected,
    );
    // Either side alone fails closed.
    for (bytes, approved) in [(&b"network=on\n"[..], false), (&b"A=1\n"[..], true)] {
        let names: &[&str] = if bytes.starts_with(b"A") { &["A"] } else { &[] };
        assert!(matches!(
            with_file(bytes, names, approved, &pairs),
            Err(ExecutionError::ProjectEnvUnconfirmed { .. })
        ));
    }
    // Network names cannot be supplied as extras.
    assert!(matches!(
        with_file(b"HTTPS_PROXY=x\n", &["HTTPS_PROXY"], false, &[]),
        Err(ExecutionError::ChildEnvNameReserved { .. })
    ));
}

#[test]
fn extras_must_exactly_equal_confirmed_names() {
    let bytes = b"A=1\nB=2\n";
    for names in [&["A"][..], &["A", "B", "C"], &[]] {
        assert!(matches!(
            with_file(bytes, names, false, &[]),
            Err(ExecutionError::ProjectEnvUnconfirmed { .. })
        ));
    }
    assert_entries(
        &with_file(bytes, &["B", "A"], false, &[]).unwrap(),
        &[("A", "1"), ("B", "2")],
    );
}

#[test]
fn reserved_base_names_refused_as_extras() {
    for name in ["HOME", "PATH", "TMPDIR", "XDG_STATE_HOME"] {
        let line = format!("{name}=x\n");
        assert!(matches!(
            with_file(line.as_bytes(), &[name], false, &[]),
            Err(ExecutionError::ChildEnvNameReserved { .. })
        ));
    }
}

#[test]
fn duplicate_and_invalid_names_refused() {
    assert!(matches!(
        with_file(b"A=1\nA=2\n", &["A"], false, &[]),
        Err(ExecutionError::ChildEnvNameDuplicated { name }) if name == "A"
    ));
    assert!(matches!(
        ProjectEnvConfirmation::new(b"", ["A", "A"], false),
        Err(ExecutionError::ChildEnvNameDuplicated { .. })
    ));
    for (index, line) in [
        "lower=x",
        "1A=x",
        "A-B=x",
        "=x",
        " A=x",
        "A =x",
        "network=off",
        "NETWORK=on\nnetwork=ON",
    ]
    .iter()
    .enumerate()
    {
        let error = with_file(line.as_bytes(), &[], false, &[]).unwrap_err();
        assert!(
            matches!(error, ExecutionError::ChildEnvNameInvalid { .. }),
            "case {index}: {error:?}"
        );
    }
    assert!(matches!(
        ProjectEnvConfirmation::new(b"", ["ok", "B"], false),
        Err(ExecutionError::ChildEnvNameInvalid { position: 1, .. })
    ));
}

#[test]
fn malformed_project_file_refused() {
    for (bytes, line) in [
        (&b"A=1\r\n"[..], 1),
        (b"A=1\nB=x\0y\n", 2),
        (b"A=1\nno-equals\n", 2),
        (b"\xff=1\n", 0),
        (b"network=on\nnetwork=on\n", 2),
    ] {
        match with_file(bytes, &[], false, &[]) {
            Err(ExecutionError::ProjectEnvFileMalformed { line: actual, .. }) => {
                assert_eq!(actual, line)
            }
            other => panic!("expected malformed line {line}, got {other:?}"),
        }
    }
}

#[test]
fn values_are_literal_including_equals_and_empty() {
    let bytes = b"A=x=y==\n\nEMPTY=\nQUOTED=\"$HOME\" # not a comment";
    assert_entries(
        &with_file(bytes, &["A", "EMPTY", "QUOTED"], false, &[]).unwrap(),
        &[
            ("A", "x=y=="),
            ("EMPTY", ""),
            ("QUOTED", "\"$HOME\" # not a comment"),
        ],
    );
}

#[test]
fn changed_unconfirmed_or_unadmissible_project_file_refused() {
    let ws = Workspace::new();
    let path = ws.file(b"A=1\n");
    let build_with = |path: &Path, confirmation: &ProjectEnvConfirmation| {
        ChildEnvPolicy::build(
            Claude,
            Subscription,
            Some((path, confirmation)),
            parent(&[]),
        )
    };
    // Changed after confirmation.
    fs::write(&path, b"A=2\n").unwrap();
    assert!(matches!(
        build_with(&path, &confirm(b"A=1\n", &["A"], false)),
        Err(ExecutionError::ProjectEnvUnconfirmed { .. })
    ));
    // Trailing-LF difference is still a byte difference.
    assert!(matches!(
        build_with(&path, &confirm(b"A=2", &["A"], false)),
        Err(ExecutionError::ProjectEnvUnconfirmed { .. })
    ));
    for (path, reason) in [
        (
            PathBuf::from("relative/project.env"),
            "path is not absolute",
        ),
        (ws.0.clone(), "not an existing regular file"),
        (ws.0.join("missing.env"), "not an existing regular file"),
    ] {
        match build_with(&path, &confirm(b"", &[], false)) {
            Err(ExecutionError::ProjectEnvFileRejected { reason: actual }) => {
                assert_eq!(actual, reason)
            }
            other => panic!("expected rejection {reason}, got {other:?}"),
        }
    }
    let oversized = vec![b'A'; MAX_PROJECT_ENV_BYTES + 1];
    let big = ws.file(&oversized);
    assert!(matches!(
        build_with(
            &big,
            &confirm(&oversized[..MAX_PROJECT_ENV_BYTES], &[], false)
        ),
        Err(ExecutionError::ProjectEnvFileRejected {
            reason: "exceeds 65536 bytes"
        })
    ));
    assert!(matches!(
        ProjectEnvConfirmation::new(oversized, Vec::<String>::new(), false),
        Err(ExecutionError::ProjectEnvUnconfirmed { .. })
    ));
}

#[test]
fn project_file_is_not_reread_at_spawn() {
    let ws = Workspace::new();
    let path = ws.file(b"A=before\n");
    let confirmation = confirm(b"A=before\n", &["A"], false);
    let policy = ChildEnvPolicy::build(
        Claude,
        Subscription,
        Some((&path, &confirmation)),
        parent(&[]),
    )
    .unwrap();
    fs::write(&path, b"A=after\n").unwrap();
    assert_child_sees_exactly(Some(policy), &[("A", "before")]);
}

#[test]
fn opted_in_policy_reaches_child_on_all_four_paths() {
    let pairs = [
        ("HOME", "/synthetic/home"),
        ("LANG", "C.UTF-8"),
        ("PATH", "/usr/bin:.::rel:/bin"),
        ("CODEX_HOME", "/synthetic/codex"),
        ("OPENAI_API_KEY", "synthetic-openai"),
        ("ANTHROPIC_API_KEY", "synthetic-anthropic"),
        ("HTTPS_PROXY", "http://synthetic.invalid:8080"),
        ("SECRET_TOKEN", SECRET),
    ];
    let bytes = b"network=on\nEXTRA=a=b\nEMPTY=\n";
    let ws = Workspace::new();
    let path = ws.file(bytes);
    let policy = ChildEnvPolicy::build(
        Codex,
        ApiKey,
        Some((&path, &confirm(bytes, &["EXTRA", "EMPTY"], true))),
        parent(&pairs),
    )
    .unwrap();
    assert_child_sees_exactly(
        Some(policy),
        &[
            ("CODEX_HOME", "/synthetic/codex"),
            ("EMPTY", ""),
            ("EXTRA", "a=b"),
            ("HOME", "/synthetic/home"),
            ("HTTPS_PROXY", "http://synthetic.invalid:8080"),
            ("LANG", "C.UTF-8"),
            ("OPENAI_API_KEY", "synthetic-openai"),
            ("PATH", "/usr/bin:/bin"),
        ],
    );
}

#[test]
fn debug_and_errors_show_names_only() {
    let pairs = [("HOME", SECRET), ("ANTHROPIC_API_KEY", SECRET)];
    let bytes = format!("EXTRA={SECRET}\n");
    let ws = Workspace::new();
    let path = ws.file(bytes.as_bytes());
    let confirmation = confirm(bytes.as_bytes(), &["EXTRA"], false);
    let policy =
        ChildEnvPolicy::build(Claude, ApiKey, Some((&path, &confirmation)), parent(&pairs))
            .unwrap();
    let request = ProcessRunRequest::new(&path, [""; 0], &ws.0, &ws.0)
        .unwrap()
        .with_env_policy(policy.clone());
    let clone = request.clone();
    assert_eq!(clone, request);

    let mut rendered = vec![
        format!("{policy:?}"),
        format!("{request:?}"),
        format!("{clone:?}"),
        format!("{confirmation:?}"),
    ];
    for name in ["HOME", "ANTHROPIC_API_KEY", "EXTRA"] {
        assert!(rendered[0].contains(name) && rendered[2].contains(name));
    }
    assert!(rendered[3].contains("EXTRA"));

    let errors = [
        with_file(format!("{SECRET}\n").as_bytes(), &[], false, &[]).unwrap_err(),
        with_file(format!("LD_PRELOAD={SECRET}\n").as_bytes(), &[], false, &[]).unwrap_err(),
        with_file(format!("lower={SECRET}\n").as_bytes(), &[], false, &[]).unwrap_err(),
        with_file(format!("A={SECRET}\r\n").as_bytes(), &[], false, &[]).unwrap_err(),
        ProjectEnvConfirmation::new(SECRET, [SECRET], false).unwrap_err(),
        ChildEnvPolicy::build(
            Claude,
            Subscription,
            Some((&path, &confirm(b"other", &[], false))),
            parent(&[]),
        )
        .unwrap_err(),
    ];
    for error in &errors {
        rendered.push(format!("{error} {error:?}"));
    }
    for text in &rendered {
        assert!(!text.contains(SECRET), "value leaked into: {text}");
    }
}

#[test]
fn real_parent_snapshot_admits_only_allowlisted_names() {
    // Reads (never mutates) the real parent environment; checks names only.
    let policy = ChildEnvPolicy::new(Claude, Subscription).unwrap();
    for name in policy.names() {
        assert!(
            BASE_NAMES.contains(&name) || name == "PATH" || name == "CLAUDE_CONFIG_DIR",
            "unexpected admitted name {name}"
        );
    }
    let policy = ChildEnvPolicy::new(Codex, ApiKey).unwrap();
    assert!(policy.names().all(|name| !name.starts_with("RECEIPTS_")
        && !name.starts_with("ANTHROPIC_")
        && name != "CLAUDE_CONFIG_DIR"));
}

// --- Gated live CLI smokes ---------------------------------------------------
//
// Ignored by default (NOT_RUN). Run explicitly with `--ignored` and
// RECEIPTS_LIVE_CLAUDE=1 + RECEIPTS_LIVE_CLAUDE_BIN=<absolute path> (or the
// CODEX pair). Only `--version` runs; the gate variables are never admitted.

fn live_version(flag: &str, bin_var: &str, provider: ChildEnvProvider) {
    assert_eq!(
        std::env::var_os(flag).as_deref(),
        Some(std::ffi::OsStr::new("1")),
        "{flag}=1 is required; a disabled gate is NOT_RUN, not a pass"
    );
    let executable = PathBuf::from(
        std::env::var_os(bin_var).unwrap_or_else(|| panic!("{bin_var} must name the CLI")),
    );
    assert!(executable.is_absolute(), "{bin_var} must be absolute");
    let policy = ChildEnvPolicy::new(provider, Subscription).unwrap();
    assert!(policy.names().all(|name| !name.starts_with("RECEIPTS_")));
    let names: Vec<&str> = policy.names().collect();
    let ws = Workspace::new();
    let request = ProcessRunRequest::new(&executable, ["--version"], &ws.0, &ws.0)
        .unwrap()
        .with_env_policy(policy.clone());
    let timeout =
        ProcessTimeoutPolicy::new(Duration::from_secs(30), Duration::from_secs(2)).unwrap();
    let captured = run_with_timeout_and_capture(&request, &timeout).unwrap();
    println!(
        "LIVE {flag}: executable={} canonical={} platform={}-{} termination={:?} exit={:?} \
         names={names:?} version={:?}",
        executable.display(),
        fs::canonicalize(&executable).unwrap().display(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        captured.outcome().termination(),
        captured.outcome().exit_code(),
        String::from_utf8_lossy(captured.stdout().head()).trim(),
    );
    assert!(captured.outcome().success());
    assert!(!captured.stdout().head().is_empty());
}

#[test]
#[ignore = "gated: RECEIPTS_LIVE_CLAUDE=1 and RECEIPTS_LIVE_CLAUDE_BIN"]
fn live_claude_version_with_policy() {
    live_version("RECEIPTS_LIVE_CLAUDE", "RECEIPTS_LIVE_CLAUDE_BIN", Claude);
}

#[test]
#[ignore = "gated: RECEIPTS_LIVE_CODEX=1 and RECEIPTS_LIVE_CODEX_BIN"]
fn live_codex_version_with_policy() {
    live_version("RECEIPTS_LIVE_CODEX", "RECEIPTS_LIVE_CODEX_BIN", Codex);
}
