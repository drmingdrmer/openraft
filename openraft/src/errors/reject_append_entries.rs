use peel_off::Peel;

use crate::RaftComposites;
use crate::errors::ConflictingLogId;
use crate::errors::RejectVote;
use crate::raft::StreamAppendError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RejectAppendEntries<C: RaftComposites> {
    #[error("reject AppendEntries by a greater vote: {0}")]
    RejectVote(RejectVote<C>),

    #[error("reject AppendEntries due to conflicting log id: {0}")]
    ConflictingLogId(ConflictingLogId<C::Prim>),
}

impl<C: RaftComposites> From<RejectVote<C>> for RejectAppendEntries<C> {
    fn from(r: RejectVote<C>) -> Self {
        RejectAppendEntries::RejectVote(r)
    }
}

impl<C: RaftComposites> From<RejectAppendEntries<C>> for StreamAppendError<C> {
    fn from(e: RejectAppendEntries<C>) -> Self {
        match e {
            RejectAppendEntries::RejectVote(r) => StreamAppendError::HigherVote(r.higher),
            RejectAppendEntries::ConflictingLogId(c) => StreamAppendError::Conflict(c.expect),
        }
    }
}

/// Peel off `RejectVote`, leaving `ConflictingLogId` as the residual.
impl<C: RaftComposites> Peel for RejectAppendEntries<C> {
    type Peeled = RejectVote<C>;
    type Residual = ConflictingLogId<C::Prim>;

    fn peel(self) -> Result<ConflictingLogId<C::Prim>, RejectVote<C>> {
        match self {
            RejectAppendEntries::RejectVote(e) => Err(e),
            RejectAppendEntries::ConflictingLogId(e) => Ok(e),
        }
    }
}
