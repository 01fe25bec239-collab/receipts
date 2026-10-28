//! Bounded asynchronous observations. Record payloads are explicit, never Debug.
use crate::{ClaudeAttemptHandle, ClaudeStreamJsonError, claude_stream_json::protocol};
use std::{
    future::{Future, poll_fn},
    pin::Pin,
    sync::{Arc, Condvar, Mutex, atomic::Ordering},
    task::{Context, Poll, Waker},
    thread::{self, JoinHandle},
    time::Duration,
};

pub enum ClaudeAttemptEvent {
    Record(Vec<u8>),
    Unavailable(ClaudeEventError),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeEventError {
    Protocol(ClaudeStreamJsonError),
    Collection,
    Observer,
}
struct Timer {
    stopped: bool,
    waker: Option<Waker>,
}
/// One observer thread per stream, one snapshot and at most one record per poll.
/// Slow consumers can lose evidence at Workspace's capture limit; that ends with
/// an explicit truncation error. No history or unbounded event queue is retained.
pub struct ClaudeEventStream<'a> {
    handle: &'a ClaudeAttemptHandle,
    cursor: usize,
    ended: bool,
    timer: Arc<(Mutex<Timer>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}
impl<'a> ClaudeEventStream<'a> {
    pub(crate) fn new(handle: &'a ClaudeAttemptHandle) -> Self {
        let timer = Arc::new((
            Mutex::new(Timer {
                stopped: false,
                waker: None,
            }),
            Condvar::new(),
        ));
        let shared = timer.clone();
        let worker = thread::Builder::new()
            .name("claude-events".into())
            .spawn(move || {
                let (mutex, cv) = &*shared;
                let mut state = mutex.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if state.stopped {
                        break;
                    }
                    state = cv
                        .wait_timeout(state, Duration::from_millis(10))
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                    if let Some(waker) = state.waker.take() {
                        drop(state);
                        waker.wake();
                        state = mutex.lock().unwrap_or_else(|e| e.into_inner());
                    }
                }
            })
            .ok();
        Self {
            handle,
            cursor: 0,
            ended: false,
            timer,
            worker,
        }
    }
    pub fn next_event(&mut self) -> impl Future<Output = Option<ClaudeAttemptEvent>> + '_ {
        poll_fn(|cx| Pin::new(&mut *self).poll_next(cx))
    }
    fn unavailable(&mut self, error: ClaudeEventError) -> Poll<Option<ClaudeAttemptEvent>> {
        self.ended = true;
        Poll::Ready(Some(ClaudeAttemptEvent::Unavailable(error)))
    }
    pub fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<ClaudeAttemptEvent>> {
        let this = self.get_mut();
        if this.ended {
            return Poll::Ready(None);
        }
        if this.worker.is_none() {
            return this.unavailable(ClaudeEventError::Observer);
        }
        let completed = this.handle.completed.load(Ordering::Acquire);
        let snapshot = match this.handle.attempt.snapshot_output() {
            Ok(snapshot) => snapshot,
            Err(_) => return this.unavailable(ClaudeEventError::Observer),
        };
        let stdout = snapshot.output().stdout();
        if stdout.truncated() {
            return this.unavailable(ClaudeEventError::Protocol(
                ClaudeStreamJsonError::StdoutTruncated,
            ));
        }
        let bytes = [stdout.head(), stdout.tail()].concat();
        let remaining = &bytes[this.cursor..];
        let end = remaining
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .or_else(|| (completed != 0 && !remaining.is_empty()).then_some(remaining.len()));
        if let Some(end) = end {
            let record = remaining[..end].to_vec();
            if let Err(error) = protocol(&Some(record.clone())) {
                return this.unavailable(ClaudeEventError::Protocol(error));
            }
            this.cursor += end;
            return Poll::Ready(Some(ClaudeAttemptEvent::Record(record)));
        }
        if completed == 2 {
            return this.unavailable(ClaudeEventError::Collection);
        }
        if completed == 1 {
            this.ended = true;
            return Poll::Ready(None);
        }
        this.timer.0.lock().unwrap_or_else(|e| e.into_inner()).waker = Some(cx.waker().clone());
        Poll::Pending
    }
}
impl Drop for ClaudeEventStream<'_> {
    fn drop(&mut self) {
        self.timer
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopped = true;
        self.timer.1.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

use crate::{CodexAttemptHandle, CodexLiveProtocolError};
pub enum CodexAttemptEvent {
    Record(Vec<u8>),
    Unavailable(CodexEventError),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexEventError {
    Protocol(CodexLiveProtocolError),
    Collection,
    Observer,
    ContradictoryTermination,
}
/// One observer thread per stream, one snapshot and at most one record per poll.
/// Slow consumers can lose evidence at Workspace's capture limit; that ends with
/// an explicit truncation error. No history or unbounded event queue is retained.
pub struct CodexEventStream<'a> {
    handle: &'a CodexAttemptHandle,
    cursor: usize,
    ended: bool,
    completed_seen: bool,
    failed_seen: bool,
    timer: Arc<(Mutex<Timer>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}
impl<'a> CodexEventStream<'a> {
    pub(crate) fn new(handle: &'a CodexAttemptHandle) -> Self {
        let timer = Arc::new((
            Mutex::new(Timer {
                stopped: false,
                waker: None,
            }),
            Condvar::new(),
        ));
        let shared = timer.clone();
        let worker = thread::Builder::new()
            .name("codex-events".into())
            .spawn(move || {
                let (mutex, cv) = &*shared;
                let mut state = mutex.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if state.stopped {
                        break;
                    }
                    state = cv
                        .wait_timeout(state, Duration::from_millis(10))
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                    if let Some(waker) = state.waker.take() {
                        drop(state);
                        waker.wake();
                        state = mutex.lock().unwrap_or_else(|e| e.into_inner());
                    }
                }
            })
            .ok();
        Self {
            handle,
            cursor: 0,
            ended: false,
            completed_seen: false,
            failed_seen: false,
            timer,
            worker,
        }
    }
    pub fn next_event(&mut self) -> impl Future<Output = Option<CodexAttemptEvent>> + '_ {
        poll_fn(|cx| Pin::new(&mut *self).poll_next(cx))
    }
    fn unavailable(&mut self, error: CodexEventError) -> Poll<Option<CodexAttemptEvent>> {
        self.ended = true;
        Poll::Ready(Some(CodexAttemptEvent::Unavailable(error)))
    }
    pub fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<CodexAttemptEvent>> {
        let this = self.get_mut();
        if this.ended {
            return Poll::Ready(None);
        }
        if this.worker.is_none() {
            return this.unavailable(CodexEventError::Observer);
        }
        let completed = this.handle.completed.load(Ordering::Acquire);
        let snapshot = match this.handle.attempt.snapshot_output() {
            Ok(snapshot) => snapshot,
            Err(_) => return this.unavailable(CodexEventError::Observer),
        };
        let stdout = snapshot.output().stdout();
        if stdout.truncated() {
            return this.unavailable(CodexEventError::Protocol(
                CodexLiveProtocolError::StdoutTruncated,
            ));
        }
        let bytes = [stdout.head(), stdout.tail()].concat();
        let remaining = &bytes[this.cursor..];
        let end = remaining
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .or_else(|| (completed != 0 && !remaining.is_empty()).then_some(remaining.len()));
        if let Some(end) = end {
            let record = remaining[..end].to_vec();
            let parsed = match crate::codex_jsonl::interpret_stdout(&record) {
                Ok(parsed) => parsed,
                Err(error) => {
                    return this.unavailable(CodexEventError::Protocol(
                        CodexLiveProtocolError::Jsonl(error),
                    ));
                }
            };
            this.completed_seen |= parsed.turn_completed_observed();
            this.failed_seen |= parsed.turn_failed_observed();
            if this.completed_seen && this.failed_seen {
                return this.unavailable(CodexEventError::ContradictoryTermination);
            }
            this.cursor += end;
            return Poll::Ready(Some(CodexAttemptEvent::Record(record)));
        }
        if completed == 2 {
            return this.unavailable(CodexEventError::Collection);
        }
        if completed == 1 {
            this.ended = true;
            return Poll::Ready(None);
        }
        this.timer.0.lock().unwrap_or_else(|e| e.into_inner()).waker = Some(cx.waker().clone());
        Poll::Pending
    }
}
impl Drop for CodexEventStream<'_> {
    fn drop(&mut self) {
        self.timer
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopped = true;
        self.timer.1.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
