//! Data structures for EzRaft
//!
//! This module contains the core data structures used throughout EzRaft.

use openraft::entry::RaftEntry;
use openraft::entry::RaftPayload;
use openraft::log_id::RaftLogId;
use openraft::vote::leader_id_std::CommittedLeaderId;
use openraft::EntryPayload;
use openraft::LogId;
use openraft::Membership;
use serde::Deserialize;
use serde::Serialize;

use crate::type_config::EzTypes;
use crate::type_config::EzVote;
use crate::type_config::OpenRaftTypes;

/// Log ID type (term, index)
///
/// A tuple that implements `RaftLogId` via OpenRaft's blanket implementation.
pub type EzLogId = (u64, u64);

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
    pub payload: EntryPayload<OpenRaftTypes<T>>,
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
impl<T> RaftPayload<OpenRaftTypes<T>> for EzEntry<T>
where T: EzTypes
{
    fn get_membership(&self) -> Option<Membership<OpenRaftTypes<T>>> {
        self.payload.get_membership()
    }
}

// Implement openraft::RaftEntry trait so EzEntry works with OpenRaft
impl<T> RaftEntry<OpenRaftTypes<T>> for EzEntry<T>
where T: EzTypes
{
    fn new(log_id: LogId<OpenRaftTypes<T>>, payload: EntryPayload<OpenRaftTypes<T>>) -> Self {
        Self {
            log_id: log_id.to_type(),
            payload,
        }
    }

    fn log_id_parts(&self) -> (&CommittedLeaderId<OpenRaftTypes<T>>, u64) {
        <EzLogId as RaftLogId<OpenRaftTypes<T>>>::log_id_parts(&self.log_id)
    }

    fn set_log_id(&mut self, new: LogId<OpenRaftTypes<T>>) {
        self.log_id = new.to_type();
    }
}

/// Raft metadata managed by the framework
///
/// The framework updates this structure and you persist it via [`EzStorage::save_state`].
/// You don't need to understand the Raft details - just serialize and store it.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct EzMeta<T>
where T: EzTypes
{
    /// Current vote (term and node_id voted for)
    pub vote: Option<EzVote<T>>,

    /// Last log entry (term, index)
    pub last_log_id: Option<EzLogId>,

    /// Last purged log entry (term, index)
    pub last_purged: Option<EzLogId>,
}

// Manual Clone implementation to avoid requiring T: Clone
impl<T> Clone for EzMeta<T>
where T: EzTypes
{
    fn clone(&self) -> Self {
        Self {
            vote: self.vote,
            last_log_id: self.last_log_id,
            last_purged: self.last_purged,
        }
    }
}

impl<T> Default for EzMeta<T>
where T: EzTypes
{
    fn default() -> Self {
        Self {
            vote: None,
            last_log_id: None,
            last_purged: None,
        }
    }
}

/// Snapshot metadata managed by the framework
///
/// The framework creates this when snapshotting and you persist it with the snapshot data.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct EzSnapshotMeta<T>
where T: EzTypes
{
    /// Last log entry included in this snapshot (term, index)
    pub last_log_id: EzLogId,

    /// Cluster membership at the time of snapshot
    pub membership: Membership<OpenRaftTypes<T>>,
}

// Manual Clone implementation to avoid requiring T: Clone
impl<T> Clone for EzSnapshotMeta<T>
where T: EzTypes
{
    fn clone(&self) -> Self {
        Self {
            last_log_id: self.last_log_id,
            membership: self.membership.clone(),
        }
    }
}

/// Snapshot data: metadata and raw bytes
pub type EzSnapshot<T> = (EzSnapshotMeta<T>, Vec<u8>);

/// State update operation to persist
///
/// Each variant represents one atomic operation that should be persisted to disk.
/// The framework calls [`EzStorage::save_state`] with these updates.
#[derive(Debug, derive_more::Display)]
pub enum EzStateUpdate<T>
where T: EzTypes
{
    /// Update Raft metadata (term, vote, log positions)
    #[display("WriteMeta")]
    WriteMeta(EzMeta<T>),

    /// Write a log entry
    #[display("WriteLog")]
    WriteLog(EzEntry<T>),

    /// Write a complete snapshot
    #[display("WriteSnapshot")]
    WriteSnapshot(EzSnapshotMeta<T>, Vec<u8>),
}
