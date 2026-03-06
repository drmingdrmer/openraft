use crate::RaftPrimitives;
use crate::errors::ClientWriteError;
use crate::errors::ForwardToLeader;
use crate::raft::ClientWriteResponse;
use crate::raft::ClientWriteResult;
use crate::type_config::alias::LogIdOf;

/// The result of a write operation, returned by [`Raft::client_write_many()`].
///
/// This is a simplified version of [`ClientWriteResult`] that only contains
/// [`ForwardToLeader`] as the error type, since batch writes do not support
/// membership changes.
///
/// [`Raft::client_write_many()`]: crate::Raft::client_write_many
pub type WriteResult<P> = Result<WriteResponse<P>, ForwardToLeader<P>>;

/// Response from a successful write operation.
///
/// This is a simplified version of [`ClientWriteResponse`] used by
/// [`Raft::client_write_many()`]. It contains the log ID where the entry
/// was applied and the application-defined response.
///
/// [`Raft::client_write_many()`]: crate::Raft::client_write_many
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(bound = "P::R: crate::AppDataResponse")
)]
pub struct WriteResponse<P: RaftPrimitives> {
    /// The log ID of the applied entry.
    pub log_id: LogIdOf<P>,

    /// Application-defined response data.
    pub response: P::R,
}

impl<P: RaftPrimitives> From<ClientWriteResponse<P>> for WriteResponse<P> {
    fn from(resp: ClientWriteResponse<P>) -> Self {
        WriteResponse {
            log_id: resp.log_id,
            response: resp.data,
        }
    }
}

/// Convert `ClientWriteResult` to `WriteResult`.
pub(crate) fn into_write_result<P: RaftPrimitives>(result: ClientWriteResult<P>) -> WriteResult<P> {
    match result {
        Ok(resp) => Ok(resp.into()),
        Err(ClientWriteError::ForwardToLeader(e)) => Err(e),
        Err(ClientWriteError::ChangeMembershipError(_)) => {
            unreachable!("ChangeMembershipError should not occur for normal writes")
        }
    }
}
