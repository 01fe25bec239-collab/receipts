//! Synthetic native worker for `protected_launch_tests`. A standalone program,
//! never a crate module: portable tests compile it unprivileged with rustc, and
//! the hosted diagnostic compiles it in its unprivileged `prepare` phase (never
//! as root). It records its initial exec environment, argv, stdin and identity
//! itself, with no shell in between, so nothing such as PWD is added. Never
//! Claude/Codex provider evidence.
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    thread,
    time::Duration,
};

unsafe extern "C" {
    fn getuid() -> u32;
}

fn main() {
    // First, before anything can change it: the environment as exec'd.
    let mut env: Vec<_> = std::env::vars_os().collect();
    env.sort();
    let argv: Vec<_> = std::env::args_os().skip(1).collect();
    fs::write("pid", std::process::id().to_string()).unwrap();
    // SAFETY: getuid has no preconditions and cannot fail.
    fs::write("uid", format!("{}\n", unsafe { getuid() })).unwrap();
    fs::write("env-observed", format!("{env:?}")).unwrap();
    fs::write("argv", format!("{argv:?}")).unwrap();
    let mut received = Vec::new();
    std::io::stdin().read_to_end(&mut received).unwrap();
    fs::write("received", received).unwrap();
    let mut out = std::io::stdout();
    out.write_all(b"{\"type\":\"future\",\"n\":1}\n").unwrap();
    out.flush().unwrap();
    fs::write("ready", "").unwrap();
    while !Path::new("release").exists() {
        thread::sleep(Duration::from_millis(10));
    }
    out.write_all(b"{\"type\":\"future\",\"n\":2}\n").unwrap();
    out.flush().unwrap();
}
