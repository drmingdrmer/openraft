//! Watch handle for async watch tasks.

use openraft_macros::since;

use crate::RaftTypeConfig;
use crate::type_config::alias::JoinHandleOf;
use crate::type_config::alias::OneshotSenderOf;

/// Handle to control an async watch task.
///
/// Use [`close()`](`Self::close`) to stop watching and wait for the task to complete.
pub struct WatchChangeHandle<C>
where C: RaftTypeConfig
{
    pub(crate) cancel_tx: Option<OneshotSenderOf<C, ()>>,
    pub(crate) join_handle: Option<JoinHandleOf<C, ()>>,
}

impl<C> WatchChangeHandle<C>
where C: RaftTypeConfig
{
    /// Stop watching and wait for the task to complete, including any active callback.
    ///
    /// Dropping this future while it is pending does not undo the stop signal. Call `close()`
    /// again to wait for the same task to finish.
    #[since(
        version = "0.10.2",
        change = "waits for the watch task even after a cancelled `close()`"
    )]
    pub async fn close(&mut self) {
        drop(self.cancel_tx.take());

        // Keep the join in the handle if this future is dropped while the task is still running.
        if let Some(handle) = self.join_handle.as_mut() {
            handle.await.ok();
            self.join_handle = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::pin::pin;
    use std::time::Duration;

    use futures_util::FutureExt;

    use super::WatchChangeHandle;
    use crate::async_runtime::OneshotSender;
    use crate::type_config::TypeConfigExt;

    crate::declare_raft_types!(TestConfig: AsyncRuntime = openraft_rt_tokio::TokioRuntime);

    #[test]
    fn close_retry_waits_for_task_after_cancelled_close() {
        TestConfig::run(async {
            let timeout = Duration::from_secs(5);
            let (cancel_tx, cancel_rx) = TestConfig::oneshot::<()>();
            let (cancelled_tx, cancelled_rx) = TestConfig::oneshot::<()>();
            let (release_tx, release_rx) = TestConfig::oneshot::<()>();
            let task = TestConfig::spawn(async move {
                assert!(
                    cancel_rx.await.is_err(),
                    "close signals cancellation by dropping the sender"
                );
                cancelled_tx.send(()).unwrap();
                let _ = release_rx.await;
            });
            let mut handle = WatchChangeHandle::<TestConfig> {
                cancel_tx: Some(cancel_tx),
                join_handle: Some(task),
            };

            tracing::info!("drop a pending close and wait for the task to observe cancellation");
            {
                assert!(handle.close().now_or_never().is_none());
                TestConfig::timeout(timeout, cancelled_rx).await.unwrap().unwrap();
            }

            tracing::info!("retry close while the task is still blocked on the release gate");
            {
                let mut retry = pin!(handle.close());
                assert!(
                    retry.as_mut().now_or_never().is_none(),
                    "retry must still wait for the task"
                );
                release_tx.send(()).unwrap();
                TestConfig::timeout(timeout, retry).await.unwrap();
            }

            tracing::info!("close again without polling the completed join");
            {
                assert!(handle.join_handle.is_none());
                assert_eq!(Some(()), handle.close().now_or_never());
            }
        });
    }
}
