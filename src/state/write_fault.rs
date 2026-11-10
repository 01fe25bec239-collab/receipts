//! Write-boundary fault injection for tests.
//!
//! [`boundary`] is called after each durable write step of a multi-row
//! operation. In production builds it is a constant `Ok(())`. Under test a
//! thread-local counter can make the Nth boundary fail, so a control can
//! prove that a failure at *every* write boundary rolls the whole operation
//! back (rows, receipts, watermark and head), and a dry run can count how
//! many boundaries an operation has.

use crate::error::StateError;

#[cfg(not(test))]
#[inline(always)]
pub(crate) fn boundary() -> Result<(), StateError> {
    Ok(())
}

#[cfg(test)]
std::thread_local! {
    static STATE: std::cell::Cell<(usize, Option<usize>)> = const { std::cell::Cell::new((0, None)) };
}

#[cfg(test)]
pub(crate) fn boundary() -> Result<(), StateError> {
    STATE.with(|state| {
        let (seen, fail_at) = state.get();
        let seen = seen + 1;
        state.set((seen, fail_at));
        if fail_at == Some(seen) {
            Err(StateError::InternalQueryFailed {
                detail: "injected write-boundary fault".to_owned(),
            })
        } else {
            Ok(())
        }
    })
}

/// Counts boundaries and fails the `fail_at`th (1-based) one, if given.
/// Returns how many boundaries `work` passed.
#[cfg(test)]
pub(crate) fn with_fault<T>(fail_at: Option<usize>, work: impl FnOnce() -> T) -> (T, usize) {
    STATE.with(|state| state.set((0, fail_at)));
    let value = work();
    let seen = STATE.with(|state| state.replace((0, None)).0);
    (value, seen)
}
