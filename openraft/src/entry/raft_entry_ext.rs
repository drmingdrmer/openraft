use crate::RaftPrimitives;
use crate::entry::RaftEntry;
use crate::log_id::ref_log_id::RefLogId;

pub(crate) trait RaftEntryExt<P>: RaftEntry<P>
where P: RaftPrimitives
{
    /// Returns a lightweight [`RefLogId`] that contains the log id information.
    fn ref_log_id(&self) -> RefLogId<'_, P> {
        let (leader_id, index) = self.log_id_parts();
        RefLogId::new(leader_id, index)
    }
}

impl<P, T> RaftEntryExt<P> for T
where
    P: RaftPrimitives,
    T: RaftEntry<P>,
{
}
