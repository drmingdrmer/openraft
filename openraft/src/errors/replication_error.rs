use crate::RaftTypes;
use crate::StorageError;
use crate::errors::RPCError;
use crate::errors::higher_vote::HigherVote;
use crate::errors::replication_closed::ReplicationClosed;

/// Error variants related to the Replication.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ReplicationError<C>
where C: RaftTypes
{
    #[error(transparent)]
    HigherVote(#[from] HigherVote<C>),

    #[error(transparent)]
    Closed(#[from] ReplicationClosed),

    #[error(transparent)]
    StorageError(#[from] StorageError<C::Prim>),

    #[error(transparent)]
    RPCError(#[from] RPCError<C::Prim>),
}
