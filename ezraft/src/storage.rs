//! Internal storage adapter
//!
//! This module bridges the user's [`EzStorage`] and [`EzStateMachine`] traits
//! to OpenRaft's [`RaftLogStorage`] and [`RaftStateMachine`] traits.
//!
//! Users don't interact with this module directly - it's used internally by [`EzRaft`].

use std::fmt::Debug;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::marker::PhantomData;
use std::ops::RangeBounds;
use std::sync::Arc;

use futures::StreamExt;
use openraft::log_id::LogIndexOptionExt;
use openraft::log_id::RaftLogId;
use openraft::storage::ApplyResponder;
use openraft::storage::IOFlushed;
use openraft::storage::LogState;
use openraft::storage::RaftLogStorage;
use openraft::storage::RaftStateMachine;
use openraft::EntryPayload;
use openraft::LogId;
use openraft::Membership;
use openraft::RaftLogReader;
use openraft::RaftSnapshotBuilder;
use openraft::RaftTypeConfig;
use openraft::Snapshot;
use openraft::SnapshotMeta;
use openraft::StoredMembership;
use tokio::sync::Mutex;

use crate::trait_::EzStateMachine;
use crate::trait_::EzStorage;
use crate::type_config::EzTypes;
use crate::type_config::OpenRaftTypes;
use crate::types::*;

/// Type alias for snapshot data
type SnapshotDataOf<T> = <OpenRaftTypes<T> as RaftTypeConfig>::SnapshotData;

/// Internal storage state protected by single mutex
pub struct StorageState<S, T>
where
    S: EzStorage<T>,
    T: EzTypes,
{
    pub storage: S,
    pub cached_meta: EzMeta<T>,
}

/// Internal storage adapter
///
/// Bridges user's `EzStorage` and `EzStateMachine` to OpenRaft's storage traits.
///
/// Only metadata is cached in memory - logs are read from user storage on demand.
pub struct StorageAdapter<T, S, M>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    storage_state: Arc<Mutex<StorageState<S, T>>>,
    user_state: Arc<Mutex<M>>,
    _phantom: PhantomData<T>,
}

impl<T, S, M> StorageAdapter<T, S, M>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    /// Create a new storage adapter and load initial metadata
    pub async fn new(mut user_storage: S, user_state: M) -> Result<Self, std::io::Error> {
        // Load initial metadata
        let cached_meta = match user_storage.load_state().await? {
            Some((meta, _)) => meta,
            None => EzMeta::<T>::default(),
        };

        let storage_state = StorageState {
            storage: user_storage,
            cached_meta,
        };

        Ok(Self {
            storage_state: Arc::new(Mutex::new(storage_state)),
            user_state: Arc::new(Mutex::new(user_state)),
            _phantom: PhantomData,
        })
    }

    /// Get reference to user storage state (storage + metadata)
    pub fn storage_state(&self) -> &Arc<Mutex<StorageState<S, T>>> {
        &self.storage_state
    }

    /// Get reference to user state machine
    pub fn user_state(&self) -> &Arc<Mutex<M>> {
        &self.user_state
    }

    /// Update metadata and persist to storage
    async fn save_meta(&self, f: impl FnOnce(&mut EzMeta<T>)) -> Result<(), std::io::Error> {
        let mut state = self.storage_state.lock().await;
        f(&mut state.cached_meta);
        let update = EzStateUpdate::WriteMeta(state.cached_meta.clone());
        state.storage.save_state(update).await
    }

    /// Split into log storage and state machine storage
    pub fn split(self) -> (Arc<Self>, Arc<Self>) {
        let arc = Arc::new(self);
        (arc.clone(), arc)
    }
}

// Implement RaftLogStorage for Arc<StorageAdapter>
impl<T, S, M> RaftLogStorage<OpenRaftTypes<T>> for Arc<StorageAdapter<T, S, M>>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    type LogReader = Self;

    async fn get_log_state(&mut self) -> Result<LogState<OpenRaftTypes<T>>, std::io::Error> {
        let state = self.storage_state.lock().await;
        let last = state.cached_meta.last_log_id.map(|(t, i)| LogId::new_term_index(t, i));
        let last_purged = state.cached_meta.last_purged.map(|(t, i)| LogId::new_term_index(t, i));

        Ok(LogState {
            last_log_id: last,
            last_purged_log_id: last_purged,
        })
    }

    async fn save_vote(&mut self, vote: &<OpenRaftTypes<T> as RaftTypeConfig>::Vote) -> Result<(), std::io::Error> {
        self.save_meta(|m| m.vote = Some(*vote)).await
    }

    async fn append<I>(&mut self, entries: I, callback: IOFlushed<OpenRaftTypes<T>>) -> Result<(), std::io::Error>
    where
        I: IntoIterator<Item = <OpenRaftTypes<T> as RaftTypeConfig>::Entry> + Send,
        I::IntoIter: Send,
    {
        let mut last_log_id = None;

        // Save all log entries
        for entry in entries {
            last_log_id = Some(entry.log_id);
            let update = EzStateUpdate::WriteLog(entry);
            let mut state = self.storage_state.lock().await;
            state.storage.save_state(update).await?;
        }

        // Update metadata once with the last entry's log_id
        if let Some(log_id) = last_log_id {
            self.save_meta(|m| m.last_log_id = Some(log_id)).await?;
        }

        callback.io_completed(Ok(()));
        Ok(())
    }

    async fn truncate_after(&mut self, last_log_id: Option<LogId<OpenRaftTypes<T>>>) -> Result<(), std::io::Error> {
        self.save_meta(|m| {
            m.last_log_id = last_log_id.map(|id| id.to_type());
        }).await
    }

    async fn purge(&mut self, log_id: LogId<OpenRaftTypes<T>>) -> Result<(), std::io::Error> {
        self.save_meta(|m| m.last_purged = Some(log_id.to_type())).await
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }
}

// Implement RaftLogReader for Arc<StorageAdapter>
impl<T, S, M> RaftLogReader<OpenRaftTypes<T>> for Arc<StorageAdapter<T, S, M>>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    async fn read_vote(&mut self) -> Result<Option<<OpenRaftTypes<T> as RaftTypeConfig>::Vote>, std::io::Error> {
        let state = self.storage_state.lock().await;
        Ok(state.cached_meta.vote)
    }

    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<<OpenRaftTypes<T> as RaftTypeConfig>::Entry>, std::io::Error> {
        // Available log range: [lo, hi)
        let (lo, hi) = {
            let state = self.storage_state.lock().await;
            let lo = state.cached_meta.last_purged.map(|(_, i)| i).next_index();
            let hi = state.cached_meta.last_log_id.map(|(_, i)| i).next_index();
            (lo, hi)
        };

        let start = match range.start_bound() {
            std::ops::Bound::Included(&x) => x,
            std::ops::Bound::Excluded(&x) => x + 1,
            std::ops::Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            std::ops::Bound::Included(&x) => x + 1,
            std::ops::Bound::Excluded(&x) => x,
            std::ops::Bound::Unbounded => hi,
        };

        // Clamp to available range
        let start = std::cmp::max(start, lo);
        let end = std::cmp::min(end, hi);

        if start >= end {
            return Ok(Vec::new());
        }

        // Load only the requested range from user storage
        let mut state = self.storage_state.lock().await;
        state.storage.load_log_range(start, end).await
    }
}

// Implement RaftStateMachine for Arc<StorageAdapter>
impl<T, S, M> RaftStateMachine<OpenRaftTypes<T>> for Arc<StorageAdapter<T, S, M>>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    type SnapshotBuilder = Self;

    async fn applied_state(
        &mut self,
    ) -> Result<(Option<LogId<OpenRaftTypes<T>>>, StoredMembership<OpenRaftTypes<T>>), std::io::Error> {
        // TODO: return `last_applied` instead of `last_log_id`
        //
        // Currently returns `last_log_id` (last entry in log storage), but should return
        // `last_applied` (last entry applied to state machine). These differ when log
        // has entries not yet applied.
        //
        // To fix:
        // 1. Add `last_applied: Option<EzLogId>` field to `EzMeta`
        // 2. Update `last_applied` in `apply()` after applying each entry
        // 3. Return `last_applied` here instead of `last_log_id`
        let (last, membership) = {
            let mut state = self.storage_state.lock().await;
            let last = state.cached_meta.last_log_id.map(|(t, i)| LogId::new_term_index(t, i));

            // Load membership from storage or return default if none exists
            let membership = match state.storage.load_state().await? {
                Some((_, Some((ref snapshot_meta, _)))) => {
                    let (term, index) = snapshot_meta.last_log_id;
                    StoredMembership::new(
                        Some(LogId::new_term_index(term, index)),
                        snapshot_meta.membership.clone(),
                    )
                }
                _ => StoredMembership::new(
                    Some(LogId::new_term_index(0, 0)),
                    Membership::<OpenRaftTypes<T>>::default(),
                ),
            };

            (last, membership)
        };

        Ok((last, membership))
    }

    async fn apply<Strm>(&mut self, entries: Strm) -> Result<(), std::io::Error>
    where Strm: futures::Stream<
                Item = Result<
                    (
                        <OpenRaftTypes<T> as RaftTypeConfig>::Entry,
                        Option<ApplyResponder<OpenRaftTypes<T>>>,
                    ),
                    std::io::Error,
                >,
            > + Send
            + Unpin {
        let mut state = self.user_state.lock().await;

        let mut entries = entries;
        while let Some(res) = entries.next().await {
            let (entry, responder) = res.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            if let EntryPayload::Normal(req) = entry.payload {
                let resp = state.apply(req).await;
                if let Some(responder) = responder {
                    responder.send(resp);
                }
            }
        }

        Ok(())
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }

    async fn begin_receiving_snapshot(&mut self) -> Result<SnapshotDataOf<T>, std::io::Error> {
        Ok(Cursor::new(Vec::new()))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<OpenRaftTypes<T>>,
        snapshot: SnapshotDataOf<T>,
    ) -> Result<(), std::io::Error> {
        // Convert OpenRaft snapshot metadata to EzSnapshotMeta
        let last_log_id = meta.last_log_id.unwrap();
        let ez_meta = EzSnapshotMeta {
            last_log_id: last_log_id.to_type(),
            membership: meta.last_membership.clone().membership().clone(),
        };

        // Extract snapshot data
        let mut cursor = snapshot;
        cursor.seek(SeekFrom::Start(0))?;
        let mut data = Vec::new();
        cursor.read_to_end(&mut data)?;

        let mut state = self.storage_state.lock().await;
        state.cached_meta.last_log_id = Some(ez_meta.last_log_id);
        let update = EzStateUpdate::WriteSnapshot(ez_meta, data);
        state.storage.save_state(update).await?;

        Ok(())
    }

    async fn get_current_snapshot(&mut self) -> Result<Option<Snapshot<OpenRaftTypes<T>>>, std::io::Error> {
        let snapshot = {
            let mut state = self.storage_state.lock().await;
            state.storage.load_state().await?.and_then(|(_, snap)| snap)
        };

        match snapshot {
            Some((ez_meta, data)) => {
                let (term, index) = ez_meta.last_log_id;
                let last_log_id = LogId::new_term_index(term, index);

                let snapshot_meta = SnapshotMeta {
                    last_log_id: Some(last_log_id),
                    last_membership: StoredMembership::new(Some(last_log_id), ez_meta.membership),
                    snapshot_id: format!("{}-{}", term, index),
                };

                Ok(Some(Snapshot {
                    meta: snapshot_meta,
                    snapshot: Cursor::new(data),
                }))
            }
            None => Ok(None),
        }
    }
}

// Implement RaftSnapshotBuilder for Arc<StorageAdapter>
impl<T, S, M> RaftSnapshotBuilder<OpenRaftTypes<T>> for Arc<StorageAdapter<T, S, M>>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    async fn build_snapshot(&mut self) -> Result<Snapshot<OpenRaftTypes<T>>, std::io::Error> {
        // Get current state
        let (last_log_id, membership) = {
            let state = self.storage_state.lock().await;
            (
                state.cached_meta.last_log_id.unwrap_or((0, 0)),
                Membership::<OpenRaftTypes<T>>::default(),
            )
        };

        // Serialize state machine state to snapshot data
        let snapshot_data = {
            // For now, we use empty snapshot data since we don't have a way to serialize
            // the user's state machine. Users who need snapshots should implement
            // snapshotting in their state machine.
            Vec::new()
        };

        let (term, index) = last_log_id;
        let log_id = LogId::new_term_index(term, index);

        let snapshot_meta = SnapshotMeta {
            last_log_id: Some(log_id),
            last_membership: StoredMembership::new(Some(log_id), membership),
            snapshot_id: format!("{}-{}", term, index),
        };

        Ok(Snapshot {
            meta: snapshot_meta,
            snapshot: Cursor::new(snapshot_data),
        })
    }
}
