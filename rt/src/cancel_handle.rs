use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;

use openraft_macros::since;

use crate::AsyncRuntime;
use crate::Oneshot;

/// Signals cancellation of a future created by [`AsyncRuntime::cancellable`].
///
/// Dropping the handle also signals cancellation. The caller drives the wrapped future.
#[since(version = "0.10.1")]
pub struct CancelHandle<R>
where R: AsyncRuntime
{
    /// Set when the handle cancels or drops. The wrapped future reads it at the start of every
    /// poll, because a receiver may report its closed channel only on a later poll: Tokio's does
    /// so when the task has used up its budget.
    cancelled: Arc<AtomicBool>,

    /// The wrapped future also waits on the receiving end, which completes once this sender is
    /// dropped, so dropping it wakes a waiting future.
    cancel: Option<<R::Oneshot as Oneshot>::Sender<()>>,
}

impl<R> CancelHandle<R>
where R: AsyncRuntime
{
    /// Create a handle and the future that completes when the handle cancels or drops.
    pub(crate) fn new() -> (Self, Cancelled<R>) {
        let (cancel, receiver) = R::Oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let handle = Self {
            cancelled: cancelled.clone(),
            cancel: Some(cancel),
        };
        (handle, Cancelled { cancelled, receiver })
    }

    /// Signal cancellation once. Cancelling an already completed future is harmless.
    #[since(version = "0.10.1")]
    pub fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.cancel = None;
    }
}

impl<R> Drop for CancelHandle<R>
where R: AsyncRuntime
{
    fn drop(&mut self) {
        self.cancel();
    }
}

/// A future that completes once its [`CancelHandle`] cancels or drops.
pub(crate) struct Cancelled<R>
where R: AsyncRuntime
{
    cancelled: Arc<AtomicBool>,
    receiver: <R::Oneshot as Oneshot>::Receiver<()>,
}

impl<R> Future for Cancelled<R>
where R: AsyncRuntime
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        Pin::new(&mut self.receiver).poll(cx).map(|_closed| ())
    }
}
