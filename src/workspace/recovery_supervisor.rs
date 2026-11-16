//! Fixture-only surviving owner. No production admission, signal or reset API.
//! A disconnected client owns nothing; the supervisor retains the original
//! process handle and directory pins. Same-user hostile mutation is out of scope.

use crate::execution::{
    LiveProcessAttempt, ProcessRunRequest, ProcessTimeoutPolicy, start_live_process_attempt,
};
use crate::protected_custody::Dir;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const IO_TIMEOUT: Duration = Duration::from_millis(300);

struct Custody {
    directories: Vec<(PathBuf, Dir)>,
    pointer: (PathBuf, File, Vec<u8>),
    _reservation: File,
}
impl Custody {
    fn acquire(worktree: &Path, git: &Path, common: &Path) -> io::Result<Self> {
        match fs::symlink_metadata(common.join("receipts-custody.state")) {
            Ok(_) => return Err(io::Error::other("protected custody remains quarantined")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let mut directories = Vec::new();
        for path in [worktree, git, common] {
            if fs::canonicalize(path)? != path {
                return Err(io::Error::other("noncanonical custody"));
            }
            for ancestor in path.ancestors() {
                if !directories.iter().any(|(p, _)| p == ancestor) {
                    directories.push((ancestor.to_owned(), Dir::open_absolute(ancestor)?));
                }
            }
        }
        let pointer_path = worktree.join(".git");
        let listed = fs::symlink_metadata(&pointer_path)?;
        if !listed.is_file() || listed.nlink() != 1 {
            return Err(io::Error::other("invalid Git pointer"));
        }
        let mut pointer = File::open(&pointer_path)?;
        let opened = pointer.metadata()?;
        if (listed.dev(), listed.ino()) != (opened.dev(), opened.ino()) || opened.len() > 8192 {
            return Err(io::Error::other("changed Git pointer"));
        }
        let mut bytes = Vec::new();
        pointer.read_to_end(&mut bytes)?;
        // Retain a refused/crashed reservation; only the disposable fixture removes it.
        let reservation = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(common.join("receipts-recovery-fixture-reservation"))?;
        let custody = Self {
            directories,
            pointer: (pointer_path, pointer, bytes),
            _reservation: reservation,
        };
        custody.verify()?;
        Ok(custody)
    }
    fn verify(&self) -> io::Result<()> {
        for (path, original) in &self.directories {
            let current = Dir::open_absolute(path)?;
            let pinned = original.current()?;
            if (current.meta.dev, current.meta.ino) != (pinned.dev, pinned.ino) {
                return Err(io::Error::other("replaced custody directory"));
            }
        }
        let (path, pinned, bytes) = &self.pointer;
        let current = fs::symlink_metadata(path)?;
        let original = pinned.metadata()?;
        if !current.is_file()
            || current.nlink() != 1
            || (current.dev(), current.ino()) != (original.dev(), original.ino())
            || current.len() > 8192
            || fs::read(path)? != *bytes
        {
            return Err(io::Error::other("replaced Git pointer"));
        }
        Ok(())
    }
}

pub(super) struct Supervisor {
    // Terminate/reap before dropping custody pins.
    attempt: LiveProcessAttempt,
    listener: UnixListener,
    capability: [u8; 32],
    incarnation: [u8; 32],
    custody: Custody,
}
impl Supervisor {
    pub(super) fn start(
        root: &Path,
        worktree: &Path,
        git: &Path,
        common: &Path,
        request: &ProcessRunRequest,
    ) -> io::Result<Self> {
        let custody = Custody::acquire(worktree, git, common)?;
        let mut capability = [0; 32];
        let mut incarnation = [0; 32];
        let mut random = File::open("/dev/urandom")?;
        random.read_exact(&mut capability)?;
        random.read_exact(&mut incarnation)?;
        let listener = UnixListener::bind(root.join("socket"))?;
        listener.set_nonblocking(true)?;
        let policy =
            ProcessTimeoutPolicy::new(Duration::from_secs(20), Duration::from_millis(50)).unwrap();
        let attempt = start_live_process_attempt(request, &policy)
            .map_err(|_| io::Error::other("fixture launch failed"))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join("capability"))?;
        file.write_all(&capability)?;
        Ok(Self {
            listener,
            capability,
            incarnation,
            custody,
            attempt,
        })
    }
    pub(super) fn serve(&self, root: &Path) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(12);
        while !root.join("stop").exists() {
            if Instant::now() >= deadline {
                return Err(io::Error::other("fixture supervisor deadline"));
            }
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    // An already disconnected peer can fail socket setup on macOS.
                    if stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
                        || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
                    {
                        continue;
                    }
                    let mut capability = [0; 32];
                    if stream.read_exact(&mut capability).is_err() {
                        continue;
                    }
                    let mut reply = [0; 33];
                    if capability == self.capability
                        && self.custody.verify().is_ok()
                        && !self.attempt.is_terminating_or_closed()
                    {
                        reply[0] = 1;
                        reply[1..].copy_from_slice(&self.incarnation);
                    }
                    // Client disconnect never releases custody or cancels the child.
                    let _ = stream.write_all(&reply);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    pub(super) fn finish(self) -> io::Result<()> {
        self.attempt.cancel();
        self.attempt
            .wait_collect()
            .map_err(|_| io::Error::other("fixture cleanup unverified"))?;
        Ok(())
    }
}

pub(super) fn inspect(root: &Path, capability: &[u8; 32]) -> io::Result<[u8; 32]> {
    let mut stream = UnixStream::connect(root.join("socket"))?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    stream.write_all(capability)?;
    let mut reply = [0; 33];
    stream.read_exact(&mut reply)?;
    if reply[0] != 1 {
        return Err(io::Error::other("fixture authority or custody refused"));
    }
    Ok(reply[1..].try_into().unwrap())
}

#[path = "recovery_supervisor_tests.rs"]
mod tests;
