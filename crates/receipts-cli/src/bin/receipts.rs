//! `receipts hook claude <Event>`: the only command in this build (W2).
//! The full product CLI is HI-W4-1. Hook JSON is read from stdin (bounded);
//! only exact host-set variable names are read from the environment.

use std::io::{Read, Write};
use std::path::PathBuf;

use receipts_cli::hook::{ClaudeHookContext, run_claude_hook};
use receipts_host_integration::claude::CLAUDE_HOOK_PAYLOAD_MAX_BYTES;

fn absolute(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [command, directory, executable, project, plugin] = args.as_slice()
        && command == "codex-native-enroll"
    {
        match receipts_host_integration::codex::native::enroll(
            std::path::Path::new(directory),
            std::path::Path::new(executable),
            std::path::Path::new(project),
            std::path::Path::new(plugin),
        ) {
            Ok(()) => return,
            Err(error) => {
                eprintln!("receipts: {error}");
                std::process::exit(1);
            }
        }
    }
    let [hook, host, event] = args.as_slice() else {
        eprintln!(
            "usage: receipts hook <claude|codex> <Event> | receipts codex-native-enroll <directory> <executable> <project> <plugin>"
        );
        std::process::exit(1);
    };
    if hook != "hook" || !matches!(host.as_str(), "claude" | "codex") {
        eprintln!(
            "usage: receipts hook <claude|codex> <Event> | receipts codex-native-enroll <directory> <executable> <project> <plugin>"
        );
        std::process::exit(1);
    }
    // One byte past the bound is enough to refuse oversized input.
    let mut stdin = Vec::new();
    let read = std::io::stdin()
        .lock()
        .take(CLAUDE_HOOK_PAYLOAD_MAX_BYTES as u64 + 1)
        .read_to_end(&mut stdin);
    if read.is_err() {
        stdin.clear();
    }
    if host == "codex" {
        let delivery = absolute("RECEIPTS_CODEX_ENROLLMENT")
            .ok_or("native enrollment unavailable; supervised fallback required")
            .and_then(|path| {
                receipts_host_integration::codex::native::NativeCodexDelivery::admit(&path)
            })
            .and_then(|delivery| {
                delivery.deliver(
                    event,
                    &stdin,
                    receipts_cli::hook::new_event_id(),
                    receipts_host_integration::claude::utc_now(),
                )
            });
        if let Err(error) = delivery {
            eprintln!("receipts: {error}");
        }
        return;
    }
    let ctx = ClaudeHookContext {
        claudecode: std::env::var_os("CLAUDECODE").is_some_and(|v| v == "1"),
        project_dir: absolute("CLAUDE_PROJECT_DIR"),
        plugin_root: absolute("CLAUDE_PLUGIN_ROOT"),
        plugin_data: absolute("CLAUDE_PLUGIN_DATA"),
        claude_executable: absolute("RECEIPTS_CLAUDE_EXECUTABLE"),
    };
    let out = run_claude_hook(event, &stdin, &ctx);
    let _ = std::io::stdout().write_all(out.stdout.as_bytes());
    let _ = std::io::stderr().write_all(out.stderr.as_bytes());
    std::process::exit(out.exit_code);
}
