//! Step-down watcher: a task that steps down a Leader that is removed from a committed
//! membership config.

use std::pin::pin;
use std::time::Duration;

use futures_util::future::Either;
use futures_util::future::select;
use tracing::Instrument;

use crate::Config;
use crate::RaftTypeConfig;
use crate::StepDownPolicy;
use crate::async_runtime::MpscSender;
use crate::async_runtime::MpscWeakSender;
use crate::async_runtime::watch::WatchReceiver;
use crate::core::ServerState;
use crate::core::raft_msg::RaftMsg;
use crate::core::raft_msg::external_command::ExternalCommand;
use crate::membership::RaftMembershipExt;
use crate::metrics::RaftMetrics;
use crate::metrics::RaftServerMetrics;
use crate::type_config::TypeConfigExt;
use crate::type_config::alias::MpscWeakSenderOf;
use crate::type_config::alias::WatchReceiverOf;

/// Steps down a Leader that is removed from a committed membership config.
///
/// It watches the server metrics, which change only on server state transitions, such as when
/// the membership config that removes this Leader is committed. When this node is a Leader and
/// the committed membership config does not contain it, after the delay given by
/// [`removed_leader_step_down`] it transfers leadership to the most up-to-date voter (chosen
/// from a snapshot of the full metrics), then after another `heartbeat_interval` it reverts this
/// node to a learner, in case the transfer does not take effect.
///
/// It is an automation plugin outside the consensus kernel: it only sends the commands an
/// application could send manually, [`TriggerTransferLeader`] and [`RefreshServerState`], both of
/// which re-validate the state inside `RaftCore`. Therefore acting on stale or coalesced metrics
/// is harmless: an outdated command is just a no-op.
///
/// The task quits when `RaftCore` terminates.
///
/// [`removed_leader_step_down`]: crate::Config::removed_leader_step_down
/// [`TriggerTransferLeader`]: ExternalCommand::TriggerTransferLeader
/// [`RefreshServerState`]: ExternalCommand::RefreshServerState
pub(crate) struct StepDownWatcher<C>
where C: RaftTypeConfig
{
    rx_server_metrics: WatchReceiverOf<C, RaftServerMetrics<C>>,

    /// The full metrics, read only when acting, to choose the transfer-leader target.
    rx_metrics: WatchReceiverOf<C, RaftMetrics<C>>,

    /// Weak sender to the `RaftCore` API channel: the watcher must not keep the channel alive.
    tx_api: MpscWeakSenderOf<C, RaftMsg<C>>,

    /// The delay between the commit of the removing membership config and the transfer-leader.
    step_down_delay: Duration,

    /// The delay between the transfer-leader and reverting this node to a learner.
    transfer_wait: Duration,
}

impl<C> StepDownWatcher<C>
where C: RaftTypeConfig
{
    /// Spawn the watcher task.
    ///
    /// It does nothing if the automated step down is disabled,
    /// i.e., [`removed_leader_step_down`] is [`StepDownPolicy::Never`].
    ///
    /// [`removed_leader_step_down`]: crate::Config::removed_leader_step_down
    pub(crate) fn spawn(
        rx_server_metrics: WatchReceiverOf<C, RaftServerMetrics<C>>,
        rx_metrics: WatchReceiverOf<C, RaftMetrics<C>>,
        tx_api: MpscWeakSenderOf<C, RaftMsg<C>>,
        config: &Config,
    ) {
        let delay = match config.removed_leader_step_down {
            StepDownPolicy::Never => return,
            StepDownPolicy::After(ms) => ms,
        };

        let this = Self {
            rx_server_metrics,
            rx_metrics,
            tx_api,
            step_down_delay: Duration::from_millis(delay),
            transfer_wait: Duration::from_millis(config.heartbeat_interval),
        };

        let span = tracing::debug_span!("step_down_watcher");
        let watch_fut = this.watch_loop().instrument(span);
        let _join_handle = C::spawn(watch_fut);
    }

    async fn watch_loop(mut self) {
        loop {
            self.try_step_down().await;

            let recv_res = self.rx_server_metrics.changed().await;
            if recv_res.is_err() {
                tracing::info!("RaftCore terminated, quit step_down_watcher");
                return;
            }
        }
    }

    /// Step down this node if it is a Leader that is removed from a committed membership config:
    ///
    /// - Wait for `step_down_delay` after the removal is observed. The delay belongs to that
    ///   removal: the vote and the committed membership config that removes this node. It ends
    ///   early when this node is no longer a removed Leader, such as when a newer membership config
    ///   that contains it is committed, and it starts again when the removal is another one, such
    ///   as a second removal after a readmission, or the same config under a new vote. Other
    ///   metrics updates leave the deadline as it is.
    /// - Transfer leadership to the most up-to-date voter.
    /// - Wait for `transfer_wait`, then revert this node to a learner. This is the guaranteed
    ///   fallback in case the transfer takes no effect; the command is fenced by the vote and the
    ///   membership config log id of the removal, thus it is dropped if a new Leader is already
    ///   established or the membership config has changed.
    async fn try_step_down(&mut self) {
        // Clone the metrics out of the watch channel: the borrowed reference is not `Send`,
        // and holding it across an `await` would block the metrics updater.
        let mut server_metrics = self.rx_server_metrics.borrow_watched().clone();
        if !is_removed_leader(&server_metrics) {
            return;
        }

        let mut deadline = C::now() + self.step_down_delay;
        loop {
            let closed = {
                let sleep = pin!(C::sleep_until(deadline));
                let changed = pin!(self.rx_server_metrics.changed());
                matches!(select(sleep, changed).await, Either::Right((Err(_), _)))
            };
            if closed {
                tracing::info!("RaftCore terminated during the step-down delay, quit step_down_watcher");
                return;
            }

            // Re-check after any wake-up: a change and the deadline may come at once.
            let latest = self.rx_server_metrics.borrow_watched().clone();
            if !is_removed_leader(&latest) {
                return;
            }
            if !is_same_removal(&server_metrics, &latest) {
                tracing::info!("another removal of this Leader is observed, restart the step-down delay");
                server_metrics = latest;
                deadline = C::now() + self.step_down_delay;
                continue;
            }
            if C::now() >= deadline {
                break;
            }
        }

        let metrics = self.rx_metrics.borrow_watched().clone();
        let target = transfer_target(&metrics);

        if let Some(to) = target {
            tracing::info!("removed Leader steps down: transfer leadership to {}", to);
            self.send(ExternalCommand::TriggerTransferLeader { to }).await;
        }

        C::sleep(self.transfer_wait).await;

        // In the fence a `None` membership log id means skipping the membership check, thus it
        // must be `Some` here. It always holds: this node is a Leader, hence the cluster is
        // initialized, hence the membership config comes from a log entry that has a log id.
        debug_assert!(
            server_metrics.membership_config.log_id().is_some(),
            "the membership config of a Leader must have a log id"
        );

        let cmd = ExternalCommand::RefreshServerState {
            vote: Some(server_metrics.vote.clone()),
            membership_log_id: server_metrics.membership_config.log_id().clone(),
        };
        self.send(cmd).await;
    }

    /// Send an external command to `RaftCore`.
    ///
    /// A failure to upgrade the weak sender or to send means `RaftCore` is terminated. It is
    /// safe to ignore it here: the watch loop quits at the next `changed()` call in this case.
    async fn send(&self, cmd: ExternalCommand<C>) {
        let Some(tx_api) = self.tx_api.upgrade() else {
            return;
        };

        let _ = tx_api.send(RaftMsg::ExternalCommand { cmd }).await;
    }
}

/// Decide whether this node is a Leader that is removed from a committed membership config.
///
/// The decision is level-based: it is derived from a single metrics snapshot, so skipped or
/// stale snapshots do not affect correctness.
fn is_removed_leader<C>(metrics: &RaftServerMetrics<C>) -> bool
where C: RaftTypeConfig {
    if metrics.state != ServerState::Leader {
        return false;
    }

    // A Leader that is still in the membership config, as a voter or a learner, keeps leading.
    if metrics.membership_config.membership().contains(&metrics.id) {
        return false;
    }

    // The Leader keeps leading until the membership config that removes it is committed,
    // because it is the one responsible for replicating this config.
    // The committed membership config equals the effective one iff the latter is committed.
    metrics.committed_membership_config == metrics.membership_config
}

/// Decide whether two snapshots observe the same removal of this Leader: the same vote, and the
/// same committed membership config, which removes it.
fn is_same_removal<C>(a: &RaftServerMetrics<C>, b: &RaftServerMetrics<C>) -> bool
where C: RaftTypeConfig {
    a.vote == b.vote && a.committed_membership_config.log_id() == b.committed_membership_config.log_id()
}

/// Choose the transfer-leader target: the voter with the greatest matching log id, which needs
/// the least catching up.
fn transfer_target<C>(metrics: &RaftMetrics<C>) -> Option<C::NodeId>
where C: RaftTypeConfig {
    // Only a Leader publishes replication metrics.
    let replication = metrics.replication.as_ref()?;

    let matching = |id: &C::NodeId| replication.get(id).cloned().flatten();

    metrics.membership_config.voter_ids().max_by_key(matching)
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::time::Duration;

    use maplit::btreemap;
    use maplit::btreeset;

    use super::StepDownWatcher;
    use super::is_removed_leader;
    use super::transfer_target;
    use crate::Config;
    use crate::Membership;
    use crate::StepDownPolicy;
    use crate::Vote;
    use crate::async_runtime::MpscReceiver;
    use crate::async_runtime::MpscSender;
    use crate::async_runtime::watch::WatchSender;
    use crate::core::ServerState;
    use crate::core::raft_msg::RaftMsg;
    use crate::core::raft_msg::external_command::ExternalCommand;
    use crate::engine::testing::UTConfig;
    use crate::engine::testing::log_id;
    use crate::metrics::RaftMetrics;
    use crate::metrics::RaftServerMetrics;
    use crate::type_config::TypeConfigExt;
    use crate::type_config::alias::StoredMembershipOf;

    /// Voters 2,3; node 1 is removed.
    fn m23() -> Membership<u64, ()> {
        Membership::new_with_defaults(vec![btreeset! {2,3}], [])
    }

    fn stored(log_index: u64, mem: Membership<u64, ()>) -> Arc<StoredMembershipOf<UTConfig>> {
        Arc::new(StoredMembershipOf::<UTConfig>::new(Some(log_id(2, 1, log_index)), mem))
    }

    /// Build server metrics of a Leader node 1, with the same effective and committed
    /// membership config.
    fn server_metrics(mem: Membership<u64, ()>) -> RaftServerMetrics<UTConfig> {
        let mut m = RaftServerMetrics::new_initial(1);
        m.state = ServerState::Leader;
        m.membership_config = stored(3, mem.clone());
        m.committed_membership_config = stored(3, mem);
        m
    }

    #[test]
    fn test_is_removed_leader_not_leader() {
        let mut m = server_metrics(m23());
        m.state = ServerState::Follower;

        assert!(!is_removed_leader(&m));
    }

    #[test]
    fn test_is_removed_leader_voter_leader() {
        let m = server_metrics(Membership::new_with_defaults(vec![btreeset! {1,2,3}], []));

        assert!(!is_removed_leader(&m));
    }

    #[test]
    fn test_is_removed_leader_learner_leader() {
        let m = server_metrics(Membership::new_with_defaults(vec![btreeset! {2,3}], btreeset! {1}));

        assert!(!is_removed_leader(&m));
    }

    #[test]
    fn test_is_removed_leader_uncommitted_membership() {
        let mut m = server_metrics(m23());
        m.committed_membership_config = stored(1, Membership::new_with_defaults(vec![btreeset! {1,2}], []));

        assert!(!is_removed_leader(&m));
    }

    #[test]
    fn test_is_removed_leader_committed_membership() {
        let m = server_metrics(m23());

        assert!(is_removed_leader(&m));
    }

    #[test]
    fn test_transfer_target_not_leader() {
        let m = RaftMetrics::<UTConfig>::new_initial(1);

        assert_eq!(None, transfer_target(&m));
    }

    #[test]
    fn test_transfer_target_greatest_matching() {
        let mut m = RaftMetrics::<UTConfig>::new_initial(1);
        m.membership_config = stored(3, m23());
        m.replication = Some(btreemap! {
            2u64 => Some(log_id(2, 1, 2)),
            3u64 => Some(log_id(2, 1, 3)),
        });

        assert_eq!(Some(3), transfer_target(&m));

        m.replication = Some(btreemap! {
            2u64 => Some(log_id(2, 1, 3)),
            3u64 => None,
        });

        assert_eq!(Some(2), transfer_target(&m));
    }

    /// Server metrics of Leader node 1 with the vote of `term`, whose effective and committed
    /// membership config is `mem` at `log_index`.
    fn leader_at(term: u64, log_index: u64, mem: Membership<u64, ()>) -> RaftServerMetrics<UTConfig> {
        let mut m = server_metrics(mem.clone());
        m.vote = Vote::new_committed(term, 1);
        m.membership_config = stored(log_index, mem.clone());
        m.committed_membership_config = stored(log_index, mem);
        m
    }

    /// Run `body` on a current-thread runtime with a paused clock, which moves only when every
    /// task waits for a timer. It fails if `body` does not finish within a minute of that clock.
    ///
    /// `body` runs in a `LocalSet`, which `C::spawn()` needs with the `single-threaded` feature.
    fn run_paused<F>(body: F)
    where F: Future<Output = ()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .expect("build a current-thread runtime with a paused clock");
        let local = tokio::task::LocalSet::new();
        let finished = local.block_on(&runtime, async {
            tokio::time::timeout(Duration::from_secs(60), body).await
        });
        finished.expect("the test body hung");
    }

    /// Run the watcher with `After(1000)` and a 50 ms `heartbeat_interval`. Node 1 is a removed
    /// Leader at 0 ms, in term 2 at membership log index 3, and the server metrics change to each
    /// of `updates` at its time in ms. No replication metrics name a transfer target, so the only
    /// command is the fallback: it must come at `fallback_at` ms, fenced by the vote of `term` and
    /// the membership config at `log_index`, and no other command may follow.
    fn assert_fallback_at(
        updates: Vec<(u64, RaftServerMetrics<UTConfig>)>,
        fallback_at: u64,
        term: u64,
        log_index: u64,
    ) {
        run_paused(async move {
            let config = Config {
                removed_leader_step_down: StepDownPolicy::After(1_000),
                heartbeat_interval: 50,
                ..Default::default()
            };
            let start = tokio::time::Instant::now();
            let at = |ms: u64| start + Duration::from_millis(ms);

            let (tx_server, rx_server) = UTConfig::<()>::watch_channel(leader_at(2, 3, m23()));
            let (_tx_metrics, rx_metrics) = UTConfig::<()>::watch_channel(RaftMetrics::new_initial(1));
            let (tx_api, mut rx_api) = UTConfig::<()>::mpsc::<RaftMsg<UTConfig>>(16);
            StepDownWatcher::<UTConfig>::spawn(rx_server, rx_metrics, tx_api.downgrade(), &config);

            for (ms, metrics) in updates {
                tokio::time::sleep_until(at(ms)).await;
                tx_server.send(metrics).unwrap();
            }

            tokio::time::sleep_until(at(fallback_at - 1)).await;
            assert!(rx_api.try_recv().is_err(), "no command before {fallback_at} ms");

            tokio::time::sleep_until(at(fallback_at + 1)).await;
            let msg = rx_api.try_recv().expect("the fallback command");
            let RaftMsg::ExternalCommand {
                cmd:
                    ExternalCommand::RefreshServerState {
                        vote,
                        membership_log_id,
                    },
            } = msg
            else {
                panic!("expected the fallback command, got: {msg}");
            };
            assert_eq!(Some(Vote::new_committed(term, 1)), vote);
            assert_eq!(Some(log_id(2, 1, log_index)), membership_log_id);

            tokio::time::sleep_until(at(fallback_at + 5_000)).await;
            assert!(rx_api.try_recv().is_err(), "no other command");
        });
    }

    #[test]
    fn test_step_down_waits_the_delay_of_the_removal() {
        assert_fallback_at(vec![], 1_050, 2, 3);
    }

    /// A metrics update that keeps the vote and the removing membership config keeps the deadline.
    #[test]
    fn test_unrelated_update_keeps_the_delay() {
        let mut update = leader_at(2, 3, m23());
        update.current_leader = Some(1);
        assert_fallback_at(vec![(600, update)], 1_050, 2, 3);
    }

    /// A readmission ends the first removal's delay, and the second removal waits its own.
    #[test]
    fn test_second_removal_after_readmission_waits_its_own_delay() {
        let readmitted = leader_at(
            2,
            4,
            Membership::new_with_defaults(vec![btreeset! {2,3}], btreeset! {1}),
        );
        let removed_again = leader_at(2, 5, m23());
        assert_fallback_at(vec![(300, readmitted), (600, removed_again)], 1_650, 2, 5);
    }

    /// A second removal waits its own delay even if the watcher never sees the readmission between
    /// them, because the removing membership config changed.
    #[test]
    fn test_coalesced_second_removal_waits_its_own_delay() {
        assert_fallback_at(vec![(600, leader_at(2, 5, m23()))], 1_650, 2, 5);
    }

    /// The same removal under a new vote waits a delay of its own.
    #[test]
    fn test_removal_under_a_new_vote_waits_its_own_delay() {
        assert_fallback_at(vec![(600, leader_at(3, 3, m23()))], 1_650, 3, 3);
    }
}
