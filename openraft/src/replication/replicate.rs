use std::fmt;

use crate::RaftPrimitives;
use crate::display_ext::DisplayOptionExt;
use crate::log_id_range::LogIdRange;
use crate::progress::inflight_id::InflightId;
use crate::replication::payload::Payload;
use crate::type_config::alias::LogIdOf;

/// A replication data request containing log entries to send.
///
/// Each request has a corresponding `Inflight` record on the leader, identified by an
/// `InflightId`. The follower's response carries the same `InflightId` so the leader can
/// match the response to the correct inflight state.
#[derive(PartialEq, Eq, Clone, Debug)]
pub(crate) struct Replicate<P>
where P: RaftPrimitives
{
    /// Identifies this inflight request for matching with responses.
    pub(crate) inflight_id: InflightId,

    /// Specifies which logs to replicate.
    pub(crate) payload: Payload<P>,
}

impl<P> Default for Replicate<P>
where P: RaftPrimitives
{
    fn default() -> Self {
        Replicate {
            inflight_id: InflightId::new(0),
            payload: Payload::LogIdRange {
                log_id_range: LogIdRange::new(None, None),
            },
        }
    }
}

impl<P: RaftPrimitives> fmt::Display for Replicate<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.payload {
            Payload::LogIdRange { log_id_range } => {
                write!(
                    f,
                    "Replicate{{log_id_range: {}, inflight_id: {}}}",
                    log_id_range, self.inflight_id
                )
            }
            Payload::LogsSince { prev } => {
                write!(
                    f,
                    "Replicate{{logs_since: {}, inflight_id: {}}}",
                    prev.display(),
                    self.inflight_id
                )
            }
        }
    }
}

impl<P> Replicate<P>
where P: RaftPrimitives
{
    /// Creates a request to replicate logs in a fixed range.
    pub(crate) fn new_logs(log_id_range: LogIdRange<P>, inflight_id: InflightId) -> Self {
        Self {
            inflight_id,
            payload: Payload::LogIdRange { log_id_range },
        }
    }

    /// Creates a request to replicate logs after `prev` with no upper bound.
    pub(crate) fn new_logs_since(prev: Option<LogIdOf<P>>, inflight_id: InflightId) -> Self {
        Self {
            inflight_id,
            payload: Payload::LogsSince { prev },
        }
    }
}
