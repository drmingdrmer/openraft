//! Data structures for EzRaft
//!
//! This module contains the core data structures used throughout EzRaft.

use std::io::Cursor;

use openraft::entry::RaftEntry;
use openraft::entry::RaftPayload;
use openraft::log_id::RaftLogId;
use openraft::vote::leader_id_std::CommittedLeaderId;
use openraft::BasicNode;
use openraft::EntryPayload;
use openraft::LogId;
use openraft::Membership;
use openraft::Snapshot;
use openraft::SnapshotMeta;
use serde::Deserialize;
use serde::Serialize;

use crate::type_config::EzTypes;
use crate::type_config::EzVote;

/// Log ID type (term, index)
///
/// A tuple that implements `RaftLogId` via OpenRaft's blanket implementation.
pub type EzLogId = (u64, u64);

/// Committed leader ID: the term of the leader that proposed a log entry
pub type EzCommittedLeaderId = CommittedLeaderId<u64>;

/// Entry payload with EzRaft's node id and node types
type EzEntryPayload<T> = EntryPayload<<T as EzTypes>::Request, u64, BasicNode>;

/// A Raft log entry with EzRaft's simplified log ID type
///
/// Wraps the entry's log ID (term, index) and payload.
/// This is the native Entry type used throughout EzRaft.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(bound = "")]
pub struct EzEntry<T>
where T: EzTypes
{
    /// Log ID (term, index)
    pub log_id: EzLogId,

    /// Entry payload (Normal request, Blank, or Membership change)
    pub payload: EzEntryPayload<T>,
}

// Manually implement Debug to avoid T: Debug bound
impl<T> std::fmt::Debug for EzEntry<T>
where T: EzTypes
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EzEntry").field("log_id", &self.log_id).field("payload", &self.payload).finish()
    }
}

// Manually implement Display
impl<T> std::fmt::Display for EzEntry<T>
where T: EzTypes
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "EzEntry(log_id: ({}, {}), payload: {:?})",
            self.log_id.0, self.log_id.1, self.payload
        )
    }
}

// Implement RaftPayload trait
impl<T> RaftPayload<u64, BasicNode> for EzEntry<T>
where T: EzTypes
{
    fn get_membership(&self) -> Option<Membership<u64, BasicNode>> {
        self.payload.get_membership()
    }
}

// Implement openraft::RaftEntry trait so EzEntry works with OpenRaft
impl<T> RaftEntry for EzEntry<T>
where T: EzTypes
{
    type CommittedLeaderId = EzCommittedLeaderId;
    type D = T::Request;
    type NodeId = u64;
    type Node = BasicNode;

    fn new(log_id: LogId<EzCommittedLeaderId>, payload: EzEntryPayload<T>) -> Self {
        Self {
            log_id: log_id.to_type(),
            payload,
        }
    }

    fn log_id_parts(&self) -> (&EzCommittedLeaderId, u64) {
        RaftLogId::log_id_parts(&self.log_id)
    }

    fn set_log_id(&mut self, new: LogId<EzCommittedLeaderId>) {
        self.log_id = new.to_type();
    }
}

/// Raft metadata managed by the framework
///
/// The framework updates this structure and you persist it via [`crate::EzStorage::persist`].
/// You don't need to understand the Raft details - just serialize and store it.
#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct EzMeta {
    /// This node's ID (assigned when joining cluster)
    pub node_id: Option<u64>,

    /// Current vote (term and node_id voted for)
    pub vote: Option<EzVote>,

    /// Last log entry (term, index)
    pub last_log_id: Option<EzLogId>,

    /// Last purged log entry (term, index)
    pub last_purged: Option<EzLogId>,
}

/// Snapshot data type: the serialized state machine bytes
pub type EzSnapshotData = Cursor<Vec<u8>>;

/// Snapshot metadata type alias
///
/// Points to OpenRaft's `SnapshotMeta` for full compatibility.
pub type EzSnapshotMeta = SnapshotMeta<EzCommittedLeaderId, u64, BasicNode>;

/// Snapshot type alias
///
/// Points to OpenRaft's `Snapshot` for full compatibility.
pub type EzSnapshot = Snapshot<EzCommittedLeaderId, u64, BasicNode, EzSnapshotData>;

/// Persistence operation
///
/// Each variant represents one atomic operation that should be persisted to disk.
/// The framework calls [`crate::EzStorage::persist`] with these operations.
#[derive(Debug, derive_more::Display)]
pub enum Persist<T>
where T: EzTypes
{
    /// Update Raft metadata (term, vote, log positions)
    #[display("Meta")]
    Meta(EzMeta),

    /// Write a log entry
    ///
    /// Entries arrive in index order and never target an index that is already present: the
    /// framework deletes conflicting entries with [`Persist::TruncateLogs`] first.
    #[display("LogEntry")]
    LogEntry(EzEntry<T>),

    /// Write a complete snapshot, replacing the previous one
    #[display("Snapshot")]
    Snapshot(EzSnapshot),

    /// Delete every log entry from this index onwards
    ///
    /// Sent when the entries conflict with the leader's log. They are not part of the
    /// replicated log and must not be returned by [`crate::EzStorage::read_logs`] again.
    #[display("TruncateLogs({_0})")]
    TruncateLogs(u64),

    /// Delete every log entry up to and including this index
    ///
    /// Sent once those entries are covered by a snapshot. This is the only signal that lets
    /// storage reclaim space.
    #[display("PurgeLogs({_0})")]
    PurgeLogs(u64),
}
