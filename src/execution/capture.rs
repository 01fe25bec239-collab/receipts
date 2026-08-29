//! Bounded, separately retained stdout/stderr capture for timed runs.
//!
//! Each captured stream is drained to EOF, but only a fixed, pre-allocated
//! amount of it is ever retained in memory. Retention is `HEAD_TAIL`: the
//! exact first [`STREAM_HEAD_RETENTION_BYTES`] bytes plus the exact last
//! [`STREAM_TAIL_RETENTION_BYTES`] bytes. Nothing synthetic is ever
//! inserted between them — no elision marker, no separator, no line
//! ending — so the retained segments contain process bytes only.
//!
//! Frozen properties of this slice:
//!
//! * **Bounded memory.** Retention buffers are allocated once, fallibly,
//!   *before* the child exists; nothing here grows with the child's total
//!   output. Reading uses one small fixed stack buffer per stream.
//! * **Draining is not retention.** Reaching the retention limit never
//!   stops reading: the middle of an oversized stream is consumed and
//!   discarded so the child can never block on a full pipe.
//! * **Raw bytes.** stdout and stderr are arbitrary byte streams. Nothing
//!   is decoded, trimmed, normalized, or line-ended; `String` is never the
//!   canonical representation.
//! * **Independent streams.** stdout and stderr have separate buffers,
//!   separate counters and separate limits. Neither borrows the other's
//!   unused capacity, and no ordering between the two pipes is invented.
//! * **Overflow fails closed.** Total-byte accounting is checked `u64`
//!   arithmetic; it never wraps and never saturates.
//!
//! Output digests are deliberately not implemented here: this slice
//! captures bytes only.

use std::collections::TryReserveError;
use std::io::{ErrorKind, Read};

use crate::execution::outcome::ProcessRunOutcome;

/// Maximum number of bytes retained in memory for one captured stream.
///
/// A stream whose total size is exactly this value is retained in full and
/// is **not** truncated; only a strictly larger stream is.
pub const STREAM_CAPTURE_LIMIT_BYTES: u64 = 1_048_576;

/// Bytes retained from the beginning of a truncated stream.
pub const STREAM_HEAD_RETENTION_BYTES: usize = 524_288;

/// Bytes retained from the end of a truncated stream.
pub const STREAM_TAIL_RETENTION_BYTES: usize = 524_288;

/// Fixed per-stream read buffer. Sized once and never derived from how much
/// the child actually produces.
const READ_BUFFER_BYTES: usize = 32 * 1024;

/// One stream's captured bytes plus the metadata describing what the
/// complete stream looked like.
///
/// `head` and `tail` are the only retained bytes and are always raw child
/// output. When `truncated` is `false`, `head` followed by `tail`
/// reproduces the complete stream exactly; when it is `true`, `head` is the
/// exact first [`STREAM_HEAD_RETENTION_BYTES`] bytes and `tail` the exact
/// last [`STREAM_TAIL_RETENTION_BYTES`] bytes, with the middle discarded
/// (but still counted in `total_bytes`, because it was still drained).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedStream {
    head: Vec<u8>,
    tail: Vec<u8>,
    total_bytes: u64,
    captured_bytes: u64,
    truncated: bool,
}

impl CapturedStream {
    /// The retained leading bytes of the stream.
    pub fn head(&self) -> &[u8] {
        &self.head
    }

    /// The retained trailing bytes of the stream.
    ///
    /// Empty whenever the whole stream already fits in
    /// [`CapturedStream::head`].
    pub fn tail(&self) -> &[u8] {
        &self.tail
    }

    /// Every byte drained from this stream through EOF, including bytes
    /// discarded from retention.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// How many bytes are actually retained: `head.len() + tail.len()`.
    pub fn captured_bytes(&self) -> u64 {
        self.captured_bytes
    }

    /// Whether the complete stream exceeded [`STREAM_CAPTURE_LIMIT_BYTES`]
    /// and therefore lost its middle from retention.
    pub fn truncated(&self) -> bool {
        self.truncated
    }
}

/// One completed timed run with both streams captured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedProcessRun {
    outcome: ProcessRunOutcome,
    stdout: CapturedStream,
    stderr: CapturedStream,
}

impl CapturedProcessRun {
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn new(
        outcome: ProcessRunOutcome,
        stdout: CapturedStream,
        stderr: CapturedStream,
    ) -> Self {
        Self {
            outcome,
            stdout,
            stderr,
        }
    }

    /// The ordinary lifecycle metadata for this run.
    pub fn outcome(&self) -> &ProcessRunOutcome {
        &self.outcome
    }

    /// The child's captured standard output.
    pub fn stdout(&self) -> &CapturedStream {
        &self.stdout
    }

    /// The child's captured standard error.
    pub fn stderr(&self) -> &CapturedStream {
        &self.stderr
    }
}

/// What can go wrong while draining one stream.
///
/// Kept separate from [`ExecutionError`](crate::execution::ExecutionError)
/// so the reader itself stays stream-agnostic; the runner attaches which
/// stream failed when it maps this into the typed error surface.
#[derive(Debug)]
pub(crate) enum CaptureFault {
    /// Reading from the child pipe failed.
    Read(std::io::Error),
    /// Counting the drained bytes would overflow `u64`.
    TotalByteOverflow {
        /// Bytes already counted before the failing chunk.
        counted: u64,
        /// Size of the chunk that could not be counted.
        chunk: usize,
    },
}

/// Fixed-capacity `HEAD_TAIL` retention for exactly one stream.
///
/// The head buffer fills first; every later byte flows through a fixed
/// ring that keeps only the most recent tail-capacity bytes. Both buffers
/// are allocated up front, so no allocation happens while the child is
/// running and retained memory never grows with the child's output.
#[derive(Debug)]
pub(crate) struct BoundedStreamRetention {
    head: Vec<u8>,
    head_cap: usize,
    /// Ring storage; always exactly `tail_cap` bytes long once allocated.
    tail: Vec<u8>,
    tail_cap: usize,
    tail_start: usize,
    tail_len: usize,
    total_bytes: u64,
}

#[cfg_attr(not(unix), allow(dead_code))]
impl BoundedStreamRetention {
    /// Allocates both retention buffers fallibly.
    ///
    /// Fallible allocation is the whole point: the runner establishes
    /// retention capacity *before* spawning a child, so a machine that
    /// cannot honor the bound fails closed with no process left behind.
    pub(crate) fn new(head_cap: usize, tail_cap: usize) -> Result<Self, TryReserveError> {
        let mut head = Vec::new();
        head.try_reserve_exact(head_cap)?;
        let mut tail = Vec::new();
        tail.try_reserve_exact(tail_cap)?;
        // The reservation above already secured the exact capacity, so
        // this resize cannot allocate again.
        tail.resize(tail_cap, 0);
        Ok(Self {
            head,
            head_cap,
            tail,
            tail_cap,
            tail_start: 0,
            tail_len: 0,
            total_bytes: 0,
        })
    }

    /// The frozen production retention shape.
    pub(crate) fn with_frozen_limits() -> Result<Self, TryReserveError> {
        Self::new(STREAM_HEAD_RETENTION_BYTES, STREAM_TAIL_RETENTION_BYTES)
    }

    /// Accounts for and retains one freshly drained chunk.
    ///
    /// Every byte is counted, whether or not it survives retention.
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<(), CaptureFault> {
        self.total_bytes = self.total_bytes.checked_add(chunk.len() as u64).ok_or(
            CaptureFault::TotalByteOverflow {
                counted: self.total_bytes,
                chunk: chunk.len(),
            },
        )?;

        let head_room = self.head_cap - self.head.len();
        let take = head_room.min(chunk.len());
        self.head.extend_from_slice(&chunk[..take]);
        let rest = &chunk[take..];
        if !rest.is_empty() {
            self.push_tail(rest);
        }
        Ok(())
    }

    /// Keeps only the most recent `tail_cap` bytes of everything pushed
    /// past the head.
    fn push_tail(&mut self, bytes: &[u8]) {
        let cap = self.tail_cap;
        if cap == 0 {
            return;
        }
        if bytes.len() >= cap {
            self.tail.copy_from_slice(&bytes[bytes.len() - cap..]);
            self.tail_start = 0;
            self.tail_len = cap;
            return;
        }
        let write_at = (self.tail_start + self.tail_len) % cap;
        let first = (cap - write_at).min(bytes.len());
        self.tail[write_at..write_at + first].copy_from_slice(&bytes[..first]);
        if first < bytes.len() {
            self.tail[..bytes.len() - first].copy_from_slice(&bytes[first..]);
        }
        let filled = self.tail_len + bytes.len();
        if filled > cap {
            self.tail_start = (self.tail_start + (filled - cap)) % cap;
            self.tail_len = cap;
        } else {
            self.tail_len = filled;
        }
    }

    /// Bytes counted so far. Exposed for the runner's fail-closed reporting.
    #[cfg(test)]
    pub(crate) fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Forces the running byte count, so checked-overflow behavior can be
    /// exercised without emitting `u64::MAX` bytes. Test-only and private.
    #[cfg(test)]
    pub(crate) fn set_total_bytes_for_test(&mut self, total: u64) {
        self.total_bytes = total;
    }

    /// Freezes retention into the immutable captured representation.
    ///
    /// Linearizing the ring is a rotation of already-owned storage, so no
    /// post-run allocation proportional to the stream happens here either.
    pub(crate) fn finish(mut self) -> CapturedStream {
        if self.tail_len == self.tail_cap {
            self.tail.rotate_left(self.tail_start);
        } else {
            // A partially filled ring never wrapped, so it starts at 0.
            debug_assert_eq!(self.tail_start, 0);
            self.tail.truncate(self.tail_len);
        }
        let captured_bytes = (self.head.len() + self.tail.len()) as u64;
        CapturedStream {
            head: self.head,
            tail: self.tail,
            total_bytes: self.total_bytes,
            captured_bytes,
            truncated: self.total_bytes > (self.head_cap + self.tail_cap) as u64,
        }
    }
}

/// Drains `source` to EOF into `retention`.
///
/// Reading continues past the retention bound on purpose: the limit bounds
/// retained memory, never bytes consumed from the pipe, because a reader
/// that stopped early would let the child block forever on a full pipe.
/// `Interrupted` is retried; every other read failure fails closed.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn drain_to_eof<R: Read>(
    mut source: R,
    retention: &mut BoundedStreamRetention,
) -> Result<(), CaptureFault> {
    let mut buffer = [0u8; READ_BUFFER_BYTES];
    loop {
        match source.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => retention.push(&buffer[..read])?,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(CaptureFault::Read(error)),
        }
    }
}
