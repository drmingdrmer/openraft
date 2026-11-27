use std::error::Error;
use std::fmt;

use display_more::DisplayOptionExt;
use validit::Valid;
use validit::Validate;
use validit::less_equal;

use crate::RaftTypeConfig;
use crate::raft_state::IOId;
use crate::raft_state::io_state::io_progress::IOProgress;
use crate::raft_state::io_state::log_io_id::LogIOId;
use crate::raft_state::io_state::monotonic::MonotonicIncrease;
use crate::type_config::alias::LogIdOf;
use crate::type_config::alias::VoteOf;

pub(crate) mod io_id;
pub(crate) mod io_progress;
pub(crate) mod log_io_id;
pub(crate) mod monotonic;

/// Tracks vote and log I/O, state-machine application, snapshot persistence, and log purges.
/// Also records whether a snapshot is being built.
///
/// Each kind of I/O has three cumulative progress boundaries:
/// - **accepted**: How far Raft has accepted work for execution, including work still queued.
/// - **submitted**: How far work has been sent to storage or the state-machine worker.
/// - **flushed**: How far work has completed. Votes and log appends are durable at this stage, but
///   a transient state machine need not persist the effects of applying entries.
///
/// Within each [`IOProgress`], `flushed <= submitted <= accepted` in the order of its I/O IDs.
/// The snapshot submission boundary cannot exceed the apply submission boundary, and completed
/// purging cannot pass the completed snapshot.
///
/// Local commitment (`apply_progress.accepted`) can advance while log I/O is still queued.
/// Moreover, a vote-only log I/O can have no log endpoint even when the node has log entries,
/// so the order of log I/O IDs does not imply the same order for their optional log endpoints.
#[derive(Debug, Clone)]
#[derive(PartialEq, Eq)]
pub(crate) struct IOState<C>
where C: RaftTypeConfig
{
    /// Whether it is building a snapshot
    building_snapshot: bool,

    /// Tracks log I/O progress to local storage (vote + log entries).
    ///
    /// Uses `IOProgress<IOId>` to track both non-committed vote I/O and log I/O.
    /// See: [`IOId`](crate::docs::data::io_id)
    pub(crate) log_progress: Valid<IOProgress<IOId<C>>>,

    /// Tracks applying committed logs to state machine.
    ///
    /// - `accepted`: The committed log id, safe to apply
    /// - `submitted`: Sent to state machine task to apply
    /// - `flushed`: Already applied to state machine
    ///
    /// Uses `IOProgress<LogId>` since only committed logs are applied.
    pub(crate) apply_progress: Valid<IOProgress<LogIdOf<C>>>,

    /// Tracks snapshot persistence progress.
    ///
    /// - `accepted`: Snapshot covering this log id should exist
    /// - `submitted`: Snapshot submitted to persist
    /// - `flushed`: Snapshot successfully persisted
    ///
    /// Tracks both locally built snapshots and snapshots installed from the leader.
    pub(crate) snapshot: Valid<IOProgress<LogIdOf<C>>>,

    /// The reported cluster commit bound, which can differ from the local apply bound.
    ///
    /// [`LogIOId`] pairs the reporting leader's `CommittedVote` with an optional `LogId`.
    /// Reports are ordered by vote first, so a higher vote can replace the stored report with a
    /// lower or absent log endpoint. The report is not restored from storage on restart.
    ///
    /// The leader derives this bound from quorum replication. A node receiving an append request
    /// considers the reported bound even if it rejects the append, so the reporting vote need not
    /// match the locally accepted I/O vote. See [`Self::calculate_local_committed`] for how the
    /// report can advance local commitment.
    pub(crate) cluster_committed: MonotonicIncrease<LogIOId<C>>,

    /// Tracks progress of purging log entries from storage.
    ///
    /// - `accepted`: Purge accepted for execution
    /// - `submitted`: Purge submitted to storage
    /// - `flushed`: Log entries purged from storage
    pub(crate) purged: Valid<IOProgress<LogIdOf<C>>>,
}

const LOG_PROGRESS_NAME: &str = "LogIO";
const APPLY_PROGRESS_NAME: &str = "Apply";
const SNAPSHOT_PROGRESS_NAME: &str = "Snapshot";
const PURGE_PROGRESS_NAME: &str = "Purge";

impl<C> Default for IOState<C>
where C: RaftTypeConfig
{
    fn default() -> Self {
        Self {
            building_snapshot: false,
            log_progress: new_progress(None, "xx", LOG_PROGRESS_NAME),
            apply_progress: new_progress(None, "xx", APPLY_PROGRESS_NAME),
            snapshot: new_progress(None, "xx", SNAPSHOT_PROGRESS_NAME),
            cluster_committed: MonotonicIncrease::default(),
            purged: new_progress(None, "xx", PURGE_PROGRESS_NAME),
        }
    }
}

impl<C> Validate for IOState<C>
where C: RaftTypeConfig
{
    fn validate(&self) -> Result<(), Box<dyn Error>> {
        self.log_progress.validate()?;
        self.apply_progress.validate()?;

        // Disable this check, because IOId.log_id is None when a Vote request is just accepted(updated to
        // non-None when appendEntries are received):
        //
        // less_equal!(
        //     self.apply_progress.submitted(),
        //     self.log_progress.submitted().and_then(|x| x.last_log_id())
        // );

        self.snapshot.validate()?;
        // Snapshot must be included in applied.
        less_equal!(self.snapshot.submitted(), self.apply_progress.submitted());

        self.purged.validate()?;
        less_equal!(self.purged.flushed(), self.snapshot.flushed());
        Ok(())
    }
}

impl<C> IOState<C>
where C: RaftTypeConfig
{
    /// Creates a new `IOState` with initial values.
    pub(crate) fn new(
        id: &str,
        vote: &VoteOf<C>,
        applied: Option<LogIdOf<C>>,
        snapshot: Option<LogIdOf<C>>,
        purged: Option<LogIdOf<C>>,
    ) -> Self {
        Self {
            building_snapshot: false,
            log_progress: new_progress(Some(IOId::new(vote)), id, LOG_PROGRESS_NAME),
            apply_progress: new_progress(applied, id, APPLY_PROGRESS_NAME),
            snapshot: new_progress(snapshot, id, SNAPSHOT_PROGRESS_NAME),
            cluster_committed: MonotonicIncrease::default(),
            purged: new_progress(purged, id, PURGE_PROGRESS_NAME),
        }
    }

    pub(crate) fn applied(&self) -> Option<&LogIdOf<C>> {
        self.apply_progress.flushed()
    }

    pub(crate) fn snapshot(&self) -> Option<&LogIdOf<C>> {
        self.snapshot.flushed()
    }

    pub(crate) fn set_building_snapshot(&mut self, building: bool) {
        self.building_snapshot = building;
    }

    pub(crate) fn building_snapshot(&self) -> bool {
        self.building_snapshot
    }

    /// Calculates a candidate local commit bound for state-machine application.
    ///
    /// Returns the lesser of the reported committed log ID and the accepted log endpoint if the
    /// accepted I/O carries a committed vote whose committed leader ID is at least both the
    /// reporting vote's committed leader ID and the reported entry's committed leader ID.
    ///
    /// Returns `None` if the report or accepted I/O is missing, either log endpoint is absent,
    /// the accepted vote is uncommitted, or either leader ID comparison fails. The caller leaves
    /// local commitment unchanged unless the calculation yields a higher bound, which can happen
    /// once the accepted I/O catches up to the report.
    ///
    /// This bound determines eligibility, not when application can be submitted. The engine
    /// submits application only up to the submitted log endpoint, with matching submitted and
    /// accepted log I/O votes, and after any accepted snapshot has been submitted to the state
    /// machine. It does not wait for local log flush: [`RaftLogStorage::append`] makes entries
    /// readable on return, independently of when they become durable.
    ///
    /// [`RaftLogStorage::append`]: crate::storage::RaftLogStorage::append
    pub(crate) fn calculate_local_committed(&self) -> Option<LogIdOf<C>> {
        let local_committed = self.do_calculate_local_committed();

        tracing::debug!(
            "{}, cluster_committed: {}, accepted: {}, local_committed: {}",
            func_name!(),
            self.cluster_committed.value().display(),
            self.log_progress.accepted().display(),
            local_committed.display()
        );

        local_committed
    }

    fn do_calculate_local_committed(&self) -> Option<LogIdOf<C>> {
        let cluster_committed = self.cluster_committed.value()?;
        let accepted = self.log_progress.accepted()?;

        let reporting_leader_id = cluster_committed.committed_vote().committed_leader_id();
        let committed_log_id = cluster_committed.last_log_id()?;
        let committed_log_leader_id = committed_log_id.committed_leader_id();

        // Normally, the committed log ID's vote never exceeds the vote carried by LogIOId.
        // Take their maximum to guard against mistakes such as a stale stream pairing an old vote
        // with a newer commit, which could otherwise apply an overwritten entry.
        let required_leader_id = std::cmp::max(&reporting_leader_id, committed_log_leader_id);
        let accepted_leader_id = accepted.committed_leader_id()?;

        // Below the required leader ID, an in-flight RPC may truncate and re-append entries
        // before the state machine can read them. Once the accepted leader ID reaches both
        // bounds, enqueued I/O cannot truncate the committed prefix.
        if &accepted_leader_id >= required_leader_id {
            let accepted_log_id = accepted.last_log_id()?;
            Some(std::cmp::min(accepted_log_id, committed_log_id).clone())
        } else {
            None
        }
    }
}

/// Creates a new `IOProgress` wrapped in `Valid`.
///
/// All three stages (accepted, submitted, flushed) are initialized to `initial_value`.
fn new_progress<T>(initial_value: Option<T>, id: impl ToString, name: &'static str) -> Valid<IOProgress<T>>
where T: PartialOrd + fmt::Debug + fmt::Display + Clone {
    Valid::new(IOProgress::new_synchronized(initial_value, id, name))
}

#[cfg(test)]
mod tests {
    use super::IOState;
    use crate::engine::testing::UTConfig;
    use crate::engine::testing::log_id;
    use crate::raft_state::IOId;
    use crate::raft_state::io_state::log_io_id::LogIOId;
    use crate::type_config::alias::VoteOf;
    use crate::vote::raft_vote::RaftVoteExt;

    #[test]
    fn test_local_committed_requires_both_votes() {
        for (commit_term, log_term, accepted_term, expected) in [
            (1, 3, 1, None),
            (3, 1, 1, None),
            (1, 3, 3, Some(log_id(3, 0, 13))),
            (3, 1, 3, Some(log_id(1, 0, 13))),
        ] {
            let mut state = IOState::<UTConfig>::default();
            let accepted_vote = VoteOf::<UTConfig>::new_committed(accepted_term, 0).to_committed();
            let accepted = IOId::new_log_io(accepted_vote, Some(log_id(accepted_term, 0, 14)));
            state.log_progress.accept(accepted);
            let commit_vote = VoteOf::<UTConfig>::new_committed(commit_term, 0).to_committed();
            let committed = LogIOId::new(commit_vote, Some(log_id(log_term, 0, 13)));
            state.cluster_committed.try_update(committed).unwrap();

            let actual = state.calculate_local_committed();
            assert_eq!(expected, actual);
        }
    }

    #[test]
    fn test_purged_tracks_pending_io() {
        let mut state = IOState::<UTConfig>::default();
        let completed = log_id(1, 0, 2);
        let submitted = log_id(1, 0, 3);
        let accepted = log_id(1, 0, 4);
        state.purged.try_update_all(completed);
        state.purged.accept(accepted);
        state.purged.submit(submitted);
        let purged = state.purged.flushed();
        assert_eq!(Some(&completed), purged);

        state.purged.flush(submitted);
        let purged = state.purged.flushed();
        assert_eq!(Some(&submitted), purged);
        let actual_accepted = state.purged.accepted();
        assert_eq!(Some(&accepted), actual_accepted);
    }
}
