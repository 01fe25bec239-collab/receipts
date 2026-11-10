//! Descriptor-relative Host storage. No symlink traversal or permission repair.
//! Custody assumes the owning user is trusted; this is not OS confinement.
use std::ffi::{CString, OsStr};
use std::fs::{File, Metadata};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::{Component, Path, PathBuf};

fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "unsafe Host storage entry")
}
fn name(value: &OsStr) -> io::Result<CString> {
    if Path::new(value).components().count() != 1
        || !matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err(refused());
    }
    CString::new(value.as_bytes()).map_err(|_| refused())
}
fn fd_file(fd: i32) -> io::Result<File> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: a successful open returns a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
fn uid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}
fn safe_dir(meta: &Metadata, owned: bool) -> io::Result<()> {
    if !meta.is_dir() || (meta.uid() != uid() && (owned || meta.uid() != 0)) {
        return Err(refused());
    }
    // Root-owned sticky temporary ancestors are allowed; storage itself is not.
    if meta.mode() & 0o022 != 0 && (owned || meta.uid() != 0 || meta.mode() & 0o1000 == 0) {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}
fn same(a: &Metadata, b: &Metadata) -> bool {
    (a.dev(), a.ino()) == (b.dev(), b.ino())
}

#[derive(Debug)]
pub struct HostDirectory {
    file: File,
    path: PathBuf,
}
impl PartialEq for HostDirectory {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.file.metadata().ok().map(|m| (m.dev(), m.ino()))
                == other.file.metadata().ok().map(|m| (m.dev(), m.ino()))
    }
}
impl Eq for HostDirectory {}
impl HostDirectory {
    /// Walks from `/` using O_DIRECTORY|O_NOFOLLOW. The leaf must be user-owned.
    pub fn open(path: &Path) -> io::Result<Self> {
        let dir = Self::open_read(path)?;
        safe_dir(&dir.file.metadata()?, true)?;
        Ok(dir)
    }
    /// Read-only trusted-owner directories may belong to root.
    pub fn open_read(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(refused());
        }
        let mut dir = Self {
            file: File::open("/")?,
            path: PathBuf::from("/"),
        };
        safe_dir(&dir.file.metadata()?, false)?;
        for part in path.components().skip(1) {
            let Component::Normal(part) = part else {
                return Err(refused());
            };
            dir = dir.child_inner(part, false, false)?;
        }
        Ok(dir)
    }
    /// Creates only missing normal components below an already validated directory.
    pub fn create_path(path: &Path) -> io::Result<Self> {
        match Self::open(path) {
            Ok(dir) => Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let parent = Self::create_path(path.parent().ok_or_else(refused)?)?;
                parent.child(path.file_name().ok_or_else(refused)?, true)
            }
            Err(e) => Err(e),
        }
    }
    fn child_inner(&self, part: &OsStr, create: bool, owned: bool) -> io::Result<Self> {
        let n = name(part)?;
        if create {
            // SAFETY: the directory descriptor and terminated component are valid.
            let result = unsafe { libc::mkdirat(self.file.as_raw_fd(), n.as_ptr(), 0o700) };
            if result != 0 && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists {
                return Err(io::Error::last_os_error());
            }
        }
        // SAFETY: openat uses a live directory and a single terminated component.
        let file = fd_file(unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                n.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        safe_dir(&file.metadata()?, owned)?;
        Ok(Self {
            file,
            path: self.path.join(part),
        })
    }
    pub fn child(&self, part: &OsStr, create: bool) -> io::Result<Self> {
        self.verify()?;
        safe_dir(&self.file.metadata()?, true)?;
        self.child_inner(part, create, true)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn verify(&self) -> io::Result<()> {
        let current = Self::open_read(&self.path)?;
        if same(&current.file.metadata()?, &self.file.metadata()?) {
            Ok(())
        } else {
            Err(refused())
        }
    }
    /// Opens one regular single-link file. Private files require exact 0600 mode.
    pub fn open_file(&self, part: &OsStr, append: bool, private: bool) -> io::Result<File> {
        self.verify()?;
        if append || private {
            safe_dir(&self.file.metadata()?, true)?;
        }
        let n = name(part)?;
        let flags = if append {
            libc::O_RDWR | libc::O_APPEND | libc::O_CREAT
        } else {
            libc::O_RDONLY
        };
        // SAFETY: openat uses a live directory and a single terminated component.
        let file = fd_file(unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                n.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        self.verify_file(part, &file, private)?;
        Ok(file)
    }
    pub fn verify_file(&self, part: &OsStr, file: &File, private: bool) -> io::Result<()> {
        self.verify()?;
        let m = file.metadata()?;
        if !m.is_file()
            || m.nlink() != 1
            || (m.uid() != uid() && (private || m.uid() != 0))
            || m.mode() & 0o022 != 0
        {
            return Err(refused());
        }
        if private && m.mode() & 0o777 != 0o600 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let n = name(part)?;
        // SAFETY: reopen without following links to compare the currently linked inode.
        let linked = fd_file(unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                n.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        })?;
        if !same(&linked.metadata()?, &m) {
            return Err(refused());
        }
        Ok(())
    }
    pub fn read(&self, part: &OsStr, private: bool, max: u64) -> io::Result<Vec<u8>> {
        let file = self.open_file(part, false, private)?;
        let mut bytes = Vec::new();
        (&file).take(max + 1).read_to_end(&mut bytes)?;
        self.verify_file(part, &file, private)?;
        if bytes.len() as u64 > max {
            return Err(refused());
        }
        Ok(bytes)
    }
    /// Atomic replacement while the caller holds its storage lock. Old bytes survive failures before rename.
    pub fn replace(&self, part: &OsStr, bytes: &[u8]) -> io::Result<()> {
        self.verify()?;
        safe_dir(&self.file.metadata()?, true)?;
        match self.open_file(part, false, true) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temp = format!(
            ".host-write-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let n = name(OsStr::new(&temp))?;
        // SAFETY: valid directory and single terminated filename, new owned descriptor.
        let mut file = fd_file(unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                n.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            self.verify_file(OsStr::new(&temp), &file, true)?;
            let target = name(part)?;
            // SAFETY: both names are single components within the pinned directory.
            if unsafe {
                libc::renameat(
                    self.file.as_raw_fd(),
                    n.as_ptr(),
                    self.file.as_raw_fd(),
                    target.as_ptr(),
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            self.file.sync_all()
        })();
        if result.is_err() {
            // SAFETY: remove only this create-new temporary entry in the pinned directory.
            unsafe {
                libc::unlinkat(self.file.as_raw_fd(), n.as_ptr(), 0);
            }
        }
        result
    }
}

/// Locks the opened inode until its last descriptor is closed. Never inherited by exec.
pub fn lock(file: &File) -> io::Result<()> {
    loop {
        // SAFETY: flock operates on a live descriptor.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}
