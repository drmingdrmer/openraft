use crate::RaftPrimitives;
use crate::errors::Fatal;
use crate::errors::Infallible;
use crate::errors::RaftError;

/// Convert a `Result<_, Fatal<P>>` to a `Result<T, RaftError<P, E>>`
///
/// This trait is used to convert `Results` from the new nested format(`Result<Result<_,E>,Fatal>`)
/// to a format that is compatible with the older API version(`Result<_, RaftError<E>>`).
/// - In the older Result style, both application Error and Fatal Error are wrapped in the one
///   [`RaftError`] type.
/// - In the new Result style, application Error and Fatal Error are wrapped in the two different
///   types(`Result<_,E>` and `Fatal<P>`).
///
/// The primary use case is for protocol methods that return `Result<T, Fatal<P>>` to be converted
/// to the backward compatible `Result<T, RaftError<P, E>>` which can represent both fatal errors
/// and application-specific errors.
pub(crate) trait IntoRaftResult<P, T, E>
where P: RaftPrimitives
{
    /// Convert a `Result<Result<T, E>, Fatal<P>>` or `Result<T, Fatal<P>>` to a
    /// `Result<T, RaftError<P, E>>`.
    fn into_raft_result(self) -> Result<T, RaftError<P, E>>;
}

impl<P, T, E> IntoRaftResult<P, T, E> for Result<Result<T, E>, Fatal<P>>
where P: RaftPrimitives
{
    fn into_raft_result(self) -> Result<T, RaftError<P, E>> {
        match self {
            Ok(Ok(t)) => Ok(t),
            Ok(Err(e)) => Err(RaftError::APIError(e)),
            Err(f) => Err(RaftError::Fatal(f)),
        }
    }
}

impl<P, T> IntoRaftResult<P, T, Infallible> for Result<T, Fatal<P>>
where P: RaftPrimitives
{
    fn into_raft_result(self) -> Result<T, RaftError<P>> {
        self.map_err(RaftError::Fatal)
    }
}
