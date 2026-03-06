use std::fmt;
use std::fmt::Debug;

use openraft_macros::since;

use crate::Membership;
use crate::RaftPrimitives;
use crate::display_ext::DisplayOptionExt;
use crate::errors::ClientWriteError;
use crate::type_config::alias::LogIdOf;

/// The result of a write request to Raft.
pub type ClientWriteResult<P> = Result<ClientWriteResponse<P>, ClientWriteError<P>>;

/// The response to a client-request.
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(bound = "P::R: crate::AppDataResponse")
)]
pub struct ClientWriteResponse<P: RaftPrimitives> {
    /// The id of the log that is applied.
    pub log_id: LogIdOf<P>,

    /// Application specific response data.
    pub data: P::R,

    /// If the log entry is a change-membership entry.
    pub membership: Option<Membership<P>>,
}

impl<P> ClientWriteResponse<P>
where P: RaftPrimitives
{
    /// Create a new instance of `ClientWriteResponse`.
    #[allow(dead_code)]
    #[since(version = "0.9.5")]
    pub(crate) fn new_app_response(log_id: LogIdOf<P>, data: P::R) -> Self {
        Self {
            log_id,
            data,
            membership: None,
        }
    }

    #[since(version = "0.9.5")]
    pub fn log_id(&self) -> &LogIdOf<P> {
        &self.log_id
    }

    #[since(version = "0.9.5")]
    pub fn response(&self) -> &P::R {
        &self.data
    }

    /// Return membership config if the log entry is a change-membership entry.
    #[since(version = "0.9.5")]
    pub fn membership(&self) -> &Option<Membership<P>> {
        &self.membership
    }
}

impl<P: RaftPrimitives> Debug for ClientWriteResponse<P>
where P::R: Debug
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientWriteResponse")
            .field("log_id", &self.log_id)
            .field("data", &self.data)
            .field("membership", &self.membership)
            .finish()
    }
}

impl<P> fmt::Display for ClientWriteResponse<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ClientWriteResponse{{log_id:{}, membership:{}}}",
            self.log_id,
            self.membership.display()
        )
    }
}
