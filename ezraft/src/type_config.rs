//! Type configuration for EzRaft
//!
//! This module provides the `EzTypes` trait and `OpenRaftTypes` wrapper
//! that implement OpenRaft's `RaftTypeConfig` with sensible defaults.

use std::marker::PhantomData;

use openraft::AppData;
use openraft::AppDataResponse;
use openraft::BasicNode;
use openraft::OptionalSend;
use openraft::RaftTypeConfig;
use openraft::Vote;
use openraft::impls::InlineBatch;
use openraft::impls::OneshotResponder;
use openraft::impls::leader_id_std::LeaderId;
use serde::Deserialize;
use serde::Serialize;

/// Trait that defines the types needed for EzRaft
///
/// Users only need to specify their request and response types.
pub trait EzTypes: Send + Sync + 'static {
    /// Application request type
    ///
    /// Serde carries it over the wire, `Clone` keeps a copy for forwarding to the leader, and
    /// [`AppData`] asks for `Debug + Display` because openraft prints requests in its logs and
    /// errors. Derive `Display` (e.g. with `derive_more`) or write a short impl - see
    /// `examples/kvstore.rs`.
    type Request: AppData + Serialize + for<'de> Deserialize<'de> + Send + Sync + Clone;

    /// Application response type
    type Response: AppDataResponse + Default + Send + Sync;
}

/// Wrapper type that implements `RaftTypeConfig` for any `T: EzTypes`
///
/// This provides all the default implementations needed for OpenRaft.
pub struct OpenRaftTypes<T>
where T: EzTypes
{
    _phantom: PhantomData<T>,
}

impl<T> Copy for OpenRaftTypes<T> where T: EzTypes {}

impl<T> Clone for OpenRaftTypes<T>
where T: EzTypes
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Default for OpenRaftTypes<T>
where T: EzTypes
{
    fn default() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<T> PartialEq for OpenRaftTypes<T>
where T: EzTypes
{
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl<T> Eq for OpenRaftTypes<T> where T: EzTypes {}

impl<T> PartialOrd for OpenRaftTypes<T>
where T: EzTypes
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for OpenRaftTypes<T>
where T: EzTypes
{
    fn cmp(&self, _other: &Self) -> std::cmp::Ordering {
        std::cmp::Ordering::Equal
    }
}

impl<T> std::fmt::Debug for OpenRaftTypes<T>
where T: EzTypes
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OpenRaftTypes").finish()
    }
}

impl<T> RaftTypeConfig for OpenRaftTypes<T>
where T: EzTypes
{
    type D = T::Request;
    type R = T::Response;
    type NodeId = u64;
    type Node = BasicNode;
    type Term = u64;
    type LeaderId = EzLeaderId;
    type Vote = Vote<EzLeaderId>;
    type Entry = crate::types::EzEntry<T>;
    type AsyncRuntime = openraft::TokioRuntime;
    type Responder<X>
        = OneshotResponder<Self, X>
    where X: OptionalSend + 'static;
    type Batch<X>
        = InlineBatch<X>
    where X: OptionalSend + 'static;
    type ErrorSource = openraft::AnyError;
}

/// Leader ID type: standard Raft, at most one leader per term
pub type EzLeaderId = LeaderId<u64, u64>;

/// Type alias for the Raft vote
pub type EzVote = Vote<EzLeaderId>;
