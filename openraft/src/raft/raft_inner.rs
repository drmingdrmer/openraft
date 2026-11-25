use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use display_more::DisplayOptionExt;
use tracing::Level;

use crate::Config;
use crate::Extensions;
use crate::OptionalSend;
use crate::RaftMetrics;
use crate::RaftTypeConfig;
use crate::async_runtime::MpscSender;
use crate::async_runtime::watch::WatchReceiver;
use crate::async_runtime::watch::WatchSender;
use crate::config::RuntimeConfig;
use crate::core::io_flush_tracking::IoProgressWatcher;
use crate::core::raft_msg::RaftMsg;
use crate::core::raft_msg::external_command::ExternalCommand;
use crate::errors::Fatal;
use crate::errors::Infallible;
use crate::metrics::RaftDataMetrics;
use crate::metrics::RaftServerMetrics;
use crate::metrics::Wait;
use crate::raft::core_state::CoreState;
use crate::raft::linearizable_read::lease_read_state::LeaseReadState;
use crate::type_config::TypeConfigExt;
use crate::type_config::alias::MpscSenderOf;
use crate::type_config::alias::OneshotReceiverOf;
use crate::type_config::alias::OneshotSenderOf;
use crate::type_config::alias::WatchReceiverOf;
use crate::type_config::alias::WatchSenderOf;

/// How long [`RaftInner::bounded_core_stop_error`] waits for RaftCore to actually stop, after a
/// response channel closed or the state machine worker did not accept a command.
///
/// A real stop is observable almost immediately -- RaftCore reports it in metrics and drops the
/// metrics sender as its task ends -- so this only needs to tolerate scheduling latency, not an
/// actual shutdown. It solely bounds how long a caller waits on such a failure path.
const RECV_CORE_STOP_TIMEOUT: Duration = Duration::from_secs(1);

/// RaftInner is the internal handle and provides internally used APIs to communicate with
/// `RaftCore`.
pub(in crate::raft) struct RaftInner<C>
where C: RaftTypeConfig
{
    pub(in crate::raft) id: C::NodeId,
    pub(in crate::raft) config: Arc<Config>,
    pub(in crate::raft) runtime_config: Arc<RuntimeConfig>,
    pub(in crate::raft) tx_api: MpscSenderOf<C, RaftMsg<C>>,
    pub(in crate::raft) rx_lease_read_state: WatchReceiverOf<C, Option<Arc<LeaseReadState<C>>>>,
    pub(in crate::raft) rx_metrics: WatchReceiverOf<C, RaftMetrics<C>>,
    pub(in crate::raft) rx_data_metrics: WatchReceiverOf<C, RaftDataMetrics<C>>,
    pub(in crate::raft) rx_server_metrics: WatchReceiverOf<C, RaftServerMetrics<C>>,
    pub(in crate::raft) progress_watcher: IoProgressWatcher<C>,

    /// The control channel of `RaftCore`: a send wakes it up, and closing the channel, by taking
    /// and dropping this sender, stops it.
    pub(in crate::raft) tx_control: Mutex<Option<WatchSenderOf<C, ()>>>,
    pub(in crate::raft) core_state: Mutex<CoreState<C>>,

    /// Type-map for storing user-defined extension data.
    ///
    /// External crates can access this via [`Raft::extensions()`](`crate::Raft::extensions`).
    pub(in crate::raft) extensions: Extensions,
}

impl<C> RaftInner<C>
where C: RaftTypeConfig
{
    pub(crate) fn id(&self) -> &C::NodeId {
        &self.id
    }

    pub(crate) fn config(&self) -> &Config {
        self.config.as_ref()
    }

    pub(crate) async fn send_msg(&self, mes: RaftMsg<C>) -> Result<(), Fatal<C>> {
        let send_res = self.tx_api.send(mes).await;

        if let Err(e) = send_res {
            let msg = e.0;

            let fatal = self.get_core_stop_error().await;
            tracing::error!("failed to send RaftMsg to RaftCore: {msg}, error: {fatal}",);
            return Err(fatal);
        }
        Ok(())
    }

    /// Invoke RaftCore by sending a RaftMsg and blocks waiting for response.
    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) async fn call_core<T>(&self, mes: RaftMsg<C>, rx: OneshotReceiverOf<C, T>) -> Result<T, Fatal<C>>
    where T: OptionalSend {
        let sum = if tracing::enabled!(Level::DEBUG) {
            Some(mes.to_string())
        } else {
            None
        };

        self.send_msg(mes).await?;
        self.recv_msg(rx).await.inspect_err(|_e| {
            tracing::error!("failed to receive from RaftCore: {}", sum.display());
        })
    }

    /// Invoke RaftCore with a message that carries a oneshot responder, and block waiting for the
    /// response.
    ///
    /// The sender half is handed to `make_msg` and the matching receiver half is awaited here.
    /// Prefer this over [`Self::call_core`]: `RaftMsg` does not carry the response type, so
    /// `call_core` accepts any receiver, including one belonging to a different request.
    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) async fn call_core_oneshot<T, F>(&self, make_msg: F) -> Result<T, Fatal<C>>
    where
        T: OptionalSend,
        F: FnOnce(OneshotSenderOf<C, T>) -> RaftMsg<C>,
    {
        let (tx, rx) = C::oneshot();
        self.call_core(make_msg(tx), rx).await
    }

    /// Receive a message from RaftCore, return an error if the response is not delivered.
    pub(crate) async fn recv_msg<T, E>(&self, rx: impl Future<Output = Result<T, E>>) -> Result<T, Fatal<C>>
    where
        T: OptionalSend,
        E: OptionalSend,
    {
        let recv_res = rx.await;
        tracing::debug!("{}: receives result is error: {:?}", func_name!(), recv_res.is_err());

        match recv_res {
            Ok(x) => Ok(x),
            Err(_) => Err(self.missing_response_error().await),
        }
    }

    /// Return the error for a response channel that was closed without a reply.
    ///
    /// Usually this means RaftCore has stopped, and the error is the fatal error that stopped it.
    /// But a task that serves the request, such as the state machine worker after a panic, can
    /// drop the responder without replying while RaftCore runs; joining the core then would block
    /// forever. So the wait is bounded: if RaftCore is not seen to stop within
    /// [`RECV_CORE_STOP_TIMEOUT`], this returns `Fatal::NoResponse`, which says only that no
    /// response arrived.
    ///
    /// The returned future borrows nothing from `self`, so a caller that must not keep the node
    /// running can drop its handle before awaiting it.
    pub(crate) fn missing_response_error(&self) -> impl Future<Output = Fatal<C>> + OptionalSend + use<C> {
        let core_stop_error = self.bounded_core_stop_error();

        async move {
            match core_stop_error.await {
                Some(fatal) => {
                    tracing::error!("{}: error: {}", func_name!(), fatal);
                    fatal
                }
                None => {
                    tracing::error!(
                        "{}: response dropped without a reply, and RaftCore was not seen to stop within {:?}",
                        func_name!(),
                        RECV_CORE_STOP_TIMEOUT
                    );
                    Fatal::NoResponse
                }
            }
        }
    }

    /// Return the error that stopped RaftCore, or `None` if RaftCore is not seen to stop within
    /// [`RECV_CORE_STOP_TIMEOUT`].
    ///
    /// This is the lookup after a failure that RaftCore's stop usually causes, but that can also
    /// happen while RaftCore runs: joining RaftCore then would block forever. The caller supplies
    /// the error for `None`, the failure it saw.
    ///
    /// The returned future borrows nothing from `self`.
    fn bounded_core_stop_error(&self) -> impl Future<Output = Option<Fatal<C>>> + OptionalSend + use<C> {
        let core_stopped = self.wait_core_stopped(RECV_CORE_STOP_TIMEOUT);

        // A response channel closes, and the state machine worker stops accepting commands, only
        // once RaftCore has started, or once the node was shut down before `run()`. Neither state
        // changes again, so reading it before the wait is the same as reading it after.
        let core_join = {
            let state = self.core_state.lock().unwrap();
            match &*state {
                CoreState::Started(join) => Some(join.clone()),
                CoreState::Unstarted(_) | CoreState::StoppedBeforeRun => None,
            }
        };

        async move {
            if !core_stopped.await {
                return None;
            }
            let fatal = match core_join {
                // Safe unwrap: the core result is always an error
                Some(join) => join.await.0.unwrap_err(),
                // As in `join_core_task()`, a node that never ran stopped with `Stopped`.
                None => Fatal::Stopped,
            };
            Some(fatal)
        }
    }

    /// Wait up to `timeout` for RaftCore to stop, without joining (and thus consuming) it.
    ///
    /// Observes the metrics watch channel, a non-destructive signal safe to poll from any number of
    /// callers: RaftCore sets [`running_state`](RaftMetrics::running_state) to `Err` when it stops,
    /// and drops the metrics sender when its task ends -- including on panic. Either is a reliable
    /// signal that the core has stopped.
    ///
    /// Returns `true` if a stop was observed, `false` if none was observed within `timeout`, such
    /// as when a dropped responder, not a core shutdown, closed the response channel.
    /// The returned future borrows nothing from `self`.
    fn wait_core_stopped(&self, timeout: Duration) -> impl Future<Output = bool> + OptionalSend + use<C> {
        let mut rx = self.rx_metrics.clone();
        let observe = async move {
            loop {
                if rx.borrow_watched().running_state.is_err() {
                    return;
                }
                // `changed()` errors only when the metrics sender is dropped, i.e. the core task
                // has ended.
                if rx.changed().await.is_err() {
                    return;
                }
            }
        };
        // The timeout starts when the returned future is first polled, not when it is created.
        async move { C::timeout(timeout, observe).await.is_ok() }
    }

    /// Send an [`ExternalCommand`] to RaftCore to execute in the `RaftCore` thread.
    ///
    /// It returns at once.
    pub(in crate::raft) async fn send_external_command(&self, cmd: ExternalCommand<C>) -> Result<(), Fatal<C>> {
        let send_res = self.tx_api.send(RaftMsg::ExternalCommand { cmd }).await;

        if send_res.is_err() {
            let fatal = self.get_core_stop_error().await;
            return Err(fatal);
        }
        Ok(())
    }

    /// Wake up RaftCore without waiting, so that it reads the runtime switches again.
    ///
    /// Wake-ups that RaftCore has not seen yet merge into one. After shutdown it does nothing.
    pub(in crate::raft) fn wake_core(&self) {
        let tx_control = self.tx_control.lock().unwrap();
        if let Some(tx) = tx_control.as_ref() {
            tx.send_if_modified(|_| true);
        }
    }

    /// Get the error that caused RaftCore to stop.
    pub(crate) async fn get_core_stop_error(&self) -> Fatal<C> {
        let (core_res, _) = self.join_core_task().await;

        // Safe unwrap: core_res is always an error
        core_res.unwrap_err()
    }

    /// Return the error for a command that the state machine worker did not accept.
    ///
    /// The worker, not RaftCore, owns the receiver of its command channel, and it can stop on its
    /// own, such as after a panic in a state machine method, while RaftCore runs. So this does not
    /// block on RaftCore as [`Self::get_core_stop_error`] would: if RaftCore is not seen to stop
    /// within [`RECV_CORE_STOP_TIMEOUT`], this returns `Fatal::Stopped`, the error that RaftCore
    /// stops with when it finds the worker stopped. It is not `Fatal::NoResponse`: the command was
    /// never submitted, and a fire-and-forget command expects no response.
    pub(crate) async fn sm_worker_unavailable_error(&self) -> Fatal<C> {
        let core_stop_error = self.bounded_core_stop_error().await;
        core_stop_error.unwrap_or_else(|| {
            tracing::error!(
                "{}: the state machine worker did not accept a command, and RaftCore was not seen to stop within {:?}",
                func_name!(),
                RECV_CORE_STOP_TIMEOUT
            );
            Fatal::Stopped
        })
    }

    /// Wait for `RaftCore` and return its result and whether its runtime join failed.
    #[tracing::instrument(level = "debug", skip_all)]
    pub(in crate::raft) async fn join_core_task(&self) -> (Result<Infallible, Fatal<C>>, bool) {
        let join = {
            let mut state = self.core_state.lock().unwrap();
            match &*state {
                CoreState::Unstarted(_) | CoreState::StoppedBeforeRun => {
                    // Dropping the unstarted future closes all of RaftCore's channels.
                    *state = CoreState::StoppedBeforeRun;
                    return (Err(Fatal::Stopped), false);
                }
                CoreState::Started(join) => join.clone(),
            }
        };
        join.await
    }

    pub(crate) fn wait(&self, timeout: Option<Duration>) -> Wait<C> {
        Wait {
            timeout,
            rx: self.rx_metrics.clone(),
        }
    }
}

#[cfg(test)]
mod raft_inner_test;
