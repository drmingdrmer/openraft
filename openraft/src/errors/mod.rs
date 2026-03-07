//! Error types exposed by this crate.

mod allow_next_revert_error;
mod conflicting_log_id;
pub mod decompose;
mod error_source;
mod fatal;
pub(crate) mod higher_vote;
pub mod into_ok;
pub(crate) mod into_raft_result;
mod leader_changed;
mod linearizable_read_error;
mod membership_error;
mod node_not_found;
mod operation;
mod raft_error;
mod reject_append_entries;
mod reject_vote;
mod replication_closed;
pub(crate) mod replication_error;
pub(crate) mod storage_error;
mod storage_io_result;
mod streaming_error;

use std::collections::BTreeSet;
use std::error::Error;
use std::time::Duration;

pub use self::allow_next_revert_error::AllowNextRevertError;
pub use self::conflicting_log_id::ConflictingLogId;
pub use self::error_source::BacktraceDisplay;
pub use self::error_source::ErrorSource;
pub use self::fatal::Fatal;
pub(crate) use self::higher_vote::HigherVote;
pub use self::leader_changed::LeaderChanged;
pub use self::linearizable_read_error::LinearizableReadError;
pub use self::membership_error::MembershipError;
pub use self::node_not_found::NodeNotFound;
pub use self::operation::Operation;
pub use self::raft_error::RaftError;
pub(crate) use self::reject_append_entries::RejectAppendEntries;
pub use self::reject_vote::RejectVote;
pub use self::replication_closed::ReplicationClosed;
pub(crate) use self::replication_error::ReplicationError;
pub(crate) use self::storage_io_result::StorageIOResult;
pub use self::streaming_error::StreamingError;
use crate::Membership;
use crate::RaftPrimitives;
use crate::RaftTypes;
use crate::network::RPCTypes;
use crate::raft_types::SnapshotSegmentId;
use crate::try_as_ref::TryAsRef;
use crate::type_config::alias::ErrorSourceOf;
use crate::type_config::alias::LogIdOf;
use crate::type_config::alias::VoteOf;

/// For backward compatibility, use [`LinearizableReadError`] instead.
#[deprecated(since = "0.10.0", note = "use `LinearizableReadError` instead")]
pub type CheckIsLeaderError<P> = LinearizableReadError<P>;

/// Error related to installing a snapshot.
// TODO: remove
#[derive(Debug, Clone, thiserror::Error, derive_more::TryInto)]
#[derive(PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum InstallSnapshotError {
    /// The snapshot segment offset does not match what was expected.
    #[error(transparent)]
    SnapshotMismatch(#[from] SnapshotMismatch),
}

/// An error related to a client write request.
#[derive(Debug, Clone, thiserror::Error, derive_more::TryInto)]
#[derive(PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub enum ClientWriteError<P>
where P: RaftPrimitives
{
    /// This node is not the leader; request should be forwarded to the leader.
    #[error(transparent)]
    ForwardToLeader(#[from] ForwardToLeader<P>),

    /// When writing a change-membership entry.
    #[error(transparent)]
    ChangeMembershipError(#[from] ChangeMembershipError<P>),
}

impl<P> TryAsRef<ForwardToLeader<P>> for ClientWriteError<P>
where P: RaftPrimitives
{
    fn try_as_ref(&self) -> Option<&ForwardToLeader<P>> {
        match self {
            Self::ForwardToLeader(f) => Some(f),
            _ => None,
        }
    }
}

/// The set of errors which may take place when requesting to propose a config change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub enum ChangeMembershipError<P: RaftPrimitives> {
    /// A membership change is already in progress.
    #[error(transparent)]
    InProgress(#[from] InProgress<P>),

    /// The proposed membership change would result in an empty membership.
    #[error(transparent)]
    EmptyMembership(#[from] EmptyMembership),

    /// A learner that should be in the cluster was not found.
    #[error(transparent)]
    LearnerNotFound(#[from] LearnerNotFound<P>),
}

/// The set of errors which may take place when initializing a pristine Raft node.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, derive_more::TryInto)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub enum InitializeError<C>
where C: RaftTypes
{
    /// Initialization operation is not allowed in the current state.
    #[error(transparent)]
    NotAllowed(#[from] NotAllowed<C>),

    /// This node is not included in the initial membership configuration.
    #[error(transparent)]
    NotInMembers(#[from] NotInMembers<C::Prim>),
}

/// Error occurs when invoking a remote raft API.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
// P already has serde bound.
// E still needs additional serde bound.
// `serde(bound="")` does not work in this case.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(bound(serialize = "E: serde::Serialize")),
    serde(bound(deserialize = "E: for <'d> serde::Deserialize<'d>"))
)]
pub enum RPCError<P: RaftPrimitives, E: Error = Infallible> {
    /// The RPC request timed out.
    #[error(transparent)]
    Timeout(#[from] Timeout<P>),

    /// The node is temporarily unreachable and should backoff before retrying.
    #[error(transparent)]
    Unreachable(#[from] Unreachable<P>),

    /// Failed to send the RPC request and should retry immediately.
    #[error(transparent)]
    Network(#[from] NetworkError<P>),

    /// The remote node returned an error.
    #[error(transparent)]
    RemoteError(#[from] RemoteError<P, E>),
}

impl<P, E> RPCError<P, E>
where
    P: RaftPrimitives,
    E: Error,
{
    /// Returns a weight indicating how severe this error is for backoff purposes.
    ///
    /// Higher values indicate more severe errors that should trigger longer backoff.
    /// - Timeout/Network errors: 2 (transient, retry soon)
    /// - Unreachable/RemoteError: 100 (more serious, back off longer)
    pub(crate) fn backoff_rank(&self) -> u64 {
        match &self {
            RPCError::Timeout(_) => 2,
            RPCError::Unreachable(_unreachable) => 100,
            RPCError::Network(_) => 2,
            RPCError::RemoteError(_) => 100,
        }
    }
}

impl<P, E> RPCError<P, RaftError<P, E>>
where
    P: RaftPrimitives,
    E: Error,
{
    /// Return a reference to ForwardToLeader error if Self::RemoteError contains one.
    pub fn forward_to_leader(&self) -> Option<&ForwardToLeader<P>>
    where E: TryAsRef<ForwardToLeader<P>> {
        match self {
            RPCError::Timeout(_) => None,
            RPCError::Unreachable(_) => None,
            RPCError::Network(_) => None,
            RPCError::RemoteError(remote_err) => remote_err.source.forward_to_leader(),
        }
    }
}

impl<P> RPCError<P>
where P: RaftPrimitives
{
    /// Convert to a [`RPCError`] with [`RaftError`] as the error type.
    pub fn with_raft_error<E: Error>(self) -> RPCError<P, RaftError<P, E>> {
        match self {
            RPCError::Timeout(e) => RPCError::Timeout(e),
            RPCError::Unreachable(e) => RPCError::Unreachable(e),
            RPCError::Network(e) => RPCError::Network(e),
        }
    }
}

/// Error that occurred on a remote Raft peer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[error("error occur on remote peer {target}: {source}")]
pub struct RemoteError<P, T: Error>
where P: RaftPrimitives
{
    /// The node ID of the remote peer where the error occurred.
    #[cfg_attr(feature = "serde", serde(bound = ""))]
    pub target: P::NodeId,
    /// The node information of the remote peer, if available.
    #[cfg_attr(feature = "serde", serde(bound = ""))]
    pub target_node: Option<P::Node>,
    /// The error that occurred on the remote peer.
    pub source: T,
}

impl<P: RaftPrimitives, T: Error> RemoteError<P, T> {
    /// Create a new RemoteError with target node ID.
    pub fn new(target: P::NodeId, e: T) -> Self {
        Self {
            target,
            target_node: None,
            source: e,
        }
    }
    /// Create a new RemoteError with target node ID and node information.
    pub fn new_with_node(target: P::NodeId, node: P::Node, e: T) -> Self {
        Self {
            target,
            target_node: Some(node),
            source: e,
        }
    }
}

impl<P, E> From<RemoteError<P, Fatal<P>>> for RemoteError<P, RaftError<P, E>>
where
    P: RaftPrimitives,
    E: Error,
{
    fn from(e: RemoteError<P, Fatal<P>>) -> Self {
        RemoteError {
            target: e.target,
            target_node: e.target_node,
            source: RaftError::Fatal(e.source),
        }
    }
}

/// Error that indicates a **temporary** network error and when it is returned, Openraft will retry
/// immediately.
///
/// Unlike [`Unreachable`], which indicates an error that should backoff before retrying.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
#[error("NetworkError: {source}")]
pub struct NetworkError<P: RaftPrimitives> {
    source: ErrorSourceOf<P>,
}

impl<P: RaftPrimitives> NetworkError<P> {
    /// Create a new NetworkError from an error.
    pub fn new<E: Error + 'static>(e: &E) -> Self {
        Self {
            source: ErrorSourceOf::<P>::from_error(e),
        }
    }

    /// Create a NetworkError from a string message.
    pub fn from_string(msg: impl ToString) -> Self {
        Self {
            source: ErrorSourceOf::<P>::from_string(msg),
        }
    }
}

/// Error indicating a node is unreachable. Retries should be delayed.
///
/// This error suggests that immediate retries are not advisable when a node is not reachable.
/// Upon encountering this error, Openraft will invoke [`backoff()`] to implement a delay before
/// attempting to resend any information.
///
/// This error is similar to [`NetworkError`] but with a key distinction: `Unreachable` advises a
/// backoff period, whereas with [`NetworkError`], Openraft may attempt an immediate retry.
///
/// [`backoff()`]: crate::network::RaftNetworkV2::backoff
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
#[error("Unreachable node: {source}")]
pub struct Unreachable<P: RaftPrimitives> {
    source: ErrorSourceOf<P>,
}

impl<P: RaftPrimitives> Unreachable<P> {
    /// Create a new Unreachable error from an error.
    pub fn new<E: Error + 'static>(e: &E) -> Self {
        Self {
            source: ErrorSourceOf::<P>::from_error(e),
        }
    }

    /// Create an Unreachable error from a string message.
    pub fn from_string(msg: impl ToString) -> Self {
        Self {
            source: ErrorSourceOf::<P>::from_string(msg),
        }
    }
}

/// Error indicating that an RPC request timed out.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
#[error("timeout after {timeout:?} when {action} {id}->{target}")]
pub struct Timeout<P: RaftPrimitives> {
    /// The type of RPC that timed out.
    pub action: RPCTypes,
    /// The node ID that initiated the request.
    pub id: P::NodeId,
    /// The target node ID.
    pub target: P::NodeId,
    /// The timeout duration that elapsed.
    pub timeout: Duration,
}

/// Error indicating that the request should be forwarded to the leader.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
#[error("has to forward request to: {leader_id:?}, {leader_node:?}")]
pub struct ForwardToLeader<P>
where P: RaftPrimitives
{
    /// The node ID of the current leader, if known.
    pub leader_id: Option<P::NodeId>,
    /// The node information of the current leader, if known.
    pub leader_node: Option<P::Node>,
}

impl<P> ForwardToLeader<P>
where P: RaftPrimitives
{
    /// Create a ForwardToLeader error with no known leader information.
    pub const fn empty() -> Self {
        Self {
            leader_id: None,
            leader_node: None,
        }
    }

    /// Create a ForwardToLeader error with known leader information.
    pub fn new(leader_id: P::NodeId, node: P::Node) -> Self {
        Self {
            leader_id: Some(leader_id),
            leader_node: Some(node),
        }
    }
}

/// Error indicating a snapshot segment ID mismatch.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[error("snapshot segment id mismatch, expect: {expect}, got: {got}")]
pub struct SnapshotMismatch {
    /// The expected snapshot segment ID.
    pub expect: SnapshotSegmentId,
    /// The actual snapshot segment ID received.
    pub got: SnapshotSegmentId,
}

/// Error indicating that not enough nodes responded to form a quorum.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
#[error("not enough for a quorum, cluster: {cluster}, got: {got:?}")]
pub struct QuorumNotEnough<P: RaftPrimitives> {
    /// A description of the cluster membership.
    pub cluster: String,
    /// The set of nodes that responded.
    pub got: BTreeSet<P::NodeId>,
}

/// Error indicating a membership change is already in progress.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
#[error(
    "the cluster is already undergoing a configuration change at log {membership_log_id:?}, last committed membership log id: {committed:?}"
)]
pub struct InProgress<P: RaftPrimitives> {
    /// The log ID of the last committed membership change.
    pub committed: Option<LogIdOf<P>>,
    /// The log ID of the membership change currently in progress.
    pub membership_log_id: Option<LogIdOf<P>>,
}

/// Error indicating a learner node was not found in the cluster.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
#[error("Learner {node_id} not found: add it as learner before adding it as a voter")]
pub struct LearnerNotFound<P: RaftPrimitives> {
    /// The node ID of the learner that was not found.
    pub node_id: P::NodeId,
}

/// Error indicating an operation is not allowed in the current state.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
#[error("not allowed to initialize due to current raft state: last_log_id: {last_log_id:?} vote: {vote}")]
pub struct NotAllowed<C: RaftTypes> {
    /// The last log ID in the current state.
    pub last_log_id: Option<LogIdOf<C::Prim>>,
    /// The current vote state.
    pub vote: VoteOf<C>,
}

/// Error indicating a node is not a member of the cluster.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
#[error("node {node_id} has to be a member. membership:{membership:?}")]
pub struct NotInMembers<P>
where P: RaftPrimitives
{
    /// The node ID that is not in the membership.
    pub node_id: P::NodeId,
    /// The current cluster membership.
    pub membership: Membership<P>,
}

/// Error indicating an empty membership configuration was provided.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[error("new membership cannot be empty")]
pub struct EmptyMembership {}

/// An error type that can never occur, used as a placeholder for infallible operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[error("infallible")]
pub enum Infallible {}

/// A placeholder to mark RaftError won't have a ForwardToLeader variant.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[error("no-forward")]
pub enum NoForward {}
