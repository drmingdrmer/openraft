use std::fmt;

use display_more::DisplayOptionExt;
use display_more::DisplayResultExt;

use crate::RaftTypeConfig;
use crate::raft::ConflictHint;
use crate::type_config::alias::LogIdOf;

/// A log or snapshot replication outcome, or a conflict reported by the heartbeat worker.
///
/// [`Notification::ReplicationProgress`] carries the stream ID and optional inflight ID.
/// Leader-clock updates use the request's send time carried separately by
/// [`Notification::HeartbeatProgress`].
///
/// [`Notification::ReplicationProgress`]: crate::core::notification::Notification::ReplicationProgress
/// [`Notification::HeartbeatProgress`]: crate::core::notification::Notification::HeartbeatProgress
#[derive(Debug, Clone)]
pub(crate) struct Progress<C>
where C: RaftTypeConfig
{
    /// The ID of the replication target.
    pub(crate) target: C::NodeId,

    /// `Ok` carries a matching or conflict result; `Err` carries a worker-reported error string.
    pub(crate) result: Result<ReplicationResult<C>, String>,

    /// Optional search guidance accompanying a conflict, never a durable acknowledgement.
    pub(crate) hint: Option<ConflictHint<C>>,
}

impl<C> fmt::Display for Progress<C>
where C: RaftTypeConfig
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "replication::Progress: target={}, result: {}",
            self.target,
            self.result.display(),
        )
    }
}

/// The target's matching log boundary or conflict.
///
/// `Ok(Some(log_id))` acknowledges the prefix through that log ID, and `Ok(None)` acknowledges
/// an empty prefix. `Err(log_id)` identifies the leader's log ID that the target rejected.
#[derive(Clone, Debug)]
pub(crate) struct ReplicationResult<C: RaftTypeConfig>(pub(crate) Result<Option<LogIdOf<C>>, LogIdOf<C>>);

impl<C> fmt::Display for ReplicationResult<C>
where C: RaftTypeConfig
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Ok(matching) => write!(f, "(Match:{})", matching.display()),
            Err(conflict) => write!(f, "(Conflict:{})", conflict),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::testing::UTConfig;
    use crate::engine::testing::log_id;
    use crate::replication::response::ReplicationResult;

    #[test]
    fn test_replication_result_display() {
        let result = ReplicationResult::<UTConfig>(Ok(Some(log_id(1, 2, 3))));
        let want = format!("(Match:{})", log_id(1, 2, 3));
        assert!(result.to_string().ends_with(&want), "{}", result.to_string());

        let result = ReplicationResult::<UTConfig>(Err(log_id(1, 2, 3)));
        let want = format!("(Conflict:{})", log_id(1, 2, 3));
        assert!(result.to_string().ends_with(&want), "{}", result.to_string());
    }
}
