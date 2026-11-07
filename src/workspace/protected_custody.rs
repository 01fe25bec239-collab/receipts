//! Descriptor-relative custody primitives and the owner-controlled custody
//! reservation used by [`crate::protected`].
//!
//! * **Reservation (one custody owner per Git common directory).** A marker
//!   file is created with `O_CREAT|O_EXCL|O_NOFOLLOW` inside the *actual*
//!   common directory (a linked worktree and every launcher or process that
//!   resolves the same directory therefore meet at the same inode) and held
//!   under `flock(LOCK_EX|LOCK_NB)` for the admission's lifetime. The marker
//!   persists its attributable state (attempt, phase, true original owner,
//!   directory identities). Process exit releases only the lock: the marker
//!   stays, so a crashed or incomplete admission refuses new admissions
//!   ("unknown prior custody") instead of making the resource look free. Only
//!   a verified, complete release deletes it. There is no recovery path here.
//! * **Stable objects.** Trees are walked through directory descriptors with
//!   `openat(O_NOFOLLOW)`; ownership and mode are applied to the same opened
//!   object (`fchown`/`fchmod`), never to a pathname. A symlink is never
//!   opened: it is re-owned in place with `fchownat(AT_SYMLINK_NOFOLLOW)` by
//!   single-component name inside its verified parent directory (payload
//!   only; metadata symlinks are unsupported). Regular files with more than one
//!   hard link (possible external aliases), special files, entries on another
//!   device and metadata symlinks refuse **before** the first mutation, and the
//!   same checks repeat on every object that is about to be mutated.
//!
//! Targets: Linux x86_64/aarch64 (the qualified launcher is x86_64) and macOS
//! aarch64 (unit tests of this logic). Everything else refuses.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

#[cfg(any(
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    all(target_os = "macos", target_arch = "aarch64")
))]
mod imp {
    use std::ffi::{CStr, CString, OsStr, OsString, c_char, c_int, c_uint};
    use std::fs::File;
    use std::io;
    use std::io::{Read, Write};
    use std::mem::ManuallyDrop;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard};

    mod sys {
        use std::ffi::{c_char, c_int, c_uint, c_void};

        #[cfg(target_os = "linux")]
        pub(super) type ModeT = u32;
        #[cfg(target_os = "macos")]
        pub(super) type ModeT = u16;

        pub(super) const O_RDONLY: c_int = 0;
        pub(super) const O_RDWR: c_int = 2;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        pub(super) const O_NOFOLLOW: c_int = 0o400_000;
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        pub(super) const O_NOFOLLOW: c_int = 0o100_000;
        #[cfg(target_os = "macos")]
        pub(super) const O_NOFOLLOW: c_int = 0x0100;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        pub(super) const O_DIRECTORY: c_int = 0o200_000;
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        pub(super) const O_DIRECTORY: c_int = 0o40_000;
        #[cfg(target_os = "macos")]
        pub(super) const O_DIRECTORY: c_int = 0x0010_0000;
        #[cfg(target_os = "linux")]
        pub(super) const O_CLOEXEC: c_int = 0o2_000_000;
        #[cfg(target_os = "macos")]
        pub(super) const O_CLOEXEC: c_int = 0x0100_0000;
        #[cfg(target_os = "linux")]
        pub(super) const O_NONBLOCK: c_int = 0o4_000;
        #[cfg(target_os = "macos")]
        pub(super) const O_NONBLOCK: c_int = 0x0004;
        #[cfg(target_os = "linux")]
        pub(super) const O_CREAT: c_int = 0o100;
        #[cfg(target_os = "macos")]
        pub(super) const O_CREAT: c_int = 0x0200;
        #[cfg(target_os = "linux")]
        pub(super) const O_EXCL: c_int = 0o200;
        #[cfg(target_os = "macos")]
        pub(super) const O_EXCL: c_int = 0x0800;
        #[cfg(target_os = "linux")]
        pub(super) const AT_SYMLINK_NOFOLLOW: c_int = 0x100;
        #[cfg(target_os = "macos")]
        pub(super) const AT_SYMLINK_NOFOLLOW: c_int = 0x20;
        #[cfg(target_os = "linux")]
        pub(super) const ELOOP: i32 = 40;
        #[cfg(target_os = "macos")]
        pub(super) const ELOOP: i32 = 62;
        pub(super) const LOCK_EX: c_int = 2;
        pub(super) const LOCK_NB: c_int = 4;
        pub(super) const LOCK_UN: c_int = 8;
        /// Offset of `d_name` in `struct dirent` (glibc / Darwin 64-bit inode).
        #[cfg(target_os = "linux")]
        pub(super) const DIRENT_NAME_OFFSET: usize = 19;
        #[cfg(target_os = "macos")]
        pub(super) const DIRENT_NAME_OFFSET: usize = 21;

        unsafe extern "C" {
            pub(super) fn openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
            pub(super) fn fchown(fd: c_int, uid: c_uint, gid: c_uint) -> c_int;
            pub(super) fn fchownat(
                dirfd: c_int,
                path: *const c_char,
                uid: c_uint,
                gid: c_uint,
                flags: c_int,
            ) -> c_int;
            pub(super) fn fchmod(fd: c_int, mode: ModeT) -> c_int;
            pub(super) fn flock(fd: c_int, operation: c_int) -> c_int;
            pub(super) fn unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int;
            pub(super) fn fdopendir(fd: c_int) -> *mut c_void;
            pub(super) fn readdir(dir: *mut c_void) -> *mut c_void;
            pub(super) fn rewinddir(dir: *mut c_void);
            pub(super) fn closedir(dir: *mut c_void) -> c_int;
            #[cfg(target_os = "linux")]
            pub(super) fn __errno_location() -> *mut c_int;
            #[cfg(target_os = "macos")]
            pub(super) fn __error() -> *mut c_int;
        }
    }

    const MARKER: &CStr = c"receipts-custody.state";
    pub(crate) const MAX_DEPTH: usize = 256;
    const MARKER_READ_LIMIT: usize = 4096;
    const FORMAT: &str = "receipts-custody-v1";

    fn check(result: c_int) -> io::Result<c_int> {
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(result)
        }
    }

    fn errno() -> *mut c_int {
        // SAFETY: the libc errno accessor has no preconditions.
        #[cfg(target_os = "linux")]
        unsafe {
            sys::__errno_location()
        }
        #[cfg(target_os = "macos")]
        unsafe {
            sys::__error()
        }
    }

    fn openat_raw(dir: RawFd, name: &CStr, flags: c_int, mode: c_uint) -> io::Result<OwnedFd> {
        // SAFETY: `name` is NUL-terminated and outlives the call; the result
        // is checked before it is wrapped.
        let fd = check(unsafe { sys::openat(dir, name.as_ptr(), flags, mode) })?;
        // SAFETY: `fd` is a fresh descriptor returned by `openat`.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    fn cstring(name: &OsStr) -> io::Result<CString> {
        CString::new(name.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    }

    /// What an opened object is. A symlink is never opened.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Kind {
        Dir,
        File,
        Symlink,
        Other,
    }

    /// Metadata of one opened object (`fstat` of the descriptor itself).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Meta {
        pub(crate) kind: Kind,
        pub(crate) uid: u32,
        pub(crate) gid: u32,
        pub(crate) mode: u32,
        pub(crate) nlink: u64,
        pub(crate) dev: u64,
        pub(crate) ino: u64,
    }

    fn meta_of(fd: RawFd) -> io::Result<Meta> {
        // SAFETY: `fd` is open for the duration of the call; ManuallyDrop
        // keeps the temporary `File` from closing it.
        let file = ManuallyDrop::new(unsafe { File::from_raw_fd(fd) });
        let metadata = file.metadata()?;
        let kind = {
            let file_type = metadata.file_type();
            if file_type.is_dir() {
                Kind::Dir
            } else if file_type.is_file() {
                Kind::File
            } else if file_type.is_symlink() {
                Kind::Symlink
            } else {
                Kind::Other
            }
        };
        Ok(Meta {
            kind,
            uid: metadata.uid(),
            gid: metadata.gid(),
            mode: metadata.mode() & 0o7777,
            nlink: metadata.nlink(),
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }

    /// An open directory object.
    #[derive(Debug)]
    pub(crate) struct Dir {
        fd: OwnedFd,
        pub(crate) meta: Meta,
    }

    /// One child opened without following links.
    #[derive(Debug)]
    pub(crate) struct Node {
        pub(crate) name: CString,
        pub(crate) kind: Kind,
        /// `None` for a symlink (never opened). Its other metadata is not
        /// observed and `meta` is zeroed.
        fd: Option<OwnedFd>,
        pub(crate) meta: Meta,
    }

    impl Dir {
        /// Opens a canonical directory path without following the final
        /// component. (Ancestors are canonical at the call site and checked
        /// against recorded identities.)
        pub(crate) fn open_absolute(path: &Path) -> io::Result<Self> {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(sys::O_DIRECTORY | sys::O_NOFOLLOW)
                .open(path)?;
            let fd = OwnedFd::from(file);
            let meta = meta_of(fd.as_raw_fd())?;
            if meta.kind != Kind::Dir {
                return Err(io::Error::other("not a directory"));
            }
            Ok(Self { fd, meta })
        }

        /// Current metadata of this very object.
        pub(crate) fn current(&self) -> io::Result<Meta> {
            meta_of(self.fd.as_raw_fd())
        }

        pub(crate) fn chown(&self, uid: u32, gid: u32) -> io::Result<()> {
            // SAFETY: operates on the descriptor owned by `self`.
            check(unsafe { sys::fchown(self.fd.as_raw_fd(), uid, gid) }).map(drop)
        }

        pub(crate) fn chmod(&self, mode: u32) -> io::Result<()> {
            // SAFETY: operates on the descriptor owned by `self`.
            check(unsafe { sys::fchmod(self.fd.as_raw_fd(), mode as sys::ModeT) }).map(drop)
        }

        /// Entry names (without `.`/`..`) of exactly this directory object.
        pub(crate) fn names(&self) -> io::Result<Vec<CString>> {
            let duplicate = self.fd.try_clone()?;
            let raw = duplicate.into_raw_fd();
            // SAFETY: `raw` is an owned descriptor; on success the stream
            // owns it, on failure it is re-wrapped below.
            let stream = unsafe { sys::fdopendir(raw) };
            if stream.is_null() {
                let error = io::Error::last_os_error();
                // SAFETY: `fdopendir` failed, so `raw` is still ours.
                drop(unsafe { OwnedFd::from_raw_fd(raw) });
                return Err(error);
            }
            // SAFETY: `stream` is a live directory stream.
            unsafe { sys::rewinddir(stream) };
            let mut names = Vec::new();
            let result = loop {
                // SAFETY: errno is thread-local and writable.
                unsafe { *errno() = 0 };
                // SAFETY: `stream` is live until `closedir` below.
                let entry = unsafe { sys::readdir(stream) };
                if entry.is_null() {
                    // SAFETY: errno is thread-local.
                    let code = unsafe { *errno() };
                    break if code == 0 {
                        Ok(())
                    } else {
                        Err(io::Error::from_raw_os_error(code))
                    };
                }
                // SAFETY: `readdir` returned a valid dirent whose `d_name`
                // is NUL-terminated at the platform offset.
                let name = unsafe {
                    CStr::from_ptr(
                        entry
                            .cast::<u8>()
                            .add(sys::DIRENT_NAME_OFFSET)
                            .cast::<c_char>(),
                    )
                };
                if name.to_bytes() != b"." && name.to_bytes() != b".." {
                    names.push(name.to_owned());
                }
            };
            // SAFETY: closes the stream and its duplicated descriptor.
            unsafe { sys::closedir(stream) };
            result.map(|()| names)
        }

        /// Opens one child by single-component name, never following a
        /// symlink. Refuses a different device (mount boundary).
        pub(crate) fn open_child(&self, name: &CStr) -> io::Result<Node> {
            let flags = sys::O_RDONLY | sys::O_NOFOLLOW | sys::O_NONBLOCK | sys::O_CLOEXEC;
            match openat_raw(self.fd.as_raw_fd(), name, flags, 0) {
                Ok(fd) => {
                    let meta = meta_of(fd.as_raw_fd())?;
                    if meta.dev != self.meta.dev {
                        return Err(io::Error::other("crosses a filesystem boundary"));
                    }
                    Ok(Node {
                        name: name.to_owned(),
                        kind: meta.kind,
                        fd: Some(fd),
                        meta,
                    })
                }
                Err(error) if error.raw_os_error() == Some(sys::ELOOP) => Ok(Node {
                    name: name.to_owned(),
                    kind: Kind::Symlink,
                    fd: None,
                    meta: Meta {
                        kind: Kind::Symlink,
                        uid: 0,
                        gid: 0,
                        mode: 0,
                        nlink: 0,
                        dev: 0,
                        ino: 0,
                    },
                }),
                Err(error) => Err(error),
            }
        }

        fn unlink(&self, name: &CStr) -> io::Result<()> {
            // SAFETY: single-component name relative to our directory object.
            check(unsafe { sys::unlinkat(self.fd.as_raw_fd(), name.as_ptr(), 0) }).map(drop)
        }
    }

    impl Node {
        fn into_dir(self) -> Dir {
            Dir {
                fd: self.fd.expect("directories are opened"),
                meta: self.meta,
            }
        }

        /// Re-owns this very object: `fchown` on its descriptor, or for a
        /// symlink `fchownat(AT_SYMLINK_NOFOLLOW)` on its name inside the
        /// verified parent. The link target is never touched.
        pub(crate) fn chown(&self, parent: &Dir, uid: u32, gid: u32) -> io::Result<()> {
            match &self.fd {
                Some(fd) => {
                    // SAFETY: operates on a descriptor owned by `self`.
                    check(unsafe { sys::fchown(fd.as_raw_fd(), uid, gid) }).map(drop)
                }
                None => {
                    // SAFETY: single-component name, final component not
                    // followed, inside a directory we hold open.
                    check(unsafe {
                        sys::fchownat(
                            parent.fd.as_raw_fd(),
                            self.name.as_ptr(),
                            uid,
                            gid,
                            sys::AT_SYMLINK_NOFOLLOW,
                        )
                    })
                    .map(drop)
                }
            }
        }

        /// Sets the mode of this very object (never of a symlink).
        pub(crate) fn chmod(&self, mode: u32) -> io::Result<()> {
            let fd = self
                .fd
                .as_ref()
                .ok_or_else(|| io::Error::other("a symlink has no mode of its own"))?;
            // SAFETY: operates on a descriptor owned by `self`.
            check(unsafe { sys::fchmod(fd.as_raw_fd(), mode as sys::ModeT) }).map(drop)
        }
    }

    /// Which part of the custody a walk covers.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Tree {
        /// Git metadata: only directories and single-link regular files.
        Metadata,
        /// Worker payload: additionally symlinks (never followed).
        Payload,
    }

    /// Depth-first walk over descriptors. Every object is policy-checked as it
    /// is opened; `visit` mutates (or not) through the same opened object.
    /// `skip_top` names a top-level entry handled separately.
    pub(crate) fn walk(
        root: &Dir,
        tree: Tree,
        skip_top: Option<&CStr>,
        visit: &mut dyn FnMut(&Dir, &Node) -> io::Result<()>,
    ) -> Result<(), String> {
        walk_dir(root, tree, skip_top, visit, &mut PathBuf::new(), 0)
    }

    fn walk_dir(
        dir: &Dir,
        tree: Tree,
        skip_top: Option<&CStr>,
        visit: &mut dyn FnMut(&Dir, &Node) -> io::Result<()>,
        relative: &mut PathBuf,
        depth: usize,
    ) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!(
                "directory nesting exceeds {MAX_DEPTH} levels at {}",
                relative.display()
            ));
        }
        let names = dir
            .names()
            .map_err(|error| format!("list {}: {error}", relative.display()))?;
        for name in names {
            if depth == 0 && skip_top == Some(name.as_c_str()) {
                continue;
            }
            relative.push(OsStr::from_bytes(name.to_bytes()));
            let node = dir
                .open_child(&name)
                .map_err(|error| format!("{}: {error}", relative.display()))?;
            let refusal = match node.kind {
                Kind::File if node.meta.nlink > 1 => Some(format!(
                    "regular file has {} hard links (possible external alias)",
                    node.meta.nlink
                )),
                Kind::Symlink if tree == Tree::Metadata => {
                    Some("symlink in Git metadata is unsupported".to_owned())
                }
                Kind::Other => Some("special file is unsupported".to_owned()),
                _ => None,
            };
            if let Some(refusal) = refusal {
                return Err(format!("{}: {refusal}", relative.display()));
            }
            visit(dir, &node).map_err(|error| format!("{}: {error}", relative.display()))?;
            if node.kind == Kind::Dir {
                let child = node.into_dir();
                walk_dir(&child, tree, skip_top, visit, relative, depth + 1)?;
            }
            relative.pop();
        }
        Ok(())
    }

    /// Opened-directory identity `(device, inode)`.
    pub(crate) type Identity = (u64, u64);

    /// Identities of the two custody roots, recorded at admission.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct RootIds {
        pub(crate) common: Identity,
        pub(crate) worktree: Identity,
    }

    /// The two custody roots as stable directory objects.
    #[derive(Debug)]
    pub(crate) struct Roots {
        pub(crate) common: Dir,
        pub(crate) worktree: Dir,
    }

    impl Roots {
        /// Opens both canonical paths; with `expect`, refuses when either is no
        /// longer the directory object recorded at admission (replacement).
        pub(crate) fn open(
            common: &Path,
            worktree: &Path,
            expect: Option<RootIds>,
        ) -> Result<Self, String> {
            let open = |path: &Path, what: &str| {
                Dir::open_absolute(path)
                    .map_err(|error| format!("{what} {}: {error}", path.display()))
            };
            let roots = Self {
                common: open(common, "common directory")?,
                worktree: open(worktree, "worktree")?,
            };
            if let Some(expected) = expect
                && roots.ids() != expected
            {
                return Err("a custody root was replaced since admission".into());
            }
            Ok(roots)
        }

        pub(crate) fn ids(&self) -> RootIds {
            RootIds {
                common: (self.common.meta.dev, self.common.meta.ino),
                worktree: (self.worktree.meta.dev, self.worktree.meta.ino),
            }
        }
    }

    /// The worktree `.git` pointer must be a plain single-link regular file in
    /// the verified worktree directory (never a symlink or alias).
    fn check_pointer(worktree: &Dir) -> Result<(), String> {
        let node = worktree
            .open_child(c".git")
            .map_err(|error| format!(".git pointer: {error}"))?;
        match node.kind {
            Kind::File if node.meta.nlink == 1 => Ok(()),
            Kind::File => Err(".git pointer is a hard-linked file".into()),
            Kind::Symlink => Err(".git pointer is a symlink".into()),
            _ => Err(".git pointer is not a regular file".into()),
        }
    }

    /// Mount points strictly below a custody root (bind mounts included).
    /// Pure so it can be tested against fixture text.
    pub(crate) fn refuse_nested_mounts(mountinfo: &str, roots: &[&Path]) -> Result<(), String> {
        for line in mountinfo.lines() {
            let Some(raw) = line.split_whitespace().nth(4) else {
                continue;
            };
            let mount_point = PathBuf::from(OsString::from_vec(unescape_octal(raw)));
            for root in roots {
                if mount_point.starts_with(root) && mount_point != *root {
                    return Err(format!(
                        "mount point {} lies inside custody root {}",
                        mount_point.display(),
                        root.display()
                    ));
                }
            }
        }
        Ok(())
    }

    fn unescape_octal(text: &str) -> Vec<u8> {
        let bytes = text.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            let octal = bytes.get(index + 1..index + 4).filter(|digits| {
                bytes[index] == b'\\' && digits.iter().all(|byte| (b'0'..=b'7').contains(byte))
            });
            if let Some(digits) = octal {
                let value = digits
                    .iter()
                    .fold(0u32, |acc, digit| acc * 8 + u32::from(digit - b'0'));
                out.push(value as u8);
                index += 4;
            } else {
                out.push(bytes[index]);
                index += 1;
            }
        }
        out
    }

    /// Live mount-topology check (Linux). Unreadable topology refuses.
    fn mount_topology(roots: &[&Path]) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            let text = std::fs::read_to_string("/proc/self/mountinfo")
                .map_err(|error| format!("mount topology unverifiable: {error}"))?;
            refuse_nested_mounts(&text, roots)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = roots;
            Ok(())
        }
    }

    /// Read-only layout checks shared by admission (every platform) and the
    /// Linux custody transfer: root overlap, metadata tree, `.git` pointer,
    /// payload tree and mount topology. Nothing is modified.
    pub(crate) fn preflight_roots(
        roots: &Roots,
        common: &Path,
        worktree: &Path,
    ) -> Result<(), String> {
        if common.starts_with(worktree) || worktree.starts_with(common) {
            return Err("the worktree and the Git common directory overlap".into());
        }
        mount_topology(&[common, worktree])?;
        walk(&roots.common, Tree::Metadata, None, &mut |_, _| Ok(()))
            .map_err(|error| format!("Git metadata: {error}"))?;
        check_pointer(&roots.worktree)?;
        walk(
            &roots.worktree,
            Tree::Payload,
            Some(c".git"),
            &mut |_, _| Ok(()),
        )
        .map_err(|error| format!("worktree payload: {error}"))
    }

    /// Opens and checks the roots; returns their identities for the record.
    pub(crate) fn preflight_layout(common: &Path, worktree: &Path) -> Result<RootIds, String> {
        let roots = Roots::open(common, worktree, None)?;
        preflight_roots(&roots, common, worktree)?;
        Ok(roots.ids())
    }

    /// Controller and worker identities for the custody transfer.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct Ownership {
        pub(crate) controller_uid: u32,
        pub(crate) controller_gid: u32,
        pub(crate) worker_uid: u32,
        pub(crate) worker_gid: u32,
    }

    /// Transfers metadata to the controller (no group/other write), payload to
    /// the worker and the worktree root to `controller:worker 01770`, all
    /// through verified descriptors. Call only after [`preflight_roots`].
    pub(crate) fn acquire(roots: &Roots, own: &Ownership) -> Result<(), String> {
        let text = |error: io::Error| error.to_string();
        // The pointer is verified (and kept open) before the first change.
        let pointer = roots.worktree.open_child(c".git").map_err(text)?;
        if pointer.kind != Kind::File || pointer.meta.nlink != 1 {
            return Err(".git pointer is not a single-link regular file".into());
        }
        roots
            .common
            .chown(own.controller_uid, own.controller_gid)
            .map_err(text)?;
        roots
            .common
            .chmod(roots.common.meta.mode & !0o022)
            .map_err(text)?;
        walk(&roots.common, Tree::Metadata, None, &mut |parent, node| {
            node.chown(parent, own.controller_uid, own.controller_gid)?;
            node.chmod(node.meta.mode & !0o022)
        })?;
        walk(
            &roots.worktree,
            Tree::Payload,
            Some(c".git"),
            &mut |parent, node| node.chown(parent, own.worker_uid, own.worker_gid),
        )?;
        roots
            .worktree
            .chown(own.controller_uid, own.worker_gid)
            .map_err(text)?;
        roots.worktree.chmod(0o1770).map_err(text)?;
        pointer
            .chown(&roots.worktree, own.controller_uid, own.controller_gid)
            .map_err(text)?;
        pointer.chmod(0o644).map_err(text)
    }

    /// Returns both trees to the recorded original owner through verified
    /// descriptors and restores the worktree root mode; then proves the roots.
    /// Call only after [`preflight_roots`] on the same roots.
    pub(crate) fn restore(
        roots: &Roots,
        uid: u32,
        gid: u32,
        worktree_mode: u32,
    ) -> Result<(), String> {
        let text = |error: io::Error| error.to_string();
        roots.common.chown(uid, gid).map_err(text)?;
        walk(&roots.common, Tree::Metadata, None, &mut |parent, node| {
            node.chown(parent, uid, gid)
        })?;
        roots.worktree.chown(uid, gid).map_err(text)?;
        walk(&roots.worktree, Tree::Payload, None, &mut |parent, node| {
            node.chown(parent, uid, gid)
        })?;
        roots.worktree.chmod(worktree_mode).map_err(text)?;
        for (name, dir) in [
            ("common directory", &roots.common),
            ("worktree", &roots.worktree),
        ] {
            let now = dir.current().map_err(text)?;
            if (now.uid, now.gid) != (uid, gid) {
                return Err(format!("{name} was not restored"));
            }
        }
        Ok(())
    }

    /// Phase of an admitted custody, persisted in the marker.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Phase {
        /// Exclusively reserved; no ownership or mode has changed.
        Reserved,
        /// Ownership transfer began or completed.
        Custody,
        /// Restoration began.
        Releasing,
        /// Setup or release did not complete; nothing is free.
        Incomplete,
    }

    impl Phase {
        fn as_str(self) -> &'static str {
            match self {
                Self::Reserved => "reserved",
                Self::Custody => "custody",
                Self::Releasing => "releasing",
                Self::Incomplete => "incomplete",
            }
        }
    }

    /// The true original owner, recorded once before any mutation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct OriginalOwner {
        pub(crate) uid: u32,
        pub(crate) gid: u32,
        pub(crate) worktree_mode: u32,
    }

    #[derive(Debug, Clone)]
    struct Record {
        attempt_id: String,
        worktree: PathBuf,
        phase: Phase,
        worker_uid: Option<u32>,
        original: Option<OriginalOwner>,
        ids: Option<RootIds>,
    }

    impl Record {
        fn render(&self) -> String {
            let mut text = format!(
                "{FORMAT}\nattempt={}\nphase={}\nworktree={:?}\n",
                self.attempt_id,
                self.phase.as_str(),
                self.worktree
            );
            if let Some(uid) = self.worker_uid {
                text.push_str(&format!("worker_uid={uid}\n"));
            }
            if let Some(original) = self.original_lines() {
                text.push_str(&original);
            }
            if let Some(ids) = self.ids {
                text.push_str(&format!(
                    "common_identity={}:{}\nworktree_identity={}:{}\n",
                    ids.common.0, ids.common.1, ids.worktree.0, ids.worktree.1
                ));
            }
            text
        }

        fn original_lines(&self) -> Option<String> {
            self.original.map(|original| {
                format!(
                    "original_uid={}\noriginal_gid={}\noriginal_worktree_mode={:o}\n",
                    original.uid, original.gid, original.worktree_mode
                )
            })
        }
    }

    /// The PID namespace pin and the mount namespace identity record are
    /// installed by trusted host administration. A container-local PID 1
    /// cannot establish their provenance.
    fn production_registry() -> Result<Dir, String> {
        #[cfg(not(target_os = "linux"))]
        {
            Err("host custody registry requires Linux".into())
        }
        #[cfg(target_os = "linux")]
        {
            let root = Path::new("/var/lib/receipts/protected-custody");
            for path in root.ancestors() {
                let meta = std::fs::symlink_metadata(path)
                    .map_err(|e| format!("registry ancestor: {e}"))?;
                if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
                    return Err(format!("unsafe registry ancestor {}", path.display()));
                }
            }
            let dir = Dir::open_absolute(root).map_err(|e| e.to_string())?;
            if dir.meta.uid != 0 || dir.meta.mode != 0o700 {
                return Err("unsafe custody registry directory".into());
            }
            let lock = dir.open_child(REGISTRY_LOCK).map_err(|e| e.to_string())?;
            if lock.kind != Kind::File
                || lock.meta.uid != 0
                || lock.meta.mode != 0o600
                || lock.meta.nlink != 1
            {
                return Err("unsafe custody registry lock".into());
            }
            let topology =
                std::fs::read_to_string("/proc/self/mountinfo").map_err(|e| e.to_string())?;
            {
                let (anchor, namespace) = ("host-pid", "pid");
                let anchor_path = root.join(anchor);
                let found = topology.lines().any(|line| {
                    let Some((left, right)) = line.split_once(" - ") else {
                        return false;
                    };
                    let Some(mount) = left.split_whitespace().nth(4) else {
                        return false;
                    };
                    OsString::from_vec(unescape_octal(mount)) == anchor_path.as_os_str()
                        && right.split_whitespace().next() == Some("nsfs")
                });
                if !found {
                    return Err(format!(
                        "namespace anchor {anchor} is not a pinned nsfs mount"
                    ));
                }
                let name = cstring(OsStr::new(anchor)).map_err(|e| e.to_string())?;
                let fd = openat_raw(
                    dir.fd.as_raw_fd(),
                    &name,
                    sys::O_RDONLY | sys::O_NOFOLLOW | sys::O_CLOEXEC,
                    0,
                )
                .map_err(|e| e.to_string())?;
                let pinned = meta_of(fd.as_raw_fd()).map_err(|e| e.to_string())?;
                if pinned.uid != 0 {
                    return Err("unsafe namespace anchor owner".into());
                }
                for process in ["self", "1"] {
                    let actual = std::fs::metadata(format!("/proc/{process}/ns/{namespace}"))
                        .map_err(|e| e.to_string())?;
                    if (actual.dev(), actual.ino()) != (pinned.dev, pinned.ino) {
                        return Err(format!("{namespace} namespace differs from host anchor"));
                    }
                }
            }
            let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .map_err(|e| format!("boot identity: {e}"))?;
            let mnt = |process: &str| {
                std::fs::metadata(format!("/proc/{process}/ns/mnt"))
                    .map(|m| (m.dev(), m.ino()))
                    .map_err(|e| e.to_string())
            };
            admit_host_mnt(
                &dir,
                0,
                boot.strip_suffix('\n').unwrap_or(boot.as_str()),
                mnt("self")?,
                mnt("1")?,
            )?;
            refuse_alias_mounts(&[root])?;
            Ok(dir)
        }
    }

    const HOST_MNT_IDENTITY: &CStr = c"host-mnt.identity";
    const HOST_MNT_FORMAT: &str = "receipts-host-mnt-identity-v1";
    const HOST_MNT_LIMIT: u64 = 256;

    /// Admits the administrator's record of the host initial mount namespace:
    /// an administrative assertion, not a kernel handle (the kernel refuses an
    /// nsfs bind of a mount namespace inside that same namespace). It must be
    /// an `owner`-owned 0600 single-link regular file in the registry naming
    /// exactly this boot and the mount namespace of both this process and
    /// PID 1. Production passes owner 0 and kernel observations only.
    pub(crate) fn admit_host_mnt(
        dir: &Dir,
        owner: u32,
        boot: &str,
        current: Identity,
        init: Identity,
    ) -> Result<(), String> {
        let node = dir
            .open_child(HOST_MNT_IDENTITY)
            .map_err(|e| format!("host mount identity record: {e}"))?;
        if node.kind != Kind::File
            || node.meta.uid != owner
            || node.meta.mode != 0o600
            || node.meta.nlink != 1
        {
            return Err("unsafe host mount identity record".into());
        }
        let file = File::from(node.fd.ok_or("host mount identity record unavailable")?);
        let mut bytes = Vec::new();
        (&file)
            .take(HOST_MNT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("host mount identity record: {e}"))?;
        let (recorded_boot, namespace) = std::str::from_utf8(&bytes)
            .ok()
            .filter(|_| bytes.len() as u64 <= HOST_MNT_LIMIT)
            .and_then(parse_host_mnt)
            .ok_or("malformed host mount identity record")?;
        if recorded_boot != boot {
            return Err("host mount identity record is from another boot".into());
        }
        if namespace != current || namespace != init {
            return Err("mnt namespace differs from host identity record".into());
        }
        Ok(())
    }

    /// `FORMAT\nboot_id=<uuid>\nmnt_namespace=<dev>:<ino>\nend\n`, exactly.
    fn parse_host_mnt(text: &str) -> Option<(&str, Identity)> {
        let rest = text
            .strip_prefix(HOST_MNT_FORMAT)?
            .strip_prefix("\nboot_id=")?;
        let (boot, rest) = rest.split_once("\nmnt_namespace=")?;
        let (namespace, rest) = rest.split_once('\n')?;
        let (dev, ino) = namespace.split_once(':')?;
        let number = |t: &str| {
            (!t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
                .then(|| t.parse::<u64>().ok())
                .flatten()
        };
        let uuid = boot.len() == 36
            && boot.bytes().enumerate().all(|(i, b)| match i {
                8 | 13 | 18 | 23 => b == b'-',
                _ => matches!(b, b'0'..=b'9' | b'a'..=b'f'),
            });
        (rest == "end\n" && uuid).then_some((boot, (number(dev)?, number(ino)?)))
    }

    #[cfg(target_os = "linux")]
    fn refuse_alias_mounts(roots: &[&Path]) -> Result<(), String> {
        let text = std::fs::read_to_string("/proc/self/mountinfo").map_err(|e| e.to_string())?;
        refuse_alias_mounts_text(&text, roots)
    }

    pub(crate) fn refuse_alias_mounts_text(text: &str, roots: &[&Path]) -> Result<(), String> {
        let mut mounts = Vec::new();
        for line in text.lines() {
            let (left, right) = line.split_once(" - ").ok_or("malformed mountinfo")?;
            let filesystem = right
                .split_whitespace()
                .next()
                .ok_or("missing mount filesystem")?;
            let fields: Vec<_> = left.split_whitespace().collect();
            if fields.len() < 6 {
                return Err("malformed mountinfo".into());
            }
            let mount = PathBuf::from(OsString::from_vec(unescape_octal(fields[4])));
            let mount_root = PathBuf::from(OsString::from_vec(unescape_octal(fields[3])));
            mounts.push((fields[2].to_owned(), mount_root, mount, filesystem));
        }
        for root in roots {
            let containing = mounts
                .iter()
                .filter(|(_, _, mount, _)| root.starts_with(mount))
                .max_by_key(|(_, _, mount, _)| mount.components().count())
                .ok_or("mount topology missing root")?;
            if !matches!(containing.3, "ext4" | "xfs" | "btrfs" | "tmpfs")
                || containing.1 != Path::new("/")
                || mounts
                    .iter()
                    .filter(|(dev, _, _, _)| *dev == containing.0)
                    .count()
                    != 1
            {
                return Err(format!(
                    "aliasable or unverifiable mount topology at {}",
                    root.display()
                ));
            }
            if mounts.iter().any(|(_, _, mount, filesystem)| {
                let host_anchor = *root == Path::new("/var/lib/receipts/protected-custody")
                    && *filesystem == "nsfs"
                    && *mount == root.join("host-pid");
                mount.starts_with(root) && mount != *root && !host_anchor
            }) {
                return Err(format!("nested mount at {}", root.display()));
            }
        }
        Ok(())
    }

    const REGISTRY_LOCK: &CStr = c"lock";
    const REGISTRY_FORMAT: &str = "receipts-registry-v1";

    // Both roots are compared against the other's complete ancestor chain.
    fn chain(path: &Path) -> Result<Vec<Identity>, String> {
        let canonical = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        if canonical != path {
            return Err("custody path is not canonical".into());
        }
        path.ancestors()
            .map(|p| {
                let dir = Dir::open_absolute(p).map_err(|e| e.to_string())?;
                Ok((dir.meta.dev, dir.meta.ino))
            })
            .collect()
    }

    fn chain_text(chain: &[Identity]) -> String {
        chain
            .iter()
            .map(|(dev, ino)| format!("{dev}:{ino}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn parse_chain(text: &str) -> Result<Vec<Identity>, String> {
        let ids: Result<Vec<_>, _> = text
            .split(',')
            .map(|pair| {
                let (dev, ino) = pair.split_once(':').ok_or("invalid registry identity")?;
                Ok::<Identity, &str>((
                    dev.parse().map_err(|_| "invalid registry device")?,
                    ino.parse().map_err(|_| "invalid registry inode")?,
                ))
            })
            .collect();
        let ids = ids?;
        if ids.is_empty() {
            return Err("empty registry chain".into());
        }
        Ok(ids)
    }

    fn overlap(a: &[Identity], b: &[Identity]) -> bool {
        a.contains(&b[0]) || b.contains(&a[0])
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn valid_hex(text: &str) -> bool {
        !text.is_empty()
            && text.len().is_multiple_of(2)
            && text.bytes().all(|b| b.is_ascii_hexdigit())
    }

    fn decode_hex(text: &str) -> Option<Vec<u8>> {
        if !valid_hex(text) {
            return None;
        }
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digit = |b: u8| (b as char).to_digit(16);
                Some((digit(pair[0])? * 16 + digit(pair[1])?) as u8)
            })
            .collect()
    }

    fn valid_path(text: &str, chain: &[Identity]) -> bool {
        let Some(bytes) = decode_hex(text) else {
            return false;
        };
        let path = PathBuf::from(OsString::from_vec(bytes));
        path.is_absolute() && path.ancestors().count() == chain.len()
    }

    fn valid_detail(detail: &str, phase: &str, name: &CStr) -> bool {
        let Some(bytes) = decode_hex(detail) else {
            return false;
        };
        let Ok(detail) = std::str::from_utf8(&bytes) else {
            return false;
        };
        let mut lines = detail.lines();
        let attempt = name.to_string_lossy();
        if lines.next() != Some(FORMAT)
            || lines.next() != Some(format!("attempt={}", &attempt[2..]).as_str())
            || lines.next() != Some(format!("phase={phase}").as_str())
            || !lines
                .next()
                .is_some_and(|line| line.starts_with("worktree="))
        {
            return false;
        }
        let fields: Vec<_> = lines.collect();
        if fields.is_empty() {
            return phase != "custody";
        }
        if fields.len() != 6 {
            return false;
        }
        let number = |field: &str, key: &str| {
            field
                .strip_prefix(key)
                .and_then(|value| value.parse::<u32>().ok())
                .is_some()
        };
        let identity = |field: &str, key: &str| {
            field
                .strip_prefix(key)
                .and_then(|value| parse_chain(value).ok())
                .is_some_and(|ids| ids.len() == 1)
        };
        number(fields[0], "worker_uid=")
            && number(fields[1], "original_uid=")
            && number(fields[2], "original_gid=")
            && fields[3]
                .strip_prefix("original_worktree_mode=")
                .and_then(|v| u32::from_str_radix(v, 8).ok())
                .is_some()
            && identity(fields[4], "common_identity=")
            && identity(fields[5], "worktree_identity=")
    }

    #[derive(Debug)]
    struct Registry {
        dir: Dir,
        lock: File,
        name: CString,
        common: Vec<Identity>,
        worktree: Vec<Identity>,
        common_path: PathBuf,
        worktree_path: PathBuf,
        local_lock: Mutex<()>,
        production: bool,
    }

    struct RegistryGuard<'a> {
        file: &'a File,
        _local: MutexGuard<'a, ()>,
    }
    impl Drop for RegistryGuard<'_> {
        fn drop(&mut self) {
            // SAFETY: the file remains open throughout the guard's lifetime.
            unsafe { sys::flock(self.file.as_raw_fd(), sys::LOCK_UN) };
        }
    }

    impl Registry {
        fn guard(&self) -> Result<RegistryGuard<'_>, String> {
            let local = self
                .local_lock
                .lock()
                .map_err(|_| "registry lock poisoned")?;
            // SAFETY: the descriptor refers to the stable coordination lock.
            check(unsafe { sys::flock(self.lock.as_raw_fd(), sys::LOCK_EX) })
                .map_err(|e| format!("registry lock: {e}"))?;
            let guard = RegistryGuard {
                file: &self.lock,
                _local: local,
            };
            if self.production {
                let anchor = production_registry()?;
                if (anchor.meta.dev, anchor.meta.ino) != (self.dir.meta.dev, self.dir.meta.ino) {
                    return Err("custody registry anchor was replaced".into());
                }
            }
            let current = self
                .dir
                .open_child(REGISTRY_LOCK)
                .map_err(|e| e.to_string())?;
            let held = meta_of(self.lock.as_raw_fd()).map_err(|e| e.to_string())?;
            if current.kind != Kind::File || current.meta != held {
                return Err("registry lock was replaced".into());
            }
            Ok(guard)
        }

        fn begin(
            dir: Dir,
            common: &Path,
            worktree: &Path,
            record: &Record,
            production: bool,
        ) -> Result<Self, String> {
            let attempt = &record.attempt_id;
            if attempt.is_empty()
                || !attempt
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err("invalid custody attempt".into());
            }
            let node = dir
                .open_child(REGISTRY_LOCK)
                .map_err(|e| format!("registry lock: {e}"))?;
            if node.kind != Kind::File || node.meta.nlink != 1 {
                return Err("invalid registry lock".into());
            }
            let lock = File::from(node.fd.ok_or("registry lock unavailable")?);
            let name = CString::new(format!("r-{attempt}")).map_err(|e| e.to_string())?;
            let registry = Self {
                dir,
                lock,
                name,
                common: chain(common)?,
                worktree: chain(worktree)?,
                common_path: common.to_path_buf(),
                worktree_path: worktree.to_path_buf(),
                local_lock: Mutex::new(()),
                production,
            };
            if overlap(&registry.common, &registry.worktree) {
                return Err("the worktree and the Git common directory overlap".into());
            }
            {
                let _guard = registry.guard()?;
                if chain(common)? != registry.common || chain(worktree)? != registry.worktree {
                    return Err("custody root or ancestor changed during reservation".into());
                }
                for name in registry.dir.names().map_err(|e| e.to_string())? {
                    if name == REGISTRY_LOCK || name == c"host-pid" || name == HOST_MNT_IDENTITY {
                        continue;
                    }
                    if !name.to_bytes().starts_with(b"r-") {
                        return Err("unknown custody registry entry".into());
                    }
                    let node = registry.dir.open_child(&name).map_err(|e| e.to_string())?;
                    if node.kind != Kind::File || node.meta.nlink != 1 {
                        return Err("invalid custody registry record".into());
                    }
                    let file = File::from(node.fd.ok_or("invalid custody registry record")?);
                    let metadata = file.metadata().map_err(|e| e.to_string())?;
                    if metadata.len() > 65536 {
                        return Err("oversized custody registry record".into());
                    }
                    let mut bytes = vec![0; metadata.len() as usize];
                    file.read_exact_at(&mut bytes, 0)
                        .map_err(|e| e.to_string())?;
                    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
                    let mut lines = text.lines();
                    if lines.next() != Some(REGISTRY_FORMAT) {
                        return Err("malformed custody registry record".into());
                    }
                    let a = parse_chain(lines.next().ok_or("missing common chain")?)?;
                    let b = parse_chain(lines.next().ok_or("missing worktree chain")?)?;
                    let common_path = lines.next().ok_or("missing registry common path")?;
                    let worktree_path = lines.next().ok_or("missing registry worktree path")?;
                    let phase = lines.next().ok_or("missing registry phase")?;
                    let detail = lines.next().ok_or("missing registry detail")?;
                    let end = lines.next().ok_or("missing registry terminator")?;
                    if !valid_path(common_path, &a)
                        || !valid_path(worktree_path, &b)
                        || !valid_detail(detail, phase, &name)
                        || !matches!(phase, "reserved" | "custody" | "releasing" | "incomplete")
                        || end != "end"
                        || !text.ends_with("end\n")
                        || lines.next().is_some()
                    {
                        return Err("malformed custody registry record".into());
                    }
                    let retained_paths = [common_path, worktree_path].map(|text| {
                        PathBuf::from(OsString::from_vec(
                            decode_hex(text).expect("validated path"),
                        ))
                    });
                    let path_overlap =
                        [&registry.common_path, &registry.worktree_path]
                            .iter()
                            .any(|ours| {
                                retained_paths
                                    .iter()
                                    .any(|other| ours.starts_with(other) || other.starts_with(ours))
                            });
                    if path_overlap
                        || [&registry.common, &registry.worktree]
                            .iter()
                            .any(|ours| overlap(ours, &a) || overlap(ours, &b))
                    {
                        if registry.common[0] == a[0] {
                            let common_dir =
                                Dir::open_absolute(common).map_err(|e| e.to_string())?;
                            return Err(Reservation::diagnose(&common_dir));
                        }
                        return Err(format!(
                            "custody resources overlap retained attempt {}, phase {}",
                            String::from_utf8_lossy(&name.to_bytes()[2..]),
                            phase
                        ));
                    }
                }
                if chain_text(&registry.common).len()
                    + chain_text(&registry.worktree).len()
                    + 2 * (common.as_os_str().as_bytes().len()
                        + worktree.as_os_str().as_bytes().len()
                        + record.render().len()
                        + 256)
                    > 65536
                {
                    return Err("custody registry record would exceed its supported size".into());
                }
                let fd = openat_raw(
                    registry.dir.fd.as_raw_fd(),
                    &registry.name,
                    sys::O_RDWR | sys::O_CREAT | sys::O_EXCL | sys::O_NOFOLLOW | sys::O_CLOEXEC,
                    0o600,
                )
                .map_err(|e| format!("create registry reservation: {e}"))?;
                let mut file = File::from(fd);
                write!(
                    file,
                    "{REGISTRY_FORMAT}\n{}\n{}\n{}\n{}\nreserved\n{}\nend\n",
                    chain_text(&registry.common),
                    chain_text(&registry.worktree),
                    hex(common.as_os_str().as_bytes()),
                    hex(worktree.as_os_str().as_bytes()),
                    hex(record.render().as_bytes())
                )
                .and_then(|()| file.sync_all())
                .map_err(|e| format!("persist registry reservation: {e}"))?;
                registry
                    .dir
                    .fd
                    .try_clone()
                    .map(File::from)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| e.to_string())?;
            }
            Ok(registry)
        }

        fn update(
            &self,
            marker_dir: &Dir,
            marker_file: &File,
            previous: &Record,
            record: &Record,
        ) -> io::Result<()> {
            let _guard = self.guard().map_err(io::Error::other)?;
            self.verify_held(marker_dir, marker_file, previous)
                .map_err(io::Error::other)?;
            let fd = openat_raw(
                self.dir.fd.as_raw_fd(),
                &self.name,
                sys::O_RDWR | sys::O_NOFOLLOW | sys::O_CLOEXEC,
                0,
            )?;
            let meta = meta_of(fd.as_raw_fd())?;
            if meta.kind != Kind::File || meta.nlink != 1 {
                return Err(io::Error::other("invalid owned registry record"));
            }
            let file = File::from(fd);
            let text = format!(
                "{REGISTRY_FORMAT}\n{}\n{}\n{}\n{}\n{}\n{}\nend\n",
                chain_text(&self.common),
                chain_text(&self.worktree),
                hex(self.common_path.as_os_str().as_bytes()),
                hex(self.worktree_path.as_os_str().as_bytes()),
                record.phase.as_str(),
                hex(record.render().as_bytes())
            );
            file.set_len(0)?;
            file.write_all_at(text.as_bytes(), 0)?;
            file.sync_all()
        }

        fn verify_held(
            &self,
            marker_dir: &Dir,
            marker_file: &File,
            record: &Record,
        ) -> Result<(), String> {
            let registry_node = self.dir.open_child(&self.name).map_err(|e| e.to_string())?;
            if registry_node.kind != Kind::File || registry_node.meta.nlink != 1 {
                return Err("invalid owned registry record".into());
            }
            let registry_file = File::from(registry_node.fd.ok_or("missing registry record")?);
            let expected = format!(
                "{REGISTRY_FORMAT}\n{}\n{}\n{}\n{}\n{}\n{}\nend\n",
                chain_text(&self.common),
                chain_text(&self.worktree),
                hex(self.common_path.as_os_str().as_bytes()),
                hex(self.worktree_path.as_os_str().as_bytes()),
                record.phase.as_str(),
                hex(record.render().as_bytes())
            );
            let mut bytes = vec![0; expected.len()];
            if registry_file.metadata().map_err(|e| e.to_string())?.len() != bytes.len() as u64
                || registry_file.read_exact_at(&mut bytes, 0).is_err()
                || bytes != expected.as_bytes()
            {
                return Err("owned registry record changed; retaining quarantine".into());
            }
            let marker = marker_dir.open_child(MARKER).map_err(|e| e.to_string())?;
            let held = meta_of(marker_file.as_raw_fd()).map_err(|e| e.to_string())?;
            if marker.kind != Kind::File
                || marker.meta != held
                || read_marker(marker_file) != record.render()
            {
                return Err("custody marker changed; retaining quarantine".into());
            }
            Ok(())
        }

        fn revalidate(
            &self,
            marker_dir: &Dir,
            marker_file: &File,
            record: &Record,
        ) -> Result<(), String> {
            let _guard = self.guard()?;
            if chain(&self.common_path)? != self.common
                || chain(&self.worktree_path)? != self.worktree
            {
                return Err("custody root or ancestor was replaced".into());
            }
            self.verify_held(marker_dir, marker_file, record)
        }

        fn remove(
            &self,
            marker_dir: &Dir,
            marker_file: &File,
            record: &Record,
        ) -> Result<(), String> {
            let _guard = self.guard()?;
            #[cfg(target_os = "linux")]
            if self.production {
                refuse_alias_mounts(&[&self.common_path, &self.worktree_path])?;
            }
            self.verify_held(marker_dir, marker_file, record)?;
            marker_dir.unlink(MARKER).map_err(|e| e.to_string())?;
            marker_dir
                .fd
                .try_clone()
                .map(File::from)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
            self.dir.unlink(&self.name).map_err(|e| e.to_string())?;
            self.dir
                .fd
                .try_clone()
                .map(File::from)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())
        }
    }

    /// Exclusive, attributable reservation of one Git common directory.
    #[derive(Debug)]
    pub(crate) struct Reservation {
        dir: Dir,
        file: File,
        record: Mutex<Record>,
        registry: Registry,
        uncertain: AtomicBool,
    }

    fn write_record(file: &File, record: &Record) -> io::Result<()> {
        let text = record.render();
        file.set_len(0)?;
        file.write_all_at(text.as_bytes(), 0)?;
        file.sync_data()
    }

    fn read_marker(file: &File) -> String {
        let mut buffer = vec![0u8; MARKER_READ_LIMIT];
        let read = file.read_at(&mut buffer, 0).unwrap_or(0);
        String::from_utf8_lossy(&buffer[..read]).into_owned()
    }

    fn marker_field<'a>(text: &'a str, key: &str) -> &'a str {
        text.lines()
            .find_map(|line| {
                line.strip_prefix(key)
                    .and_then(|rest| rest.strip_prefix('='))
            })
            .unwrap_or("unknown")
    }

    fn lock_exclusive(file: &File) -> io::Result<()> {
        // SAFETY: `file` owns an open descriptor.
        check(unsafe { sys::flock(file.as_raw_fd(), sys::LOCK_EX | sys::LOCK_NB) }).map(drop)
    }

    impl Reservation {
        /// Reserves the actual common directory for one admission, or refuses
        /// without changing anything.
        pub(crate) fn begin(
            common: &Path,
            attempt_id: &str,
            worktree: &Path,
        ) -> Result<Self, String> {
            let registry_dir = production_registry()?;
            #[cfg(target_os = "linux")]
            refuse_alias_mounts(&[common, worktree])?;
            let anchor_chain = chain(Path::new("/var/lib/receipts/protected-custody"))?;
            if overlap(&chain(common)?, &anchor_chain) || overlap(&chain(worktree)?, &anchor_chain)
            {
                return Err("custody roots overlap the host registry".into());
            }
            Self::begin_at(registry_dir, common, attempt_id, worktree, true)
        }

        #[cfg(test)]
        pub(crate) fn begin_for_test(
            registry: &Path,
            common: &Path,
            attempt_id: &str,
            worktree: &Path,
        ) -> Result<Self, String> {
            let dir = Dir::open_absolute(registry).map_err(|e| e.to_string())?;
            Self::begin_at(dir, common, attempt_id, worktree, false)
        }

        fn begin_at(
            registry_dir: Dir,
            common: &Path,
            attempt_id: &str,
            worktree: &Path,
            production: bool,
        ) -> Result<Self, String> {
            let record = Record {
                attempt_id: attempt_id.to_owned(),
                worktree: worktree.to_path_buf(),
                phase: Phase::Reserved,
                worker_uid: None,
                original: None,
                ids: None,
            };
            if record.render().len() + 256 > MARKER_READ_LIMIT {
                return Err("custody marker would exceed its supported size".into());
            }
            let registry = Registry::begin(registry_dir, common, worktree, &record, production)?;
            let dir = Dir::open_absolute(common)
                .map_err(|error| format!("common directory {}: {error}", common.display()))?;
            let flags = sys::O_RDWR | sys::O_CREAT | sys::O_EXCL | sys::O_NOFOLLOW | sys::O_CLOEXEC;
            #[cfg(test)]
            crate::protected::custody_event_for_test("registry record written".into());
            match openat_raw(dir.fd.as_raw_fd(), MARKER, flags, 0o600) {
                Ok(fd) => {
                    let file = File::from(fd);
                    #[cfg(test)]
                    crate::protected::custody_event_for_test("marker created".into());
                    let started = lock_exclusive(&file)
                        .and_then(|()| write_record(&file, &record))
                        .map_err(|error| error.to_string());
                    #[cfg(test)]
                    crate::protected::custody_event_for_test(format!(
                        "marker lock+record: {started:?}"
                    ));
                    if let Err(error) = started {
                        return Err(format!(
                            "custody reservation could not be started; registry retained: {error}"
                        ));
                    }
                    Ok(Self {
                        dir,
                        file,
                        record: Mutex::new(record),
                        registry,
                        uncertain: AtomicBool::new(false),
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    #[cfg(test)]
                    crate::protected::custody_event_for_test("marker exists".into());
                    Err(Self::diagnose(&dir))
                }
                Err(error) => Err(format!("custody reservation: {error}")),
            }
        }

        /// Explains an existing marker without touching it.
        fn diagnose(dir: &Dir) -> String {
            let flags = sys::O_RDONLY | sys::O_NOFOLLOW | sys::O_NONBLOCK | sys::O_CLOEXEC;
            let Ok(fd) = openat_raw(dir.fd.as_raw_fd(), MARKER, flags, 0) else {
                return "custody reservation exists but is unreadable; refusing".into();
            };
            let file = File::from(fd);
            let text = read_marker(&file);
            // An owner creates the marker, then locks it, then writes its
            // record. Probing the lock of an empty marker could take it inside
            // that window and refuse the only valid owner, so it is not probed.
            let liveness = if text.is_empty() {
                "owner starting or stopped before recording state"
            } else {
                match lock_exclusive(&file) {
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => "owner alive",
                    _ => "no live owner: retained state",
                }
            };
            // Runs while this probe still holds `file` (and any lock it took).
            #[cfg(test)]
            crate::protected::custody_event_for_test(format!("probe marker {text:?}: {liveness}"));
            format!(
                "Git common directory is already under custody (attempt {}, phase {}, {}); \
                 occupied or unknown prior custody refuses without recovery",
                marker_field(&text, "attempt"),
                marker_field(&text, "phase"),
                liveness
            )
        }

        fn update(&self, change: impl FnOnce(&mut Record)) -> io::Result<()> {
            let mut record = self
                .record
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut next = record.clone();
            change(&mut next);
            if next.phase == Phase::Custody
                && (next.worker_uid.is_none() || next.original.is_none() || next.ids.is_none())
            {
                return Err(io::Error::other(
                    "custody identity and original owner are required",
                ));
            }
            if let Err(error) = self
                .registry
                .update(&self.dir, &self.file, &record, &next)
                .and_then(|()| write_record(&self.file, &next))
            {
                self.uncertain.store(true, Ordering::SeqCst);
                return Err(error);
            }
            *record = next;
            Ok(())
        }

        /// Records the identity and the true original owner before any mutation.
        pub(crate) fn record_identity(
            &self,
            worker_uid: u32,
            original: OriginalOwner,
            ids: RootIds,
        ) -> io::Result<()> {
            if ids.common != self.registry.common[0] || ids.worktree != self.registry.worktree[0] {
                return Err(io::Error::other(
                    "custody roots differ from registry reservation",
                ));
            }
            self.update(|record| {
                record.worker_uid = Some(worker_uid);
                record.original = Some(original);
                record.ids = Some(ids);
            })
        }

        pub(crate) fn set_phase(&self, phase: Phase) -> io::Result<()> {
            self.update(|record| record.phase = phase)
        }

        pub(crate) fn ids(&self) -> Option<RootIds> {
            self.record
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .ids
        }

        pub(crate) fn revalidate(&self) -> Result<(), String> {
            let record = self
                .record
                .lock()
                .map_err(|_| "custody record lock poisoned")?;
            self.registry.revalidate(&self.dir, &self.file, &record)?;
            #[cfg(target_os = "linux")]
            if self.registry.production {
                refuse_alias_mounts(&[&self.registry.common_path, &self.registry.worktree_path])?;
            }
            Ok(())
        }

        /// The persisted evidence must still name this attempt and this
        /// original owner (never trusting in-memory state alone).
        pub(crate) fn verify_original(&self, original: OriginalOwner) -> Result<(), String> {
            let on_disk = read_marker(&self.file);
            let record = self
                .record
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let expected = Record {
                original: Some(original),
                ..record.clone()
            };
            let lines = expected.original_lines().unwrap_or_default();
            let attempt = format!("attempt={}", record.attempt_id);
            if !on_disk.lines().any(|line| line == attempt)
                || !lines
                    .lines()
                    .all(|line| on_disk.lines().any(|seen| seen == line))
            {
                return Err("persisted original-owner evidence does not match".into());
            }
            Ok(())
        }

        /// Deletes the marker. Only for a refused setup that mutated nothing,
        /// or a verified complete release.
        pub(crate) fn finish(&self) -> Result<(), String> {
            if self.uncertain.load(Ordering::SeqCst) {
                return Err("uncertain custody record remains quarantined".into());
            }
            let record = self
                .record
                .lock()
                .map_err(|_| "custody record lock poisoned")?;
            self.registry.remove(&self.dir, &self.file, &record)
        }
    }
}

#[cfg(any(
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    all(target_os = "macos", target_arch = "aarch64")
))]
pub(crate) use imp::*;

/// Platforms without a descriptor-relative implementation refuse.
#[cfg(not(any(
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    all(target_os = "macos", target_arch = "aarch64")
)))]
mod stub {
    use std::path::Path;

    const REFUSAL: &str = "custody reservation is unsupported on this platform";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct RootIds;
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct OriginalOwner {
        pub(crate) uid: u32,
        pub(crate) gid: u32,
        pub(crate) worktree_mode: u32,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Phase {
        Reserved,
        Custody,
        Releasing,
        Incomplete,
    }
    #[derive(Debug)]
    pub(crate) struct Reservation;

    impl Reservation {
        pub(crate) fn begin(_: &Path, _: &str, _: &Path) -> Result<Self, String> {
            Err(REFUSAL.into())
        }
        pub(crate) fn record_identity(
            &self,
            _: u32,
            _: OriginalOwner,
            _: RootIds,
        ) -> std::io::Result<()> {
            Err(std::io::Error::other(REFUSAL))
        }
        pub(crate) fn set_phase(&self, _: Phase) -> std::io::Result<()> {
            Err(std::io::Error::other(REFUSAL))
        }
        pub(crate) fn verify_original(&self, _: OriginalOwner) -> Result<(), String> {
            Err(REFUSAL.into())
        }
        pub(crate) fn revalidate(&self) -> Result<(), String> {
            Err(REFUSAL.into())
        }
        pub(crate) fn finish(&self) -> Result<(), String> {
            Err(REFUSAL.into())
        }
    }

    pub(crate) fn preflight_layout(_: &Path, _: &Path) -> Result<RootIds, String> {
        Err(REFUSAL.into())
    }
}

#[cfg(not(any(
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    all(target_os = "macos", target_arch = "aarch64")
)))]
pub(crate) use stub::*;

#[cfg(all(
    test,
    any(
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
        all(target_os = "macos", target_arch = "aarch64")
    )
))]
#[path = "protected_custody_tests.rs"]
mod tests;
