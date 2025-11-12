use std::sync::Arc;

use anyerror::AnyError;

use crate::Config;
use crate::RaftNetworkFactory;
use crate::RaftTypeConfig;
use crate::Snapshot;
use crate::StorageError;
use crate::alias::InstantOf;
use crate::alias::MpscSenderOf;
use crate::alias::MpscUnboundedReceiverOf;
use crate::alias::MpscUnboundedWeakSenderOf;
use crate::alias::MutexOf;
use crate::alias::OneshotOf;
use crate::alias::OneshotReceiverOf;
use crate::alias::OneshotSenderOf;
use crate::alias::VoteOf;
use crate::alias::WatchReceiverOf;
use crate::async_runtime::MpscSender;
use crate::async_runtime::Mutex;
use crate::core::notification::Notification;
use crate::core::sm::handle::SnapshotReader;
use crate::display_ext::DisplayOptionExt;
use crate::error::HigherVote;
use crate::error::RPCError;
use crate::error::ReplicationClosed;
use crate::error::ReplicationError;
use crate::error::StreamingError;
use crate::network::RPCOption;
use crate::network::v2::RaftNetworkV2;
use crate::replication::Progress;
use crate::replication::ReplicationCore;
use crate::replication::ReplicationSessionId;
use crate::replication::callbacks::SnapshotCallback;
use crate::replication::request::Data;
use crate::replication::request::Replicate;
use crate::replication::response::ReplicationResult;
use crate::storage::RaftLogStorage;
use crate::type_config::TypeConfigExt;

pub(crate) struct SnapshotTransmit<C, N, LS>
where
    C: RaftTypeConfig,
    N: RaftNetworkFactory<C>,
    LS: RaftLogStorage<C>,
{
    /// The ID of the target Raft node which replication events are to be sent to.
    target: C::NodeId,

    /// Identifies which session this replication belongs to.
    session_id: ReplicationSessionId<C>,

    /// A channel for sending events to the RaftCore.
    #[allow(clippy::type_complexity)]
    tx_notify: MpscSenderOf<C, Notification<C>>,

    /// For receiving cancel signal.
    rx_cancel: WatchReceiverOf<C, ()>,

    /// Another `RaftNetwork` specific for snapshot replication.
    ///
    /// Snapshot transmitting is a long-running task and is processed in a separate task.
    snapshot_network: Arc<MutexOf<C, N::Network>>,

    /// The backoff policy if an [`Unreachable`](`crate::error::Unreachable`) error is returned.
    /// It will be reset to `None` when a successful response is received.
    backoff: Option<Backoff>,

    /// The [`RaftLogStorage::LogReader`] interface.
    log_reader: LS::LogReader,

    /// The handle to get a snapshot directly from the state machine.
    snapshot_reader: SnapshotReader<C>,

    /// The Raft's runtime config.
    config: Arc<Config>,
}

impl<C, N, LS> SnapshotTransmit<C, N, LS>
where
    C: RaftTypeConfig,
    N: RaftNetworkFactory<C>,
    LS: RaftLogStorage<C>,
{
    #[tracing::instrument(level = "info", skip_all)]
    async fn stream_snapshot(mut self) {
        let res = self.do_stream_snapshot().await;
    }

    async fn do_stream_snapshot(mut self) -> Result<(), ReplicationError<C>> {
        tracing::info!("{}", func_name!());

        let mut ith: i32 = -1;
        loop {
            ith += 1;

            let snapshot = self.snapshot_reader.get_snapshot().await.map_err(|reason| {
                tracing::warn!(error = display(&reason), "failed to get snapshot from state machine");
                ReplicationClosed::new(reason)
            })?;

            tracing::info!(
                "{}-th snapshot sending: has read snapshot: meta:{}",
                ith,
                snapshot.as_ref().map(|x| &x.meta).display()
            );

            let snapshot = match snapshot {
                None => {
                    let sto_err = StorageError::read_snapshot(None, AnyError::error("snapshot not found"));
                    return Err(sto_err);
                }
                Some(x) => x,
            };

            let mut option = RPCOption::new(self.config.install_snapshot_timeout());
            option.snapshot_chunk_size = Some(self.config.snapshot_max_chunk_size as usize);

            let res = self.send_snapshot(snapshot, option).await;

            match res {
                Err(error) => {
                    tracing::error!("ReplicationError: {}; when (sending snapshot)", error);

                    match error {
                        ReplicationError::Closed(closed) => {
                            tracing::info!("Snapshot transmitting is canceled: {}", closed);
                            return;
                        }
                        ReplicationError::HigherVote(h) => {
                            self.tx_notify
                                .send(Notification::HigherVote {
                                    target: self.target,
                                    higher: h.higher,
                                    leader_vote: self.session_id.committed_vote(),
                                })
                                .await
                                .ok();
                        }
                        ReplicationError::StorageError(error) => {
                            tracing::error!(error=%error, "error replication to target={}", self.target);

                            self.tx_notify.send(Notification::StorageError { error }).await.ok();
                        }
                        ReplicationError::RPCError(err) => {
                            let retry = match &err {
                                RPCError::Timeout(_) => false,
                                RPCError::Unreachable(_unreachable) => {
                                    // If there is an [`Unreachable`] error, we will backoff for a
                                    // period of time. Backoff will be reset if there is a
                                    // successful RPC is sent.
                                    if self.backoff.is_none() {
                                        self.backoff = Some(self.network.backoff());
                                    }
                                    false
                                }
                                RPCError::Network(_) => false,
                                RPCError::RemoteError(_) => false,
                            };

                            if retry {
                                debug_assert!(self.next_action.is_some(), "next_action must be Some");
                            } else {
                                // If there is no id, it is a heartbeat and do not need to notify RaftCore
                                if need_notify {
                                    self.send_progress_error(err).await;
                                } else {
                                    tracing::warn!("heartbeat RPC failed, do not send any response to RaftCore");
                                };
                            }
                        }
                    };
                }
                Some(x) => {}
            };
        }
    }

    async fn send_snapshot(&mut self, snapshot: Snapshot<C>, option: RPCOption) -> Result<(), ReplicationError<C>> {
        let meta = snapshot.meta.clone();

        let mut net = self.snapshot_network.lock().await;

        let start_time = C::now();

        let c = self.rx_cancel.clone();
        let cancel = async move {
            let _ = c.await;
            ReplicationClosed::new("RaftCore is dropped")
        };

        let vote = self.session_id.vote();
        let resp = net.full_snapshot(vote, snapshot, cancel, option).await?;

        tracing::info!("finished sending full_snapshot, resp: {}", resp);

        // Handle response conditions.
        let sender_vote = self.session_id.vote();
        if resp.vote.as_ref_vote() > sender_vote.as_ref_vote() {
            return Err(ReplicationError::HigherVote(HigherVote {
                higher: resp.vote,
                sender_vote,
            }));
        }

        self.notify_heartbeat_progress(start_time).await;
        self.notify_progress(ReplicationResult(Ok(meta.last_log_id))).await;
        Ok(())
    }

    async fn notify_heartbeat_progress(&mut self, sending_time: InstantOf<C>) {
        self.tx_notify
            .send({
                Notification::HeartbeatProgress {
                    session_id: self.session_id.clone(),
                    target: self.target.clone(),
                    sending_time,
                }
            })
            .await
            .ok();
    }

    async fn notify_progress(&mut self, replication_result: ReplicationResult<C>) {
        tracing::debug!(
            target = display(self.target.clone()),
            result = display(&replication_result),
            "{}",
            func_name!()
        );

        self.tx_notify
            .send({
                Notification::ReplicationProgress {
                    has_payload: true,
                    progress: Progress {
                        session_id: self.session_id.clone(),
                        target: self.target.clone(),
                        result: Ok(replication_result.clone()),
                    },
                }
            })
            .await
            .ok();
    }
}
