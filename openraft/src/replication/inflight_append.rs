use std::fmt;

use display_more::DisplayOptionExt;

use crate::RaftTypeConfig;
use crate::display_ext::display_instant::DisplayInstantExt;
use crate::type_config::TypeConfigExt;
use crate::type_config::alias::InstantOf;
use crate::type_config::alias::LogIdOf;

/// Tracks metadata for a single in-flight AppendEntries request.
///
/// [`InflightAppendQueue`] uses the log boundaries to attribute acknowledgements and conflicts
/// to conservative sending times. These times feed the leader clock, which the leader uses to
/// check its lease and determine whether pending reads are fresh enough to complete.
/// See [`InflightAppendQueue::drain_acked`] for the shared-boundary policy.
///
/// [`InflightAppendQueue`]: super::inflight_append_queue::InflightAppendQueue
/// [`InflightAppendQueue::drain_acked`]: super::inflight_append_queue::InflightAppendQueue::drain_acked
#[derive(Debug, Clone)]
pub(crate) struct InflightAppend<C>
where C: RaftTypeConfig
{
    /// The time captured when the request is enqueued, before it is yielded to the transport.
    pub(crate) sending_time: InstantOf<C>,

    /// The log id immediately before the entries in this request.
    pub(crate) prev_log_id: Option<LogIdOf<C>>,

    /// The log id of the last entry, or `prev_log_id` if the request is empty.
    /// Both endpoints are `None` if the request is empty and has no preceding log id.
    pub(crate) last_log_id: Option<LogIdOf<C>>,
}

impl<C> fmt::Display for InflightAppend<C>
where C: RaftTypeConfig
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "InflightAppend{{sending_time:{}, prev_log_id:{}, last_log_id:{}}}",
            self.sending_time.display(),
            self.prev_log_id.display(),
            self.last_log_id.display()
        )
    }
}

impl<C> InflightAppend<C>
where C: RaftTypeConfig
{
    pub(crate) fn new(prev_log_id: Option<LogIdOf<C>>, last_log_id: Option<LogIdOf<C>>) -> Self {
        Self {
            sending_time: C::now(),
            prev_log_id,
            last_log_id,
        }
    }
}
