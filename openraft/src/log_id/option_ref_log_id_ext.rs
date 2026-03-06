use crate::RaftPrimitives;
use crate::log_id::ref_log_id::RefLogId;
use crate::type_config::alias::LogIdOf;

pub(crate) trait OptionRefLogIdExt<C>
where C: RaftPrimitives
{
    /// Creates a new owned [`LogId`] from the reference log ID.
    ///
    /// [`LogId`]: crate::log_id::LogId
    fn to_log_id(&self) -> Option<LogIdOf<C>>;
}

impl<C> OptionRefLogIdExt<C> for Option<RefLogId<'_, C>>
where C: RaftPrimitives
{
    fn to_log_id(&self) -> Option<LogIdOf<C>> {
        self.as_ref().map(|r| r.into_log_id())
    }
}
