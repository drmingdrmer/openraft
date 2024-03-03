use crate::storage::LogMetaV3;
use crate::type_config::alias::EntryOf;
use crate::LogId;
use crate::OptionalSend;
use crate::RaftTypeConfig;

/// An I/O request submitted to the Log store.
///
/// A `LogIO` can include operations such as saving the `LogStoreMeta`, appending, truncating, or
/// purging log entries. It can be considered as an WAL (Write-Ahead Log) entry for the underlying
/// storage implementation. To persist the log store state, the implementation can serialize and
/// store a `LogIO` instance to its WAL.
///
/// ### Correctness Requirements:
///
/// - `LogIO` instances must be handled sequentially. The implementation must **NOT** apply a later
///   `LogIO` before a previous `LogIO`, regardless of whether they are the same enum variants or
///   not.
///
/// - There must not be any **holes** in the logs. This is because Raft only examines the last log
///   id to ensure correctness.
pub enum LogIO<C, I = [EntryOf<C>; 0]>
where
    C: RaftTypeConfig,
    I: IntoIterator<Item = C::Entry> + OptionalSend,
    I::IntoIter: OptionalSend,
{
    /// Save metadata.
    Meta(LogMetaV3<C>),

    /// Purge logs up to (and including) the specified `LogId`.
    Purge(LogId<C::NodeId>),

    /// Truncate logs starting from (and including) the specified `LogId`.
    Truncate(LogId<C::NodeId>),

    /// Append log entries.
    Append(I),
}

impl<C> LogIO<C, [C::Entry; 0]>
where C: RaftTypeConfig
{
    pub fn meta(meta: LogMetaV3<C>) -> Self {
        Self::Meta(meta)
    }

    pub fn purge(upto: LogId<C::NodeId>) -> Self {
        Self::Purge(upto)
    }

    pub fn truncate(from: LogId<C::NodeId>) -> Self {
        Self::Truncate(from)
    }
}

impl<C, I> LogIO<C, I>
where
    C: RaftTypeConfig,
    I: IntoIterator<Item = C::Entry> + OptionalSend,
    I::IntoIter: OptionalSend,
{
    pub fn append(entries: I) -> Self {
        Self::Append(entries)
    }
}
