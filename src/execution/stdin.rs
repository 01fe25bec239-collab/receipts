//! Private nonblocking delivery, advanced by the existing execution monitor.
//! No writer thread, callback, or public pipe authority exists.

use std::io::{ErrorKind, PipeWriter, Write};
use std::os::fd::{AsRawFd, IntoRawFd};
use std::process::Command;

use super::{BoundedStdinBytes, ExecutionError, ProcessStdin};

pub(super) struct StdinDelivery {
    pipe: Option<PipeWriter>,
    bytes: Option<BoundedStdinBytes>,
    offset: usize,
    #[cfg(test)]
    fault: u8,
    #[cfg(test)]
    tracked: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
}

impl StdinDelivery {
    pub(super) fn prepare(
        command: &mut Command,
        stdin: &ProcessStdin,
    ) -> Result<Self, ExecutionError> {
        let mut delivery = Self {
            pipe: None,
            bytes: None,
            offset: 0,
            #[cfg(test)]
            fault: FAULT.replace(0),
            #[cfg(test)]
            tracked: TRACKER.take(),
        };
        #[cfg(test)]
        if let Some(tracker) = &delivery.tracked {
            tracker.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        if let ProcessStdin::Bytes(bytes) = stdin {
            // Use exactly the supported process-control platforms, before spawning.
            super::runner::ensure_timeout_platform_supported()?;
            #[cfg(test)]
            match delivery.fault {
                1 => {
                    return Err(ExecutionError::StdinPipeCreationFailed {
                        kind: ErrorKind::Other,
                    });
                }
                2 => {
                    return Err(ExecutionError::StdinConfigurationFailed {
                        kind: ErrorKind::Other,
                    });
                }
                _ => {}
            }
            let (reader, writer) = std::io::pipe()
                .map_err(|e| ExecutionError::StdinPipeCreationFailed { kind: e.kind() })?;
            nonblocking(&writer)
                .map_err(|e| ExecutionError::StdinConfigurationFailed { kind: e.kind() })?;
            command.stdin(reader);
            delivery.pipe = Some(writer);
            delivery.bytes = Some(bytes.clone());
        }
        Ok(delivery)
    }

    pub(super) fn complete(&self) -> bool {
        self.pipe.is_none()
    }

    /// At most one nonblocking write per turn. Interrupted/WouldBlock return to
    /// the lifecycle monitor, so even repeated interruptions cannot hide a deadline.
    pub(super) fn advance(&mut self) -> Result<bool, ExecutionError> {
        let Some(pipe) = &mut self.pipe else {
            return Ok(true);
        };
        #[cfg(test)]
        if self.fault == 3 {
            return Err(ExecutionError::StdinWriteFailed {
                kind: ErrorKind::Other,
            });
        }
        let bytes = self.bytes.as_ref().expect("owned stdin payload").as_bytes();
        if self.offset < bytes.len() {
            let end = bytes.len().min(self.offset + 32 * 1024);
            match pipe.write(&bytes[self.offset..end]) {
                Ok(0) => {
                    return Err(ExecutionError::StdinDeliveryFailed {
                        kind: ErrorKind::WriteZero,
                    });
                }
                Ok(n) => self.offset += n,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                    return Ok(false);
                }
                Err(e) => return Err(ExecutionError::StdinWriteFailed { kind: e.kind() }),
            }
        }
        if self.offset != bytes.len() {
            return Ok(false);
        }
        // Consume descriptor ownership once. Never retry close: a failed close
        // may already have released the descriptor for reuse by another thread.
        let fd = self.pipe.take().expect("owned stdin pipe").into_raw_fd();
        self.bytes.take();
        // SAFETY: fd is the uniquely owned pipe descriptor just consumed above.
        if unsafe { close(fd) } != 0 {
            return Err(ExecutionError::StdinDeliveryFailed {
                kind: std::io::Error::last_os_error().kind(),
            });
        }
        #[cfg(test)]
        if self.fault == 4 {
            return Err(ExecutionError::StdinDeliveryFailed {
                kind: ErrorKind::Other,
            });
        }
        Ok(true)
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static FAULT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    pub(super) static TRACKER: std::cell::RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
impl Drop for StdinDelivery {
    fn drop(&mut self) {
        self.pipe.take();
        if let Some(tracker) = &self.tracked {
            tracker.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

unsafe extern "C" {
    fn fcntl(fd: std::os::raw::c_int, cmd: std::os::raw::c_int, ...) -> std::os::raw::c_int;
    fn close(fd: std::os::raw::c_int) -> std::os::raw::c_int;
}

fn nonblocking(pipe: &PipeWriter) -> std::io::Result<()> {
    // Platform capability was checked before this function. These flags match
    // Darwin and the supported Linux/Android x86/ARM ABIs respectively.
    let flag = if cfg!(target_vendor = "apple") {
        4
    } else {
        0x800
    };
    // SAFETY: the descriptor stays owned by pipe; F_GETFL takes no third argument.
    let flags = unsafe { fcntl(pipe.as_raw_fd(), 3) };
    if flags < 0 || unsafe { fcntl(pipe.as_raw_fd(), 4, flags | flag) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
