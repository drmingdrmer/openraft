use std::collections::VecDeque;
use std::fmt::Debug;
use std::io;
use std::ops::RangeBounds;
use std::pin::pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Poll;
use std::time::Duration;

use anyhow::Result;
use openraft::Config;
use openraft::LogState;
use openraft::OptionalSend;
use openraft::Raft;
use openraft::RaftLogReader;
use openraft::Vote;
use openraft::alias::EntryOf;
use openraft::alias::LogIdOf;
use openraft::alias::VoteOf;
use openraft::async_runtime::OneshotSender;
use openraft::raft::AppendEntriesRequest;
use openraft::storage::IOFlushed;
use openraft::storage::RaftLogStorage;
use openraft::testing::blank_ent;
use openraft::type_config::TypeConfigExt;
use openraft_memstore::MemLogStore;
use openraft_memstore::TypeConfig;

use crate::fixtures::RaftRouter;
use crate::fixtures::WAIT_TIMEOUT;
use crate::fixtures::log_id;
use crate::fixtures::ut_harness;

/// A log store that stores each append at once but holds its flush until the test releases it, so
/// that appends complete one at a time, in order.
#[derive(Clone)]
struct HeldLogStore {
    inner: Arc<MemLogStore>,
    held: Arc<Mutex<VecDeque<IOFlushed<TypeConfig>>>>,
}

impl HeldLogStore {
    fn held(&self) -> usize {
        self.held.lock().unwrap().len()
    }

    /// Complete the oldest held append.
    fn flush_one(&self) {
        let callback = self.held.lock().unwrap().pop_front().expect("an append is held");
        callback.io_completed(Ok(()));
    }
}

impl RaftLogReader<TypeConfig> for HeldLogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<EntryOf<TypeConfig>>, io::Error> {
        self.inner.try_get_log_entries(range).await
    }

    async fn read_vote(&mut self) -> Result<Option<VoteOf<TypeConfig>>, io::Error> {
        self.inner.read_vote().await
    }
}

impl RaftLogStorage<TypeConfig> for HeldLogStore {
    type LogReader = Arc<MemLogStore>;

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, io::Error> {
        self.inner.get_log_state().await
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.inner.get_log_reader().await
    }

    async fn save_vote(&mut self, vote: &VoteOf<TypeConfig>) -> Result<(), io::Error> {
        self.inner.save_vote(vote).await
    }

    async fn save_committed(&mut self, committed: Option<LogIdOf<TypeConfig>>) -> Result<(), io::Error> {
        self.inner.save_committed(committed).await
    }

    async fn read_committed(&mut self) -> Result<Option<LogIdOf<TypeConfig>>, io::Error> {
        self.inner.read_committed().await
    }

    async fn append<I>(&mut self, entries: I, callback: IOFlushed<TypeConfig>) -> Result<(), io::Error>
    where
        I: IntoIterator<Item = EntryOf<TypeConfig>> + OptionalSend,
        I::IntoIter: OptionalSend,
    {
        self.inner.append(entries, IOFlushed::noop()).await?;
        self.held.lock().unwrap().push_back(callback);
        Ok(())
    }

    async fn truncate_after(&mut self, last_log_id: Option<LogIdOf<TypeConfig>>) -> Result<(), io::Error> {
        self.inner.truncate_after(last_log_id).await
    }

    async fn purge(&mut self, log_id: LogIdOf<TypeConfig>) -> Result<(), io::Error> {
        self.inner.purge(log_id).await
    }
}

/// A follower answers an append once the logs of that append are flushed, even when an append
/// that waits for a later log arrived before it.
///
/// Two overlapping appends end at logs 0 and 1, then a retry of the first one ends at log 0 again,
/// all before the log store flushes anything. Flushing log 0 answers both requests for log 0 while
/// the one for log 1 waits; flushing log 1 answers it.
#[tracing::instrument]
#[test_harness::test(harness = ut_harness)]
async fn append_reply_waits_only_for_its_own_logs() -> Result<()> {
    let config = Config {
        enable_heartbeat: false,
        enable_elect: false,
        ..Default::default()
    };
    // The router serves only as the network factory: this follower sends no RPC.
    let router = RaftRouter::new(config.clone())?;
    let (inner, state_machine) = openraft_memstore::new_mem_store();
    let log_store = HeldLogStore {
        inner,
        held: Default::default(),
    };
    let config = Arc::new(config.validate()?);
    let raft = Raft::new(1, config, router, log_store.clone(), state_machine).await?;

    let request = |prev: Option<u64>, index: u64| AppendEntriesRequest {
        vote: Vote::new_committed(1, 0),
        prev_log_id: prev.map(|i| log_id(1, 0, i)),
        entries: vec![blank_ent::<TypeConfig>(1, 0, index)],
        leader_commit: None,
    };

    tracing::info!("--- append logs 0 and 1, then retry log 0, while the store holds the flushes");
    let [to_0, mut to_1, retry_0] = {
        let mut replies = vec![];
        for rpc in [request(None, 0), request(Some(0), 1), request(None, 0)] {
            let raft = raft.clone();
            let (sent_tx, sent_rx) = TypeConfig::oneshot();
            replies.push(TypeConfig::spawn(async move {
                let mut reply = pin!(raft.append_entries(rpc));
                // The first poll sends the request; RaftCore handles requests in order.
                let polled = futures::poll!(reply.as_mut());
                sent_tx.send(()).ok();
                match polled {
                    Poll::Ready(reply) => reply,
                    Poll::Pending => reply.await,
                }
            }));
            sent_rx.await?;
        }
        // RaftCore handles this request after the three appends, which have all waited by then.
        raft.with_raft_state(|_| ()).await?;
        assert_eq!(2, log_store.held(), "the appends of logs 0 and 1 wait to flush");
        replies.try_into().unwrap_or_else(|_| unreachable!("three requests"))
    };

    tracing::info!("--- flush log 0: both requests that end at log 0 are answered");
    {
        log_store.flush_one();
        for reply in [to_0, retry_0] {
            let reply = TypeConfig::timeout(WAIT_TIMEOUT, reply).await???;
            assert_eq!(Ok(Some(log_id(1, 0, 0))), reply);
        }
        let waiting = TypeConfig::timeout(Duration::from_millis(200), &mut to_1).await;
        assert!(waiting.is_err(), "the request that ends at log 1 waits for its flush");
    }

    tracing::info!("--- flush log 1: the request that ends at log 1 is answered");
    {
        log_store.flush_one();
        let reply = TypeConfig::timeout(WAIT_TIMEOUT, to_1).await???;
        assert_eq!(Ok(Some(log_id(1, 0, 1))), reply);
    }

    raft.shutdown().await?;
    Ok(())
}
