use std::marker::PhantomData;
#[cfg(feature = "runtime-stats")]
use std::time::Duration;

#[cfg(feature = "runtime-stats")]
use base2histogram::Histogram;
use openraft_macros::since;

use crate::Config;
#[cfg(feature = "runtime-stats")]
use crate::Instant;
use crate::RaftTypeConfig;
use crate::core::NotificationName;
use crate::core::raft_msg::RaftMsgName;
#[cfg(feature = "runtime-stats")]
use crate::core::runtime_stats::DisplayMode;
#[cfg(feature = "runtime-stats")]
use crate::core::runtime_stats::RuntimeStatsDisplay;
#[cfg(feature = "runtime-stats")]
use crate::core::runtime_stats::log_stage::LogStageHistograms;
#[cfg(feature = "runtime-stats")]
use crate::core::runtime_stats::log_stage::LogStages;
use crate::core::stage::Stage;
use crate::engine::CommandName;
#[cfg(feature = "runtime-stats")]
use crate::type_config::TypeConfigExt;
use crate::type_config::alias::InstantOf;

/// Runtime statistics for Raft operations.
///
/// This is a volatile structure that is not persisted. It accumulates
/// statistics from the time the Raft node starts.
///
/// Without the `runtime-stats` feature, nothing can read the statistics, so the structure holds
/// no field and its recording methods do nothing: a node allocates and updates no statistic.
#[since(version = "0.10.2", change = "Add the re-arm wake and metrics send counters")]
#[since(version = "0.10.0", change = "Add core_busy_histogram")]
#[since(version = "0.10.0", change = "Add core_busy_duration snapshot")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStats<C>
where C: RaftTypeConfig
{
    /// Cumulative wall-clock time spent processing work in the core event loop.
    ///
    /// Includes awaits and scheduler delays during processing, but excludes the
    /// top-level wait for incoming events. This measures loop occupancy, not CPU time.
    /// Updated before the core waits for incoming events. A snapshot includes
    /// completed processing intervals; the current interval is included after
    /// the core next reaches that wait. Fetching a snapshot requires core progress.
    #[cfg(feature = "runtime-stats")]
    #[since(version = "0.10.0")]
    pub core_busy_duration: Duration,

    /// Distribution of wall-clock time, in microseconds, of one core-loop turn.
    ///
    /// Each sample is one completed interval added to [`Self::core_busy_duration`].
    /// The turn in progress is recorded when the loop next reaches the wait.
    #[cfg(feature = "runtime-stats")]
    #[since(version = "0.10.0")]
    pub core_busy_histogram: Histogram,

    /// Histogram of the number of log entries the state machine receives in one `apply()` call.
    ///
    /// One sample is recorded per [`RaftStateMachine::apply()`] call. The state-machine worker
    /// merges queued apply commands into one batch, so a sample may cover several commands; it
    /// shows the batch size the state machine's I/O actually sees.
    ///
    /// [`RaftStateMachine::apply()`]: crate::storage::RaftStateMachine::apply
    #[cfg(feature = "runtime-stats")]
    #[since(version = "0.10.1", change = "one sample per `apply()` call, not per command")]
    pub apply_batch: Histogram,

    /// Histogram tracking the distribution of log entry counts when appending to storage.
    ///
    /// This tracks how many log entries are included in each AppendEntries command
    /// submitted to the storage layer, helping identify write batch patterns and storage I/O
    /// efficiency.
    #[cfg(feature = "runtime-stats")]
    pub append_batch: Histogram,

    /// Histogram tracking the distribution of log entry counts in replication RPCs.
    ///
    /// This tracks how many log entries are included in each AppendEntries RPC
    /// sent to followers during replication, helping identify replication batch patterns.
    #[cfg(feature = "runtime-stats")]
    pub replicate_batch: Histogram,

    /// Histogram tracking the distribution of RaftMsg counts processed in a batch.
    ///
    /// This tracks how many RaftMsg are processed together before calling
    /// `run_engine_commands()`, helping identify message batching efficiency.
    #[cfg(feature = "runtime-stats")]
    pub raft_msg_per_run: Histogram,

    /// Histogram tracking the distribution of client write entries merged per batch.
    ///
    /// This tracks how many client write entries are merged together in each
    /// `RaftMsg::ClientWrite` after batching, helping identify batching efficiency.
    #[cfg(feature = "runtime-stats")]
    pub write_batch: Histogram,

    /// Histogram tracking the budget for RaftMsg processing.
    ///
    /// This tracks the maximum number of RaftMsg allowed to process in each
    /// `process_raft_msg()` call, helping understand load balancing behavior.
    #[cfg(feature = "runtime-stats")]
    pub raft_msg_budget: Histogram,

    /// Histogram tracking the budget for Notification processing.
    ///
    /// This tracks the maximum number of Notifications allowed to process in each
    /// `process_notification()` call, helping understand load balancing behavior.
    #[cfg(feature = "runtime-stats")]
    pub notification_budget: Histogram,

    /// Histogram tracking RaftMsg budget utilization in permille (0-1000).
    ///
    /// This tracks `processed * 1000 / budget` for each `process_raft_msg()` call,
    /// where 1000 means 100% utilization. Helps identify if the budget is well-tuned.
    #[cfg(feature = "runtime-stats")]
    pub raft_msg_usage_permille: Histogram,

    /// Histogram tracking Notification budget utilization in permille (0-1000).
    ///
    /// This tracks `processed * 1000 / budget` for each `process_notification()` call,
    /// where 1000 means 100% utilization. Helps identify if the budget is well-tuned.
    #[cfg(feature = "runtime-stats")]
    pub notification_usage_permille: Histogram,

    /// Count of each command type executed.
    ///
    /// This tracks how many times each command type has been executed,
    /// useful for understanding workload patterns and debugging.
    /// Indexed by `CommandName::index()`.
    #[cfg(feature = "runtime-stats")]
    pub command_counts: Vec<u64>,

    /// Count of each RaftMsg type received.
    ///
    /// This tracks how many times each RaftMsg type has been received,
    /// useful for understanding API usage patterns and debugging.
    /// Indexed by `stats::RaftMsgName::index()`.
    #[cfg(feature = "runtime-stats")]
    pub raft_msg_counts: Vec<u64>,

    /// Count of each Notification type received.
    ///
    /// This tracks how many times each Notification type has been received,
    /// useful for understanding internal message patterns and debugging.
    /// Indexed by `NotificationName::index()`.
    #[cfg(feature = "runtime-stats")]
    pub notification_counts: Vec<u64>,

    /// Count of the times `RaftCore` woke the heartbeat timer's task to re-arm it.
    ///
    /// `RaftCore` wakes the task when it sets a deadline the task could otherwise sleep past: one
    /// earlier than the stored deadline, or any deadline when the stored one has passed or is
    /// `None`. The deadline set after a heartbeat fires is one such wake, because the stored
    /// deadline has just passed. The fires themselves are the `NotificationName::Tick` entries of
    /// [`Self::notification_counts`].
    ///
    /// A deadline moved later is stored without a wake. The task reads it when its timer expires
    /// at the stored deadline, and that wake is not counted.
    #[cfg(feature = "runtime-stats")]
    #[since(version = "0.10.2")]
    pub heartbeat_rearm_wakes: u64,

    /// Count of the times `RaftCore` woke the election timer's task to re-arm it.
    ///
    /// The fires of this timer are the `NotificationName::ElectionDeadlineReached` entries of
    /// [`Self::notification_counts`]. See [`Self::heartbeat_rearm_wakes`] for when `RaftCore`
    /// wakes a timer's task.
    #[cfg(feature = "runtime-stats")]
    #[since(version = "0.10.2")]
    pub election_rearm_wakes: u64,

    /// Count of the times `RaftCore` woke the pending-read timer's task to re-arm it.
    ///
    /// The fires of this timer are the `NotificationName::PendingReadDeadlineReached` entries of
    /// [`Self::notification_counts`]. See [`Self::heartbeat_rearm_wakes`] for when `RaftCore`
    /// wakes a timer's task.
    #[cfg(feature = "runtime-stats")]
    #[since(version = "0.10.2")]
    pub pending_read_rearm_wakes: u64,

    /// Count of the `RaftMetrics` sends: one per metrics report, whether or not a field changed.
    #[cfg(feature = "runtime-stats")]
    #[since(version = "0.10.2")]
    pub metrics_sends: u64,

    /// Count of the `RaftDataMetrics` sends: the metrics reports that changed one of its fields.
    #[cfg(feature = "runtime-stats")]
    #[since(version = "0.10.2")]
    pub data_metrics_sends: u64,

    /// Count of the `RaftServerMetrics` sends: the metrics reports that changed one of its fields.
    #[cfg(feature = "runtime-stats")]
    #[since(version = "0.10.2")]
    pub server_metrics_sends: u64,

    /// Per-entry timestamp tracking across 6 lifecycle stages
    /// (Proposed, Received, Submitted, Persisted, Committed, Applied).
    ///
    /// Uses a fixed-capacity ring buffer per stage.
    /// Stage-to-stage gaps reveal where latency accumulates
    /// (channel queueing, storage I/O, replication, state machine apply).
    #[cfg(feature = "runtime-stats")]
    pub log_stage: LogStages<InstantOf<C>>,

    /// Precomputed stage-to-stage duration histograms derived from [`log_stage`](Self::log_stage).
    ///
    /// Rebuilt on demand via [`build_log_stage_histograms()`](Self::build_log_stage_histograms).
    #[cfg(feature = "runtime-stats")]
    pub log_stage_histograms: LogStageHistograms,

    _phantom: PhantomData<C>,
}

impl<C> Default for RuntimeStats<C>
where C: RaftTypeConfig
{
    fn default() -> Self {
        Self::new(&Config::default(), 0)
    }
}

impl<C> RuntimeStats<C>
where C: RaftTypeConfig
{
    /// Create the statistics of a node whose state machine has applied the logs before
    /// `log_stage_begin`, such as at startup: their stages were not observed, so the log stages
    /// start at that index.
    #[cfg(feature = "runtime-stats")]
    pub(crate) fn new(config: &Config, log_stage_begin: u64) -> Self {
        Self {
            core_busy_duration: Duration::ZERO,
            core_busy_histogram: Histogram::<()>::new(),
            apply_batch: Histogram::<()>::new(),
            append_batch: Histogram::<()>::new(),
            replicate_batch: Histogram::<()>::new(),
            raft_msg_per_run: Histogram::<()>::new(),
            write_batch: Histogram::<()>::new(),
            raft_msg_budget: Histogram::<()>::new(),
            notification_budget: Histogram::<()>::new(),
            raft_msg_usage_permille: Histogram::<()>::new(),
            notification_usage_permille: Histogram::<()>::new(),
            command_counts: vec![0; CommandName::COUNT],
            raft_msg_counts: vec![0; RaftMsgName::COUNT],
            notification_counts: vec![0; NotificationName::COUNT],
            heartbeat_rearm_wakes: 0,
            election_rearm_wakes: 0,
            pending_read_rearm_wakes: 0,
            metrics_sends: 0,
            data_metrics_sends: 0,
            server_metrics_sends: 0,
            log_stage: LogStages::new(config.log_stage_capacity(), log_stage_begin),
            log_stage_histograms: LogStageHistograms::new(),
            _phantom: PhantomData,
        }
    }

    /// Create the statistics of a node: without the `runtime-stats` feature, an empty value.
    #[cfg(not(feature = "runtime-stats"))]
    pub(crate) fn new(config: &Config, log_stage_begin: u64) -> Self {
        let _ = (config, log_stage_begin);
        Self { _phantom: PhantomData }
    }

    /// Start one core-loop busy interval.
    ///
    /// Returns `None` when the `runtime-stats` feature is off.
    #[cfg(feature = "runtime-stats")]
    pub(crate) fn begin_core_busy(&self) -> Option<InstantOf<C>> {
        Some(C::now())
    }

    /// Start one core-loop busy interval.
    #[cfg(not(feature = "runtime-stats"))]
    pub(crate) fn begin_core_busy(&self) -> Option<InstantOf<C>> {
        None
    }

    /// Add one core-loop busy interval to [`Self::core_busy_duration`] and
    /// [`Self::core_busy_histogram`].
    ///
    /// The histogram sample is the elapsed time since `since`, in microseconds.
    /// `None` records nothing.
    pub(crate) fn end_core_busy(&mut self, since: Option<InstantOf<C>>) {
        #[cfg(feature = "runtime-stats")]
        {
            let Some(since) = since else {
                return;
            };
            let now = C::now();
            let elapsed = now.saturating_duration_since(since);
            self.core_busy_duration += elapsed;
            let micros = elapsed.as_micros() as u64;
            self.core_busy_histogram.record(micros);
        }

        #[cfg(not(feature = "runtime-stats"))]
        {
            let _ = since;
        }
    }

    /// Record the execution of a command.
    pub fn record_command(&mut self, name: CommandName) {
        #[cfg(feature = "runtime-stats")]
        {
            self.command_counts[name.index()] += 1;
        }
        #[cfg(not(feature = "runtime-stats"))]
        let _ = name;
    }

    /// Record the receipt of a RaftMsg.
    pub fn record_raft_msg(&mut self, name: RaftMsgName) {
        #[cfg(feature = "runtime-stats")]
        {
            self.raft_msg_counts[name.index()] += 1;
        }
        #[cfg(not(feature = "runtime-stats"))]
        let _ = name;
    }

    /// Record the receipt of a Notification.
    pub fn record_notification(&mut self, name: NotificationName) {
        #[cfg(feature = "runtime-stats")]
        {
            self.notification_counts[name.index()] += 1;
        }
        #[cfg(not(feature = "runtime-stats"))]
        let _ = name;
    }

    /// Record the budget of one `process_raft_msg()` call.
    pub(crate) fn record_raft_msg_budget(&mut self, budget: u64) {
        #[cfg(feature = "runtime-stats")]
        self.raft_msg_budget.record(budget);
        #[cfg(not(feature = "runtime-stats"))]
        let _ = budget;
    }

    /// Record the number of RaftMsg processed before one run of the engine commands.
    pub(crate) fn record_raft_msg_per_run(&mut self, processed: u64) {
        #[cfg(feature = "runtime-stats")]
        self.raft_msg_per_run.record(processed);
        #[cfg(not(feature = "runtime-stats"))]
        let _ = processed;
    }

    /// Record the RaftMsg budget utilization of one drain, in permille.
    pub(crate) fn record_raft_msg_usage_permille(&mut self, permille: u64) {
        #[cfg(feature = "runtime-stats")]
        self.raft_msg_usage_permille.record(permille);
        #[cfg(not(feature = "runtime-stats"))]
        let _ = permille;
    }

    /// Record the budget of one `process_notification()` call.
    pub(crate) fn record_notification_budget(&mut self, budget: u64) {
        #[cfg(feature = "runtime-stats")]
        self.notification_budget.record(budget);
        #[cfg(not(feature = "runtime-stats"))]
        let _ = budget;
    }

    /// Record the Notification budget utilization of one drain, in permille.
    pub(crate) fn record_notification_usage_permille(&mut self, permille: u64) {
        #[cfg(feature = "runtime-stats")]
        self.notification_usage_permille.record(permille);
        #[cfg(not(feature = "runtime-stats"))]
        let _ = permille;
    }

    /// Record the number of client write entries merged into one write.
    pub(crate) fn record_write_batch(&mut self, entries: u64) {
        #[cfg(feature = "runtime-stats")]
        self.write_batch.record(entries);
        #[cfg(not(feature = "runtime-stats"))]
        let _ = entries;
    }

    /// Record the number of log entries one `apply()` call receives.
    pub(crate) fn record_apply_batch(&mut self, entries: u64) {
        #[cfg(feature = "runtime-stats")]
        self.apply_batch.record(entries);
        #[cfg(not(feature = "runtime-stats"))]
        let _ = entries;
    }

    /// Record the number of log entries one storage append receives.
    pub(crate) fn record_append_batch(&mut self, entries: u64) {
        #[cfg(feature = "runtime-stats")]
        self.append_batch.record(entries);
        #[cfg(not(feature = "runtime-stats"))]
        let _ = entries;
    }

    /// Count one `RaftMetrics` send.
    pub(crate) fn count_metrics_send(&mut self) {
        #[cfg(feature = "runtime-stats")]
        {
            self.metrics_sends += 1;
        }
    }

    /// Count one `RaftDataMetrics` send.
    pub(crate) fn count_data_metrics_send(&mut self) {
        #[cfg(feature = "runtime-stats")]
        {
            self.data_metrics_sends += 1;
        }
    }

    /// Count one `RaftServerMetrics` send.
    pub(crate) fn count_server_metrics_send(&mut self) {
        #[cfg(feature = "runtime-stats")]
        {
            self.server_metrics_sends += 1;
        }
    }

    /// Count one wake of the heartbeat timer's task to re-arm it.
    pub(crate) fn count_heartbeat_rearm_wake(&mut self) {
        #[cfg(feature = "runtime-stats")]
        {
            self.heartbeat_rearm_wakes += 1;
        }
    }

    /// Count one wake of the election timer's task to re-arm it.
    pub(crate) fn count_election_rearm_wake(&mut self) {
        #[cfg(feature = "runtime-stats")]
        {
            self.election_rearm_wakes += 1;
        }
    }

    /// Count one wake of the pending-read timer's task to re-arm it.
    pub(crate) fn count_pending_read_rearm_wake(&mut self) {
        #[cfg(feature = "runtime-stats")]
        {
            self.pending_read_rearm_wakes += 1;
        }
    }

    /// Record that the logs before index `right` reached `stage` now.
    ///
    /// `right` is the exclusive end of the batch that newly reached `stage`: pass the index of its
    /// last log plus one. Entry 0 alone is recorded with `right = 1`, and a batch ending at index 7
    /// with `right = 8`.
    ///
    /// Without the `runtime-stats` feature, it does nothing.
    pub fn record_log_stage_now(&mut self, stage: Stage, right: u64) {
        #[cfg(feature = "runtime-stats")]
        self.record_log_stage(stage, right, C::now());

        #[cfg(not(feature = "runtime-stats"))]
        {
            let _ = (stage, right);
        }
    }

    /// Record that the logs before index `right` reached `stage` at `now`.
    ///
    /// `right` is the exclusive end of the batch, as for [`Self::record_log_stage_now`]: the index
    /// of the batch's last log plus one.
    ///
    /// Without the `runtime-stats` feature, it does nothing.
    pub fn record_log_stage(&mut self, stage: Stage, right: u64, now: InstantOf<C>) {
        #[cfg(feature = "runtime-stats")]
        self.log_stage.record_stage(stage, right, now);

        #[cfg(not(feature = "runtime-stats"))]
        {
            let _ = (stage, right, now);
        }
    }

    /// Discard the recorded log stages of the logs from index `since` on, which a truncation
    /// removed from the log.
    ///
    /// Without the `runtime-stats` feature, it does nothing.
    pub(crate) fn truncate_log_stages(&mut self, since: u64) {
        #[cfg(feature = "runtime-stats")]
        self.log_stage.truncate(since);

        #[cfg(not(feature = "runtime-stats"))]
        {
            let _ = since;
        }
    }

    /// Exclude the logs in `range` from the log stages, such as the logs an installed snapshot
    /// covers beyond the applied ones: their stages were not observed.
    ///
    /// Without the `runtime-stats` feature, it does nothing.
    pub(crate) fn exclude_log_stages(&mut self, range: std::ops::Range<u64>) {
        #[cfg(feature = "runtime-stats")]
        self.log_stage.exclude(range);

        #[cfg(not(feature = "runtime-stats"))]
        {
            let _ = range;
        }
    }

    /// Compute `log_stage_histograms` from the recorded log stages.
    ///
    /// Without the `runtime-stats` feature, it does nothing.
    pub fn build_log_stage_histograms(&mut self) {
        #[cfg(feature = "runtime-stats")]
        {
            self.log_stage_histograms = self.log_stage.compute_histograms();
        }
    }

    /// Returns a displayable representation of the runtime statistics.
    ///
    /// The percentiles are computed when calling this method, and the returned
    /// `RuntimeStatsDisplay` formats them into text each time it is displayed. It reads no clock,
    /// so it can be formatted any number of times, also after the runtime has exited.
    ///
    /// Use builder methods like `.human_readable()` to change the display mode.
    #[cfg(feature = "runtime-stats")]
    pub fn display(&self) -> RuntimeStatsDisplay<C> {
        RuntimeStatsDisplay {
            mode: DisplayMode::default(),
            core_busy_duration: self.core_busy_duration,
            core_busy_histogram: self.core_busy_histogram.percentile_stats(),
            apply_batch: self.apply_batch.percentile_stats(),
            append_batch: self.append_batch.percentile_stats(),
            replicate_batch: self.replicate_batch.percentile_stats(),
            raft_msg_per_run: self.raft_msg_per_run.percentile_stats(),
            write_batch: self.write_batch.percentile_stats(),
            raft_msg_budget: self.raft_msg_budget.percentile_stats(),
            notification_budget: self.notification_budget.percentile_stats(),
            raft_msg_usage_permille: self.raft_msg_usage_permille.percentile_stats(),
            notification_usage_permille: self.notification_usage_permille.percentile_stats(),
            command_counts: self.command_counts.clone(),
            raft_msg_counts: self.raft_msg_counts.clone(),
            notification_counts: self.notification_counts.clone(),
            log_stage_percentiles: self.log_stage_histograms.percentile_stats_array(),
            log_stage_histograms: self.log_stage_histograms.clone(),
            log_stages: self.log_stage.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    /// Without the `runtime-stats` feature nothing reads the statistics, so they hold no
    /// histogram, vector or counter, and a node allocates none.
    #[cfg(not(feature = "runtime-stats"))]
    #[test]
    fn test_disabled_stats_hold_nothing() {
        use super::RuntimeStats;
        use crate::Config;
        use crate::engine::testing::UTConfig;

        assert_eq!(0, size_of::<RuntimeStats<UTConfig>>());

        let mut stats = RuntimeStats::<UTConfig>::new(&Config::default(), 0);
        stats.record_raft_msg_budget(10);
        stats.count_metrics_send();
        assert_eq!(RuntimeStats::<UTConfig>::default(), stats);
    }
}
