use super::*;
use crate::test_support::{TempDir, TestRepo, git};
use std::{
    os::unix::fs::PermissionsExt,
    process::{Child, Command, Stdio},
};

const HELPER: &str = "recovery_supervisor::tests::";
fn helper(name: &str, root: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            format!("{HELPER}{name}"),
            "--exact".into(),
            "--ignored".into(),
            "--nocapture".into(),
        ])
        .env_clear()
        .env("RECEIPTS_D4_FIXTURE", root)
        .stdout(Stdio::null());
    command
}
fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "fixture deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn worker_present(pid: &str) -> bool {
    Command::new("/bin/ps")
        .args(["-p", pid, "-o", "pid="])
        .output()
        .unwrap()
        .status
        .success()
}

struct Fixture {
    // Stop/reap the supervisor before removing any fixture directories.
    server: Option<Child>,
    private: TempDir,
    _repo: TestRepo,
    worktree: PathBuf,
    git: PathBuf,
    common: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let private = TempDir::new("d4");
        fs::set_permissions(private.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let repo = TestRepo::new("d4-repository");
        let worktree = fs::canonicalize(repo.path()).unwrap().join("wt");
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "fixture",
                worktree.to_str().unwrap(),
            ],
        );
        let common = fs::canonicalize(repo.path().join(".git")).unwrap();
        let admin =
            fs::canonicalize(crate::test_support::worktree_meta_dir(&repo, &worktree).unwrap())
                .unwrap();
        fs::write(
            private.path().join("paths"),
            serde_json::to_vec(&[&worktree, &admin, &common]).unwrap(),
        )
        .unwrap();
        let server = helper("supervisor_probe", private.path()).spawn().unwrap();
        let fixture = Self {
            server: Some(server),
            private,
            _repo: repo,
            worktree,
            git: admin,
            common,
        };
        until(|| fixture.private.path().join("ready").exists());
        fixture
    }
    fn capability(&self) -> [u8; 32] {
        fs::read(self.private.path().join("capability"))
            .unwrap()
            .try_into()
            .unwrap()
    }
    fn inspect(&self) -> io::Result<[u8; 32]> {
        inspect(self.private.path(), &self.capability())
    }
    fn controller(&self) -> Vec<u8> {
        let mut child = helper("controller_probe", self.private.path())
            .spawn()
            .unwrap();
        until(|| child.try_wait().unwrap().is_some());
        assert!(
            child.wait().unwrap().success(),
            "controller must exit successfully"
        );
        fs::read(self.private.path().join("controller-result")).unwrap()
    }
    fn stop(&mut self) {
        if let Some(mut server) = self.server.take() {
            fs::write(self.private.path().join("stop"), b"").unwrap();
            until(|| server.try_wait().unwrap().is_some());
            assert!(
                server.wait().unwrap().success(),
                "supervisor must clean up before exit"
            );
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut server) = self.server.take() {
            let _ = fs::write(self.private.path().join("stop"), b"");
            let deadline = Instant::now() + Duration::from_secs(5);
            while matches!(server.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if matches!(server.try_wait(), Ok(None)) {
                let _ = server.kill();
            }
            let _ = server.wait();
        }
    }
}

#[test]
#[ignore]
fn supervisor_probe() {
    let root = PathBuf::from(std::env::var_os("RECEIPTS_D4_FIXTURE").unwrap());
    let paths: Vec<PathBuf> =
        serde_json::from_slice(&fs::read(root.join("paths")).unwrap()).unwrap();
    let executable = std::env::current_exe().unwrap();
    let request = ProcessRunRequest::new(
        executable,
        [
            format!("{HELPER}worker_probe"),
            "--exact".into(),
            "--ignored".into(),
        ],
        &paths[0],
        &paths[0],
    )
    .unwrap();
    let supervisor = Supervisor::start(&root, &paths[0], &paths[1], &paths[2], &request).unwrap();
    until(|| paths[0].join("worker-ready").exists());
    fs::write(root.join("ready"), b"").unwrap();
    supervisor.serve(&root).unwrap();
    supervisor.finish().unwrap();
    fs::write(root.join("cleaned"), b"").unwrap();
}
#[test]
#[ignore]
fn controller_probe() {
    let root = PathBuf::from(std::env::var_os("RECEIPTS_D4_FIXTURE").unwrap());
    let capability = fs::read(root.join("capability"))
        .unwrap()
        .try_into()
        .unwrap();
    let incarnation = inspect(&root, &capability).unwrap();
    fs::write(root.join("controller-result"), incarnation).unwrap();
}
#[test]
#[ignore]
fn worker_probe() {
    fs::write("worker-ready", std::process::id().to_string()).unwrap();
    for tick in 0..4000 {
        if Path::new("worker-stop").exists() {
            return;
        }
        fs::write("pulse", tick.to_string()).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn controller_exit_reconnect_retains_original_process_and_custody() {
    let mut f = Fixture::new();
    let pid = fs::read(f.worktree.join("worker-ready")).unwrap();
    let first = f.controller(); // Real controller subprocess has exited/reaped.
    let pulse = fs::read(f.worktree.join("pulse")).unwrap();
    until(|| fs::read(f.worktree.join("pulse")).is_ok_and(|p| p != pulse));
    assert_eq!(first, f.controller());
    assert_eq!(pid, fs::read(f.worktree.join("worker-ready")).unwrap());
    let competitor = Custody::acquire(&f.worktree, &f.git, &f.common);
    assert!(
        competitor.is_err(),
        "reconnect must retain exclusive reservation"
    );
    assert_eq!(f.inspect().unwrap().as_slice(), first);
    f.stop();
    assert!(f.private.path().join("cleaned").exists());
    assert!(!worker_present(std::str::from_utf8(&pid).unwrap()));
}

#[test]
fn wrong_capability_and_disconnected_client_do_not_release_custody() {
    let f = Fixture::new();
    let original = f.inspect().unwrap();
    let mut wrong = f.capability();
    wrong[0] ^= 1;
    assert!(inspect(f.private.path(), &wrong).is_err());
    drop(UnixStream::connect(f.private.path().join("socket")).unwrap());
    assert_eq!(original, f.inspect().unwrap());
    assert!(Custody::acquire(&f.worktree, &f.git, &f.common).is_err());
}

#[test]
fn protected_quarantine_is_never_adopted_or_changed() {
    let f = Fixture::new();
    let marker = f.common.join("receipts-custody.state");
    fs::write(&marker, b"unknown prior protected custody").unwrap();
    let error = match Custody::acquire(&f.worktree, &f.git, &f.common) {
        Ok(_) => panic!("protected custody must refuse"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("protected custody"));
    assert_eq!(
        fs::read(marker).unwrap(),
        b"unknown prior protected custody"
    );
}

#[test]
fn supervisor_loss_refuses_reconnect_and_leaves_reservation() {
    let mut f = Fixture::new();
    let incarnation = f.inspect().unwrap();
    let pid = fs::read_to_string(f.worktree.join("worker-ready")).unwrap();
    let mut server = f.server.take().unwrap();
    server.kill().unwrap();
    server.wait().unwrap();
    // The owned disposable worker exits cooperatively after supervisor loss.
    fs::write(f.worktree.join("worker-stop"), b"").unwrap();
    until(|| !worker_present(&pid));
    assert!(f.inspect().is_err());
    assert!(Custody::acquire(&f.worktree, &f.git, &f.common).is_err());
    assert_ne!(incarnation, [0; 32]);
}

#[test]
fn replaced_worktree_admin_common_and_pointer_refuse() {
    for target in 0..4 {
        let f = Fixture::new();
        let path = match target {
            0 => f.worktree.clone(),
            1 => f.git.clone(),
            2 => f.common.clone(),
            _ => f.worktree.join(".git"),
        };
        let retained = path.with_extension("retained");
        fs::rename(&path, &retained).unwrap();
        if target == 3 {
            fs::write(&path, fs::read(&retained).unwrap()).unwrap();
        } else {
            fs::create_dir(&path).unwrap();
        }
        assert!(
            f.inspect().is_err(),
            "physical replacement {target} must refuse"
        );
        if target == 3 {
            fs::remove_file(&path).unwrap();
        } else {
            fs::remove_dir(&path).unwrap();
        }
        fs::rename(&retained, &path).unwrap();
        assert!(
            f.inspect().is_ok(),
            "restoring the pinned object preserves original custody"
        );
    }
}
