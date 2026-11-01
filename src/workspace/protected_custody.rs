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
    use std::mem::ManuallyDrop;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

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

    /// Exclusive, attributable reservation of one Git common directory.
    #[derive(Debug)]
    pub(crate) struct Reservation {
        dir: Dir,
        file: File,
        record: Mutex<Record>,
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
            let dir = Dir::open_absolute(common)
                .map_err(|error| format!("common directory {}: {error}", common.display()))?;
            let flags = sys::O_RDWR | sys::O_CREAT | sys::O_EXCL | sys::O_NOFOLLOW | sys::O_CLOEXEC;
            match openat_raw(dir.fd.as_raw_fd(), MARKER, flags, 0o600) {
                Ok(fd) => {
                    let file = File::from(fd);
                    let record = Record {
                        attempt_id: attempt_id.to_owned(),
                        worktree: worktree.to_path_buf(),
                        phase: Phase::Reserved,
                        worker_uid: None,
                        original: None,
                        ids: None,
                    };
                    let started = lock_exclusive(&file)
                        .and_then(|()| write_record(&file, &record))
                        .map_err(|error| error.to_string());
                    if let Err(error) = started {
                        let _ = dir.unlink(MARKER);
                        return Err(format!("custody reservation could not be started: {error}"));
                    }
                    Ok(Self {
                        dir,
                        file,
                        record: Mutex::new(record),
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
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
            let live = match lock_exclusive(&file) {
                Ok(()) => false,
                Err(error) => error.kind() == io::ErrorKind::WouldBlock,
            };
            format!(
                "Git common directory is already under custody (attempt {}, phase {}, {}); \
                 occupied or unknown prior custody refuses without recovery",
                marker_field(&text, "attempt"),
                marker_field(&text, "phase"),
                if live {
                    "owner alive"
                } else {
                    "no live owner: retained state"
                }
            )
        }

        fn update(&self, change: impl FnOnce(&mut Record)) -> io::Result<()> {
            let mut record = self
                .record
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut next = record.clone();
            change(&mut next);
            write_record(&self.file, &next)?;
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
            self.dir
                .unlink(MARKER)
                .map_err(|error| format!("custody reservation could not be removed: {error}"))
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
