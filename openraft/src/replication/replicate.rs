use std::fmt;

use display_more::DisplayOptionExt;

use crate::RaftTypeConfig;
use crate::log_id_range::LogIdRange;
use crate::progress::inflight_id::InflightId;
use crate::replication::payload::Payload;
use crate::type_config::alias::LogIdOf;

/// A leader-local instruction to probe for a matching point, stream logs, or clear a plan.
///
/// A log-stream plan can span multiple network requests. The replication task attaches the
/// instruction's `InflightId` to local progress notifications to RaftCore or, for a clear, to a
/// separate confirmation. The ID is not sent to the follower.
#[derive(PartialEq, Eq, Clone, Debug)]
pub(crate) struct Replicate<C>
where C: RaftTypeConfig
{
    /// Identifies this instruction in notifications to RaftCore.
    pub(crate) inflight_id: InflightId,

    /// Specifies which logs to probe or stream.
    ///
    /// `None` clears the previous plan. The replication task confirms the clear only once it no
    /// longer reads logs for that plan. RaftCore waits for the confirmation before purging logs
    /// the plan could read; see
    /// [`Command::ClearReplicate`](crate::engine::Command::ClearReplicate).
    pub(crate) payload: Option<Payload<C>>,
}

impl<C: RaftTypeConfig> fmt::Display for Replicate<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.payload {
            Some(Payload::Probe { log_id_range }) => {
                write!(
                    f,
                    "Replicate{{probe: {}, inflight_id: {}}}",
                    log_id_range, self.inflight_id
                )
            }
            Some(Payload::LogsSince { prev }) => {
                write!(
                    f,
                    "Replicate{{logs_since: {}, inflight_id: {}}}",
                    prev.display(),
                    self.inflight_id
                )
            }
            None => write!(f, "Replicate{{clear, inflight_id: {}}}", self.inflight_id),
        }
    }
}

impl<C> Replicate<C>
where C: RaftTypeConfig
{
    /// Creates a request to probe the matching point with logs from a candidate range.
    pub(crate) fn new_probe(log_id_range: LogIdRange<C>, inflight_id: InflightId) -> Self {
        Self {
            inflight_id,
            payload: Some(Payload::Probe { log_id_range }),
        }
    }

    /// Creates a request to replicate logs after `prev` with no upper bound.
    pub(crate) fn new_logs_since(prev: Option<LogIdOf<C>>, inflight_id: InflightId) -> Self {
        Self {
            inflight_id,
            payload: Some(Payload::LogsSince { prev }),
        }
    }

    /// Creates a request to clear the plan the replication task ran before, which the task answers
    /// with `inflight_id`.
    pub(crate) fn new_clear(inflight_id: InflightId) -> Self {
        Self {
            inflight_id,
            payload: None,
        }
    }
}
