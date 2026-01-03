//! Type configuration for EzRaft
//!
//! This module provides the `EzTypes` trait and `OpenRaftTypes` wrapper
//! that implement OpenRaft's `RaftTypeConfig` with sensible defaults.

use std::io::Cursor;
use std::marker::PhantomData;

use openraft::impls::leader_id_std::LeaderId;
use openraft::impls::OneshotResponder;
use openraft::AppData;
use openraft::AppDataResponse;
use openraft::BasicNode;
use openraft::LogId;
use openraft::Membership;
use openraft::RaftTypeConfig;
use openraft::Vote;
use serde::Deserialize;
use serde::Serialize;

/// Trait that defines the types needed for EzRaft
///
/// Users only need to specify their request and response types.
pub trait EzTypes: Send + Sync + 'static {
    /// Application request type
    type Request: AppData + Serialize + for<'de> Deserialize<'de> + Send + Sync + Clone;

    /// Application response type
    type Response: AppDataResponse + Send + Sync;
}

/// Wrapper type that implements `RaftTypeConfig` for any `T: EzTypes`
///
/// This provides all the default implementations needed for OpenRaft.
#[derive(Serialize, Deserialize)]
pub struct OpenRaftTypes<T: EzTypes> {
    #[serde(skip)]
    _phantom: PhantomData<T>,
}

impl<T: EzTypes> Copy for OpenRaftTypes<T> {}

impl<T: EzTypes> Clone for OpenRaftTypes<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: EzTypes> Default for OpenRaftTypes<T> {
    fn default() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<T: EzTypes> PartialEq for OpenRaftTypes<T> {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl<T: EzTypes> Eq for OpenRaftTypes<T> {}

impl<T: EzTypes> PartialOrd for OpenRaftTypes<T> {
    fn partial_cmp(&self, _other: &Self) -> Option<std::cmp::Ordering> {
        Some(std::cmp::Ordering::Equal)
    }
}

impl<T: EzTypes> Ord for OpenRaftTypes<T> {
    fn cmp(&self, _other: &Self) -> std::cmp::Ordering {
        std::cmp::Ordering::Equal
    }
}

impl<T: EzTypes> std::fmt::Debug for OpenRaftTypes<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OpenRaftTypes").finish()
    }
}

impl<T: EzTypes> RaftTypeConfig for OpenRaftTypes<T> {
    type D = T::Request;
    type R = T::Response;
    type NodeId = u64;
    type Node = BasicNode;
    type Term = u64;
    type LeaderId = LeaderId<Self>;
    type Vote = Vote<Self>;
    type Entry = crate::types::EzEntry<T>;
    type SnapshotData = Cursor<Vec<u8>>;
    type AsyncRuntime = openraft::TokioRuntime;
    type Responder<X: Send + 'static> = OneshotResponder<Self, X>;
    type ErrorSource = openraft::AnyError;
}

/// Type alias for OpenRaft Raft with OpenRaftTypes<T>
///
/// This is the underlying OpenRaft Raft instance used internally by EzRaft.
pub type EzOpenRaft<T> = openraft::Raft<OpenRaftTypes<T>>;

/// Type alias for Vote with OpenRaftTypes<T>
pub type EzVote<T> = Vote<OpenRaftTypes<T>>;

/// Type alias for LogId with OpenRaftTypes<T>
pub type EzLogIdOf<T> = LogId<OpenRaftTypes<T>>;

/// Type alias for EzEntry with OpenRaftTypes<T>
pub type EzEntryOf<T> = crate::types::EzEntry<T>;

/// Type alias for Membership with OpenRaftTypes<T>
pub type EzMembershipOf<T> = Membership<OpenRaftTypes<T>>;

/// Type alias for SnapshotData with OpenRaftTypes<T>
pub type EzSnapshotDataOf<T> = <OpenRaftTypes<T> as RaftTypeConfig>::SnapshotData;
