use crate::LogId;
use crate::log_id::option_raft_log_id_ext::OptionRaftLogIdExt;
use crate::log_id::raft_log_id::RaftLogId;
use crate::log_id::raft_log_id_ext::RaftLogIdExt;
use crate::log_id::ref_log_id::RefLogId;
use crate::vote::RaftCommittedLeaderId;

/// Log boundaries and I/O progress in the Raft state.
///
/// Logical boundaries can advance while storage or state machine commands are still pending.
/// Use the `io_*` accessors to check completed I/O rather than inferring it from those boundaries.
///
/// See: [`log_pointers`](`crate::docs::data::log_pointers`).
pub(crate) trait LogStateReader<CLID>
where CLID: RaftCommittedLeaderId
{
    /// Get previous log id, i.e., the log id at index - 1
    fn prev_log_id(&self, index: u64) -> Option<LogId<CLID>> {
        if index == 0 { None } else { self.get_log_id(index - 1) }
    }

    /// Return if a log id exists.
    ///
    /// It assumes a committed log will always get positive return value, according to raft spec.
    fn has_log_id(&self, log_id: impl RaftLogId<CommittedLeaderId = CLID>) -> bool {
        if log_id.index() < self.committed().next_index() {
            debug_assert!(Some(log_id.to_ref()) <= self.committed().to_ref());
            return true;
        }

        // The local log id exists at the index and is same as the input.
        if let Some(local) = self.ref_log_id(log_id.index()) {
            log_id.to_ref() == local
        } else {
            false
        }
    }

    fn get_log_id(&self, index: u64) -> Option<LogId<CLID>> {
        self.ref_log_id(index).map(RefLogId::into_log_id)
    }

    /// Get the log ID at the specified index in the engine's log history.
    ///
    /// Returns the retained purge anchor at [`Self::last_purged_log_id`], and `None` for indexes
    /// below that boundary or above [`Self::last_log_id`].
    fn ref_log_id(&self, index: u64) -> Option<RefLogId<'_, CLID>>;

    /// The logical log tail tracked by the engine.
    ///
    /// An accepted append can advance this boundary before its entries are persisted. If no live
    /// entries remain, this may return the retained purge anchor ([`Self::last_purged_log_id`]).
    fn last_log_id(&self) -> Option<&LogId<CLID>>;

    /// The last known committed log id, i.e., the id of the log that is accepted by a quorum of
    /// voters.
    fn committed(&self) -> Option<&LogId<CLID>>;

    /// The last known applied log id, i.e., the id of the log that is applied to state machine.
    ///
    /// This is actually happened io-state which might fall behind committed log id.
    fn io_applied(&self) -> Option<&LogId<CLID>>;

    /// The last log ID in the latest completed snapshot.
    ///
    /// This can lag behind the engine's snapshot metadata ([`Self::snapshot_last_log_id`]).
    fn io_snapshot_last_log_id(&self) -> Option<&LogId<CLID>>;

    /// The highest log ID through which purging has completed in storage, inclusive.
    ///
    /// This can lag behind the engine's accepted purge boundary ([`Self::last_purged_log_id`]).
    fn io_purged(&self) -> Option<&LogId<CLID>>;

    /// The last log ID in the engine's snapshot metadata.
    ///
    /// The metadata can reflect an accepted installation before it completes. Use
    /// [`Self::io_snapshot_last_log_id`] to check completed snapshot progress.
    fn snapshot_last_log_id(&self) -> Option<&LogId<CLID>>;

    /// Return the log id it wants to purge up to.
    ///
    /// Logs may not be able to be purged at once because they are in use by replication tasks.
    fn purge_upto(&self) -> Option<&LogId<CLID>>;

    /// The highest log ID the engine has accepted for purging, inclusive.
    ///
    /// The engine's live log range is `(last_purged_log_id(), last_log_id()]`. Equal boundaries
    /// mean there are no logical live entries, not necessarily that storage is empty.
    ///
    /// A purge command may still be pending. Use [`Self::io_purged`] to check how far purging has
    /// completed in storage.
    fn last_purged_log_id(&self) -> Option<&LogId<CLID>>;
}
