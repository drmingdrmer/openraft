use crate::LogId;
use crate::RaftPrimitives;
#[cfg(doc)]
use crate::core::RaftCore;

/// State for [`RaftCore`] that does not directly affect consensus.
///
/// Handles behavior not in [`Engine`](crate::engine::Engine), such as snapshot triggering and log
/// purging.
#[derive(Debug, Default, Clone)]
pub(crate) struct CoreState<P>
where P: RaftPrimitives
{
    /// LogId of the last snapshot attempt.
    ///
    /// Prevents repeated attempts when the state machine declines to build a snapshot.
    pub(crate) snapshot_tried_at: Option<LogId<P>>,
}
