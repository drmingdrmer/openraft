use std::fmt::Debug;
use std::ops::RangeBounds;

use crate::alias::AsyncRuntimeOf;
use crate::storage::LogFlushed;
use crate::storage::LogIO;
use crate::storage::LogMetaV3;
use crate::AsyncRuntime;
use crate::OptionalSend;
use crate::OptionalSync;
use crate::RaftLogReader;
use crate::RaftTypeConfig;
use crate::StorageError;
use crate::StorageIOError;

/// API for log store.
///
/// `vote` API are also included because in raft, vote is part to the log: `vote` is about **when**,
/// while `log` is about **what**. A distributed consensus is about **at what a time, happened what
/// a event**.
///
/// ### To ensure correctness:
///
/// - Logs must be consecutive, i.e., there must **NOT** leave a **hole** in logs.
/// - All write-IO must be serialized, i.e., the internal implementation must **NOT** apply a latter
///   write request before a former write request is completed. This rule applies to both `vote` and
///   `log` IO. E.g., Saving a vote and appending a log entry must be serialized too.
#[add_async_trait]
pub trait RaftLogWriter<C>: OptionalSend + 'static
where C: RaftTypeConfig
{
    /// Log reader type.
    ///
    /// Log reader is used by multiple replication tasks, which read logs and send them to remote
    /// nodes.
    type LogReader: RaftLogReader<C>;

    /// Get the log reader.
    ///
    /// The method is intentionally async to give the implementation a chance to use asynchronous
    /// primitives to serialize access to the common internal object, if needed.
    async fn get_log_reader(&mut self) -> Self::LogReader;

    /// Submits a LogIO, and calls the `callback` once the IO is persisted on disk.
    ///
    /// The method should return immediately after saving the input in memory and call the
    /// `callback` when the data is persisted on disk, avoiding blocking.
    ///
    /// This method is still async because preparing the IO is typically an async operation.
    ///
    /// ### Correctness Requirements:
    ///
    /// - When this method returns, the data must be readable via a `LogReader` or `read_meta()`.
    ///
    /// - When the `callback` is called, the data must be persisted on disk.
    ///
    ///   NOTE: The `callback` can be called either before or after this method returns.
    ///
    /// - `LogIO` operations must be processed sequentially, i.e., the implementation must **NOT**
    ///   apply a later `LogIO` before a previous `LogIO` .
    ///
    /// - The implementation must not leave a **hole** in logs. Raft relies on examining the last
    ///   log id to ensure correctness.
    async fn write<I>(&mut self, _data: LogIO<C, I>, _callback: LogFlushed<C>) -> Result<(), StorageError<C::NodeId>>
    where
        I: IntoIterator<Item = C::Entry> + OptionalSend,
        I::IntoIter: OptionalSend,
    {
        // TODO: LogFlushed should contain leader vote
        // TODO: delegate to `append` and `truncate` and `purge` and `read_meta` and `save_vote` and
        todo!()
    }
}

#[add_async_trait]
pub trait RaftLogReaderV3<C>: OptionalSend + OptionalSync + 'static
where C: RaftTypeConfig
{
    /// Get a series of log entries from storage.
    ///
    /// ### Correctness requirements
    ///
    /// - The absence of an entry is tolerated only at the beginning or end of the range. Missing
    ///   entries within the range (i.e., holes) are not permitted and should result in a
    ///   `StorageError`.
    ///
    /// - The read operation must be transactional. That is, it should not reflect any state changes
    ///   that occur after the read operation has commenced.
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<C::Entry>, StorageError<C::NodeId>>;

    /// Returns the last saved metadata from the log store.
    ///
    /// Despite the function not modifying `self`, it is marked as `mut`, to indicate that only a
    /// single reader should access the metadata at a time.
    ///
    /// ### Implementation Note:
    ///
    /// If there is no metadata saved, the implementation should return a default value.
    async fn read_meta(&mut self) -> Result<LogMetaV3<C>, StorageError<C::NodeId>> {
        // TODO: provide default impl upon read_vote?
        todo!()
    }
}

/// Extension trait for RaftLogStorage to provide utility methods.
///
/// All methods in this trait are provided with default implementation.
#[add_async_trait]
pub trait RaftLogStorageV3Ext<C>: RaftLogWriter<C>
where C: RaftTypeConfig
{
    /// Writes data to the log store in a blocking mode.
    ///
    /// This function is similar to `write()`, but it operates in a blocking mode,
    /// waiting for the write operation to be completed before returning.
    /// Thus it does not require a callback function.
    async fn blocking_write<I>(&mut self, data: LogIO<C, I>) -> Result<(), StorageError<C::NodeId>>
    where
        I: IntoIterator<Item = C::Entry> + OptionalSend,
        I::IntoIter: OptionalSend,
    {
        let (tx, rx) = AsyncRuntimeOf::<C>::oneshot();

        let cb = LogFlushed::new(None, tx);

        self.write(data, cb).await?;

        rx.await.map_err(|e| StorageIOError::write(&e))?.map_err(|e| StorageIOError::write(&e))?;

        Ok(())
    }
}

impl<C, T> RaftLogStorageV3Ext<C> for T
where
    T: RaftLogWriter<C>,
    C: RaftTypeConfig,
{
}
