//! Stopping work that is no longer wanted.
//!
//! Whoever starts long work holds a [`Cancel`] and runs the work
//! [under it](Cancel::run); the work — a step being applied, a face
//! being sampled — passes [`checkpoint`]s, wherever it loops for long.
//! Once the `Cancel` is cancelled the next checkpoint unwinds the work
//! back to where it was started, as would a panic; that one is caught
//! there too, so a step that breaks is a step that failed, not a thread
//! lost.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The work stopped because it was no longer wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the build was cancelled")]
pub struct Cancelled;

/// Why work [run](Cancel::run) under a [`Cancel`] did not finish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Stopped {
    /// It was cancelled.
    #[error("the build was cancelled")]
    Cancelled,
    /// It panicked, with this message.
    #[error("it broke: {0}")]
    Broke(String),
}

/// What stops work: shared between who asked for it and the threads
/// doing it.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

thread_local! {
    /// What the work this thread is doing is run under.
    static CURRENT: RefCell<Option<Cancel>> = const { RefCell::new(None) };
}

impl Cancel {
    /// One that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stops the work that holds this.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether it was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// Does `work` on this thread, its [`checkpoint`]s looking at this:
    /// what it makes, or why it stopped. (What the work was changing
    /// when it stopped is left half done, and is the caller's to drop.)
    pub fn run<T>(&self, work: impl FnOnce() -> T) -> Result<T, Stopped> {
        let outer = CURRENT.replace(Some(self.clone()));
        let done = catch_unwind(AssertUnwindSafe(work));
        CURRENT.set(outer);
        done.map_err(|payload| {
            if payload.is::<Cancelled>() {
                Stopped::Cancelled
            } else if let Some(text) = payload.downcast_ref::<&str>() {
                Stopped::Broke((*text).to_owned())
            } else if let Some(text) = payload.downcast_ref::<String>() {
                Stopped::Broke(text.clone())
            } else {
                Stopped::Broke("for no reason it gave".to_owned())
            }
        })
    }
}

/// Stops the work this thread is doing, if it was cancelled: unwinds to
/// where it was [started](Cancel::run). Costs a few nanoseconds; to be
/// passed in every loop that can run for long.
pub fn checkpoint() {
    let cancelled = CURRENT.with_borrow(|c| c.as_ref().is_some_and(Cancel::is_cancelled));
    if cancelled {
        resume_unwind(Box::new(Cancelled));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_work_stops_at_its_next_checkpoint_and_broken_work_is_told() {
        let cancel = Cancel::new();
        let stopper = cancel.clone();
        let stopped = cancel.run(|| {
            let mut passed = 0_u64;
            loop {
                checkpoint();
                passed += 1;
                if passed == 1000 {
                    stopper.cancel();
                }
            }
        });
        assert_eq!(stopped, Err::<(), _>(Stopped::Cancelled));
        // (Outside of it, nothing is stopped.)
        checkpoint();
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let broke = Cancel::new().run(|| panic!("no such face"));
        std::panic::set_hook(hook);
        assert_eq!(
            broke,
            Err::<(), _>(Stopped::Broke("no such face".to_owned()))
        );
        assert_eq!(Cancel::new().run(|| 3), Ok(3));
    }
}
