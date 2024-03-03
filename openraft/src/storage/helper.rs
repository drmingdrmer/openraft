use std::marker::PhantomData;
use std::sync::Arc;

use crate::display_ext::DisplayOptionExt;
use crate::engine::LogIdList;
use crate::entry::RaftPayload;
use crate::log_id::RaftLogId;
use crate::raft_state::io_state::log_io_id::LogIOId;
use crate::raft_state::IOState;
use crate::raft_state::LogIOId;
use crate::storage::LogIO;
use crate::storage::RaftLogReaderExt;
use crate::storage::RaftLogStorage;
use crate::storage::RaftStateMachine;
use crate::type_config::TypeConfigExt;
use crate::utime::UTime;
use crate::EffectiveMembership;
use crate::LogIdOptionExt;
use crate::MembershipState;
use crate::RaftSnapshotBuilder;
use crate::RaftState;
use crate::RaftTypeConfig;
use crate::StorageError;
use crate::StoredMembership;

/// StorageHelper provides additional methods to access a [`RaftLogStorage`] and
/// [`RaftStateMachine`] implementation.
pub struct StorageHelper<'a, C, LS, SM>
where
    C: RaftTypeConfig,
    LS: RaftLogStorage<C>,
    SM: RaftStateMachine<C>,
{
    pub(crate) log_store: &'a mut LS,
    pub(crate) state_machine: &'a mut SM,
    _p: PhantomData<C>,
}

impl<'a, C, LS, SM> StorageHelper<'a, C, LS, SM>
where
    C: RaftTypeConfig,
    LS: RaftLogStorage<C>,
    SM: RaftStateMachine<C>,
{
    /// Creates a new `StorageHelper` that provides additional functions based on the underlying
    ///  [`RaftLogStorage`] and [`RaftStateMachine`] implementation.
    pub fn new(sto: &'a mut LS, sm: &'a mut SM) -> Self {
        Self {
            log_store: sto,
            state_machine: sm,
            _p: Default::default(),
        }
    }

    // TODO: let RaftStore store node-id.
    //       To achieve this, RaftLogStorage must store node-id
    //       To achieve this, RaftLogStorage has to provide API to initialize with a node id and API to
    //       read node-id
    /// Get Raft's state information from storage.
    ///
    /// When the Raft node is first started, it will call this interface to fetch the last known
    /// state from stable storage.
    pub async fn get_initial_state(&mut self) -> Result<RaftState<C>, StorageError<C>> {
    {
        // 1. Load log state

        let mut log_meta = self.log_store.read_meta().await?;

        tracing::info!(meta = display(&log_meta), "get_initial_state");

        let (mut last_applied, _) = self.state_machine.applied_state().await?;

        // TODO: It is possible `committed < last_applied` because when installing snapshot,
        //       new committed should be saved, but not yet.
        if log_meta.committed < last_applied {
            log_meta.committed = last_applied;
        }

        // Re-apply log entries to recover SM to latest state.
        if last_applied < log_meta.committed {
            let start = last_applied.next_index();
            let end = log_meta.committed.next_index();

            tracing::info!("re-apply log {}..{} to state machine", start, end);

            let entries = self.log_store.get_log_entries(start..end).await?;
            self.state_machine.apply(entries).await?;

            last_applied = log_meta.committed;
        }

        // In case the `purged` in meta is written to log store but the actual purge did not finish.
        if let Some(purged) = log_meta.purged {
            self.log_store.blocking_write(LogIO::purge(purged)).await?;
        }
        // In case the `truncate` in meta is written to log store but the actual truncate did not finish.
        if let Some(last) = log_meta.last {
            self.log_store.blocking_write(LogIO::truncate(last)).await?;
        }

        // Clean up dirty state: snapshot is installed but logs are not cleaned.
        if log_meta.last < last_applied {
            tracing::info!(
                "Clean the hole between last_log_id({}) and last_applied({}) by purging logs to {}",
                log_meta.last.display(),
                last_applied.display(),
                last_applied.display(),
            );

            log_meta.last = last_applied;
            log_meta.purged = last_applied;

            self.log_store.blocking_write(LogIO::meta(log_meta.clone())).await?;
            self.log_store.blocking_write(LogIO::purge(log_meta.purged.unwrap())).await?;
        }

        tracing::info!(
            "load key log ids from ({},{}]",
            log_meta.purged.display(),
            log_meta.last.display()
        );
        let log_ids = LogIdList::load_log_ids(log_meta.purged, log_meta.last, self.log_store).await?;

        let mem_state = self.get_membership().await?;

        let snapshot = self.state_machine.get_current_snapshot().await?;

        // If there is not a snapshot and there are logs purged, which means the snapshot is not persisted,
        // we just rebuild it so that replication can use it.
        let snapshot = match snapshot {
            None => {
                if log_meta.purged.is_some() {
                    let mut b = self.state_machine.get_snapshot_builder().await;
                    let s = b.build_snapshot().await?;
                    Some(s)
                } else {
                    None
                }
            }
            s @ Some(_) => s,
        };
        let snapshot_meta = snapshot.map(|x| x.meta).unwrap_or_default();

        // TODO: `flushed` is not set.
        let io_state = IOState::new(
            log_meta.vote,
            LogIOId::default(),
            last_applied,
            snapshot_meta.last_log_id,
            log_meta.purged,
        );

        let now = C::now();

        Ok(RaftState {
            committed: last_applied,
            // The initial value for `vote` is the minimal possible value.
            // See: [Conditions for initialization][precondition]
            //
            // [precondition]: crate::docs::cluster_control::cluster_formation#preconditions-for-initialization
            vote: UTime::new(now, log_meta.vote),
            purged_next: log_meta.purged.next_index(),
            log_ids,
            membership_state: mem_state,
            snapshot_meta,

            // -- volatile fields: they are not persisted.
            server_state: Default::default(),
            accepted: Default::default(),
            io_state,
            purge_upto: log_meta.purged,
        })
    }

    /// Returns the last 2 membership config found in log or state machine.
    ///
    /// A raft node needs to store at most 2 membership config log:
    /// - The first one must be committed, because raft allows to propose new membership only when
    ///   the previous one is committed.
    /// - The second may be committed or not.
    ///
    /// Because when handling append-entries RPC, (1) a raft follower will delete logs that are
    /// inconsistent with the leader,
    /// and (2) a membership will take effect at once it is written,
    /// a follower needs to revert the effective membership to a previous one.
    ///
    /// And because (3) there is at most one outstanding, uncommitted membership log,
    /// a follower only need to revert at most one membership log.
    ///
    /// Thus a raft node will only need to store at most two recent membership logs.
    pub async fn get_membership(&mut self) -> Result<MembershipState<C>, StorageError<C>> {
        let (last_applied, sm_mem) = self.state_machine.applied_state().await?;

        let log_mem = self.last_membership_in_log(last_applied.next_index()).await?;
        tracing::debug!(membership_in_sm=?sm_mem, membership_in_log=?log_mem, "{}", func_name!());

        // There 2 membership configs in logs.
        if log_mem.len() == 2 {
            return Ok(MembershipState::new(
                Arc::new(EffectiveMembership::new_from_stored_membership(log_mem[0].clone())),
                Arc::new(EffectiveMembership::new_from_stored_membership(log_mem[1].clone())),
            ));
        }

        let effective = if log_mem.is_empty() {
            EffectiveMembership::new_from_stored_membership(sm_mem.clone())
        } else {
            EffectiveMembership::new_from_stored_membership(log_mem[0].clone())
        };

        let res = MembershipState::new(
            Arc::new(EffectiveMembership::new_from_stored_membership(sm_mem)),
            Arc::new(effective),
        );

        Ok(res)
    }

    /// Get the last 2 membership configs found in the log.
    ///
    /// This method returns at most membership logs with greatest log index which is
    /// `>=since_index`. If no such membership log is found, it returns `None`, e.g., when logs
    /// are cleaned after being applied.
    #[tracing::instrument(level = "trace", skip_all)]
    pub async fn last_membership_in_log(
        &mut self,
        since_index: u64,
    ) -> Result<Vec<StoredMembership<C>>, StorageError<C>> {
        let log_meta = self.log_store.read_meta().await?;

        let mut end = log_meta.last.next_index();

        tracing::info!("load membership from log: [{}..{})", since_index, end);

        let start = std::cmp::max(log_meta.purged.next_index(), since_index);
        let step = 64;

        let mut res = vec![];

        while start < end {
            let step_start = std::cmp::max(start, end.saturating_sub(step));
            let entries = self.log_store.try_get_log_entries(step_start..end).await?;

            for ent in entries.iter().rev() {
                if let Some(mem) = ent.get_membership() {
                    let em = StoredMembership::new(Some(*ent.get_log_id()), mem.clone());
                    res.insert(0, em);
                    if res.len() == 2 {
                        return Ok(res);
                    }
                }
            }

            end = end.saturating_sub(step);
        }

        Ok(res)
    }
}
