use crate::RaftPrimitives;
use crate::log_id::ref_log_id::RefLogId;
use crate::type_config::alias::LogIdOf;

pub(crate) trait OptionRefLogIdExt<P>
where P: RaftPrimitives
{
    /// Creates a new owned [`LogId`] from the reference log ID.
    ///
    /// [`LogId`]: crate::log_id::LogId
    fn to_log_id(&self) -> Option<LogIdOf<P>>;
}

impl<P> OptionRefLogIdExt<P> for Option<RefLogId<'_, P>>
where P: RaftPrimitives
{
    fn to_log_id(&self) -> Option<LogIdOf<P>> {
        self.as_ref().map(|r| r.into_log_id())
    }
}
