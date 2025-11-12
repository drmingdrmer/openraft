use crate::RaftTypeConfig;
use crate::error::NetworkError;
use crate::error::Timeout;

/// Error occurs when invoking a remote raft API.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RetryableError<C: RaftTypeConfig> {
    /// The RPC request timed out.
    #[error(transparent)]
    Timeout(#[from] Timeout<C>),

    /// Failed to send the RPC request and should retry immediately.
    #[error(transparent)]
    Network(#[from] NetworkError),
}
