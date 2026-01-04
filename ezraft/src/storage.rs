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
pub struct StorageState<T, S>
where
    T: EzTypes,
    S: EzStorage<T>,
{
    pub storage: S,
    pub cached_meta: EzMeta<T>,
}

/// Internal state machine wrapper that tracks Raft metadata
/// alongside the user's business logic state machine
pub struct StateMachineState<T, M>
where
    T: EzTypes,
    M: EzStateMachine<T>,
{
    /// User's state machine for business logic
    pub user_sm: M,

    /// Last log ID applied to the state machine
    pub last_applied: Option<LogId<OpenRaftTypes<T>>>,

    /// Last membership applied to the state machine
    pub membership: StoredMembership<OpenRaftTypes<T>>,
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
    pub storage_state: Arc<Mutex<StorageState<T, S>>>,
    pub sm_state: Arc<Mutex<StateMachineState<T, M>>>,
    _phantom: PhantomData<T>,
}

impl<T, S, M> StorageAdapter<T, S, M>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    /// Create a new storage adapter and load initial metadata
    pub async fn new(mut user_storage: S, user_sm: M) -> Result<Self, std::io::Error> {
        // Load initial metadata and snapshot
        let (cached_meta, snapshot) = user_storage.restore().await?;

        // Initialize state machine state from snapshot or defaults
        let (last_applied, last_membership) = match &snapshot {
            Some(snap) => (snap.meta.last_log_id, snap.meta.last_membership.clone()),
            None => (None, StoredMembership::new(None, Membership::default())),
        };

        let storage_state = StorageState {
            storage: user_storage,
            cached_meta,
        };

        let sm_state = StateMachineState {
            user_sm,
            last_applied,
            membership: last_membership,
        };

        Ok(Self {
            storage_state: Arc::new(Mutex::new(storage_state)),
            sm_state: Arc::new(Mutex::new(sm_state)),
            _phantom: PhantomData,
        })
    }

    /// Update metadata and persist to storage
    async fn save_meta(&self, f: impl FnOnce(&mut EzMeta<T>)) -> Result<(), std::io::Error> {
        let mut state = self.storage_state.lock().await;
        f(&mut state.cached_meta);
        let update = Persist::Meta(state.cached_meta.clone());
        state.storage.persist(update).await
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
            let update = Persist::LogEntry(entry);
            let mut state = self.storage_state.lock().await;
            state.storage.persist(update).await?;
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
        })
        .await
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
        state.storage.read_logs(start, end).await
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
        let sm = self.sm_state.lock().await;
        Ok((sm.last_applied, sm.membership.clone()))
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
        let mut sm = self.sm_state.lock().await;

        let mut entries = entries;
        while let Some(res) = entries.next().await {
            let (entry, responder) = res.map_err(std::io::Error::other)?;

            // Update last_applied for every entry
            let (term, index) = entry.log_id;
            let log_id = LogId::new_term_index(term, index);
            sm.last_applied = Some(log_id);

            let resp = match entry.payload {
                EntryPayload::Normal(req) => sm.user_sm.apply(req).await,
                EntryPayload::Membership(membership) => {
                    sm.membership = StoredMembership::new(Some(log_id), membership);
                    T::Response::default()
                }
                EntryPayload::Blank => T::Response::default(),
            };

            if let Some(responder) = responder {
                responder.send(resp);
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
        snapshot_meta: &SnapshotMeta<OpenRaftTypes<T>>,
        snapshot_data: SnapshotDataOf<T>,
    ) -> Result<(), std::io::Error> {
        // Extract snapshot data
        let mut cursor = snapshot_data;
        cursor.seek(SeekFrom::Start(0))?;
        let mut data = Vec::new();
        cursor.read_to_end(&mut data)?;

        // Update storage state
        {
            let mut state = self.storage_state.lock().await;
            state.cached_meta.last_log_id = snapshot_meta.last_log_id.map(|id| id.to_type());
            let snapshot = Snapshot {
                meta: snapshot_meta.clone(),
                snapshot: Cursor::new(data.clone()),
            };
            let update = Persist::Snapshot(snapshot);
            state.storage.persist(update).await?;
        }

        // Update state machine state and restore user state from snapshot
        {
            let mut sm = self.sm_state.lock().await;
            sm.last_applied = snapshot_meta.last_log_id;
            sm.membership = snapshot_meta.last_membership.clone();
            sm.user_sm.install_snapshot(&data).await?;
        }

        Ok(())
    }

    async fn get_current_snapshot(&mut self) -> Result<Option<Snapshot<OpenRaftTypes<T>>>, std::io::Error> {
        let mut state = self.storage_state.lock().await;
        Ok(state.storage.restore().await?.1)
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
        // Get current state machine state and build snapshot data
        let (last_applied, last_membership, snapshot_data) = {
            let sm = self.sm_state.lock().await;
            let data = sm.user_sm.build_snapshot().await?;
            (sm.last_applied, sm.membership.clone(), data)
        };

        let snapshot_id = match last_applied {
            Some(log_id) => format!("{}-{}", log_id.leader_id.term, log_id.index),
            None => "0-0".to_string(),
        };

        let snapshot_meta = SnapshotMeta {
            last_log_id: last_applied,
            last_membership,
            snapshot_id,
        };

        Ok(Snapshot {
            meta: snapshot_meta,
            snapshot: Cursor::new(snapshot_data),
        })
    }
}
