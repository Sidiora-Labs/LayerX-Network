//! Deliberately non-blocking executor for synchronously bounded Human journeys.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

/// Polls one future exactly once. Production component handlers use this only
/// with the configured synchronous UDS boundaries. Suspension is a closed
/// refusal, never an invitation to spin or create an unbounded runtime.
pub fn poll_once_ready<F: Future>(future: F) -> Result<F::Output, PendingOperation> {
    let waker = Waker::from(Arc::new(RefuseWake));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => Ok(output),
        Poll::Pending => Err(PendingOperation),
    }
}

struct RefuseWake;

impl Wake for RefuseWake {
    fn wake(self: Arc<Self>) {}
    fn wake_by_ref(self: &Arc<Self>) {}
}

/// A configured boundary suspended instead of completing its single bounded
/// exchange. The original mutation identity remains authoritative.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingOperation;
