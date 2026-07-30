//! Cluster-level tests driving EzRaft through its public API only: real HTTP
//! between nodes, in-memory persistence that survives a node instance so a
//! restart reads exactly what the previous incarnation persisted.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use ezraft::EzConfig;
use ezraft::EzEntry;
use ezraft::EzMeta;
use ezraft::EzRaft;
use ezraft::EzSnapshot;
use ezraft::EzSnapshotMeta;
use ezraft::EzStateMachine;
use ezraft::EzStorage;
use ezraft::EzTypes;
use ezraft::Loaded;
use ezraft::Persist;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, derive_more::Display)]
enum Request {
    #[display("Set({key})")]
    Set { key: String, value: String },
    #[display("Get({key})")]
    Get { key: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Response {
    value: Option<String>,
}

struct Types;
impl EzTypes for Types {
    type Request = Request;
    type Response = Response;
}

fn set(key: &str, value: &str) -> Request {
    Request::Set {
        key: key.into(),
        value: value.into(),
    }
}

fn get(key: &str) -> Request {
    Request::Get { key: key.into() }
}

/// KV state machine whose data is shared with the test, so assertions can
/// compare the whole applied state instead of sampling it through reads.
#[derive(Clone, Default)]
struct KvSm {
    data: Arc<Mutex<BTreeMap<String, String>>>,
}

#[async_trait]
impl EzStateMachine<Types> for KvSm {
    async fn apply(&mut self, req: Request) -> Response {
        let mut data = self.data.lock().unwrap();
        match req {
            Request::Set { key, value } => {
                data.insert(key, value);
                Response { value: None }
            }
            Request::Get { key } => Response {
                value: data.get(&key).cloned(),
            },
        }
    }

    async fn build_snapshot(&self) -> io::Result<Vec<u8>> {
        serde_json::to_vec(&*self.data.lock().unwrap()).map_err(io::Error::other)
    }

    async fn install_snapshot(&mut self, data: &[u8]) -> io::Result<()> {
        *self.data.lock().unwrap() = serde_json::from_slice(data)?;
        Ok(())
    }
}

/// What one node has "written to disk"; kept outside the storage instance so a
/// restarted node starts from it, exactly like a process reading real files.
#[derive(Default)]
struct Disk {
    meta: EzMeta,
    /// Log entries as serialized bytes, the same shape a real disk would hold.
    logs: BTreeMap<u64, Vec<u8>>,
    snapshot: Option<(EzSnapshotMeta, Vec<u8>)>,
}

#[derive(Clone, Default)]
struct MemStorage {
    disk: Arc<Mutex<Disk>>,
}

#[async_trait]
impl EzStorage<Types> for MemStorage {
    async fn load(&mut self) -> io::Result<Loaded> {
        let disk = self.disk.lock().unwrap();
        let snapshot = disk.snapshot.as_ref().map(|(meta, data)| EzSnapshot {
            meta: meta.clone(),
            snapshot: Cursor::new(data.clone()),
        });
        Ok(Loaded {
            meta: disk.meta.clone(),
            snapshot,
        })
    }

    async fn persist(&mut self, op: Persist<Types>) -> io::Result<()> {
        let mut disk = self.disk.lock().unwrap();
        match op {
            Persist::Meta(meta) => disk.meta = meta,
            Persist::LogEntry(entry) => {
                disk.logs.insert(entry.log_id.1, serde_json::to_vec(&entry)?);
            }
            Persist::Snapshot(snapshot) => {
                disk.snapshot = Some((snapshot.meta, snapshot.snapshot.into_inner()));
            }
            Persist::TruncateLogs(from) => disk.logs.retain(|&index, _| index < from),
            Persist::PurgeLogs(upto) => disk.logs.retain(|&index, _| index > upto),
        }
        Ok(())
    }

    async fn read_logs(&mut self, start: u64, end: u64) -> io::Result<Vec<EzEntry<Types>>> {
        let disk = self.disk.lock().unwrap();
        (start..end)
            .map(|index| {
                let data =
                    disk.logs.get(&index).ok_or_else(|| io::Error::other(format!("missing log entry {}", index)))?;
                Ok(serde_json::from_slice(data)?)
            })
            .collect()
    }
}

/// Grab a free port; the listener is dropped so EzRaft can bind it.
fn free_addr() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().to_string()
}

/// Short heartbeat so elections and failovers finish in test time.
fn config() -> EzConfig {
    EzConfig {
        heartbeat_interval: Duration::from_millis(100),
    }
}

/// Upper bound for every cluster-state wait in these tests
const WAIT: Option<Duration> = Some(Duration::from_secs(30));

fn expected_map(range: std::ops::Range<u32>) -> BTreeMap<String, String> {
    range.map(|i| (format!("k{}", i), format!("v{}", i))).collect()
}

/// Joining must be all it takes to become a voter, and the promoted voters
/// must keep the cluster alive when the founding leader dies.
#[tokio::test(flavor = "multi_thread")]
async fn join_promotes_to_voter_and_cluster_survives_leader_death() -> io::Result<()> {
    let addr_a = free_addr();
    let a = EzRaft::<Types>::create(&addr_a, KvSm::default(), MemStorage::default(), config()).await?;
    tokio::spawn({
        let a = a.clone();
        async move { a.serve().await }
    });
    a.inner()
        .wait(WAIT)
        .metrics(|m| m.current_leader == Some(0), "founding node leads")
        .await
        .map_err(io::Error::other)?;

    let addr_b = free_addr();
    let b = EzRaft::<Types>::join(&addr_b, &addr_a, KvSm::default(), MemStorage::default(), config()).await?;
    tokio::spawn({
        let b = b.clone();
        async move { b.serve().await }
    });

    let addr_c = free_addr();
    let c = EzRaft::<Types>::join(&addr_c, &addr_a, KvSm::default(), MemStorage::default(), config()).await?;
    tokio::spawn({
        let c = c.clone();
        async move { c.serve().await }
    });

    // Every node must see the final uniform config with all three voters before
    // the leader may die: a joint config still counts the founding node in its
    // old majority, so killing it mid-change would legitimately lose quorum.
    let voters = BTreeSet::from([0, b.node_id(), c.node_id()]);
    for node in [&a, &b, &c] {
        node.inner()
            .wait(WAIT)
            .metrics(
                |m| *m.membership_config.membership().get_joint_config() == [voters.clone()],
                "every joined node promoted to voter",
            )
            .await
            .map_err(io::Error::other)?;
    }

    assert_eq!(Response { value: None }, a.write(set("k1", "v1")).await?);

    // Kill the leader; the two remaining voters still form a quorum.
    assert!(a.is_leader());
    a.inner().shutdown().await.map_err(io::Error::other)?;

    b.inner()
        .wait(WAIT)
        .metrics(
            |m| matches!(m.current_leader, Some(id) if id != 0),
            "a surviving node takes over",
        )
        .await
        .map_err(io::Error::other)?;

    // The new leader accepts writes (reached from a follower via forwarding)
    // and still has the data acknowledged before the failover.
    assert_eq!(Response { value: None }, b.write(set("k2", "v2")).await?);
    assert_eq!(
        Response {
            value: Some("v1".into())
        },
        b.write(get("k1")).await?
    );
    assert_eq!(
        Response {
            value: Some("v2".into())
        },
        b.write(get("k2")).await?
    );

    Ok(())
}

/// A promotion interrupted by the death of the node that admitted the joiner
/// must be finished by the next leader: promotion belongs to the leader role,
/// not to the task that handled the join.
#[tokio::test(flavor = "multi_thread")]
async fn next_leader_promotes_orphaned_learner() -> io::Result<()> {
    let addr_a = free_addr();
    let a = EzRaft::<Types>::create(&addr_a, KvSm::default(), MemStorage::default(), config()).await?;
    tokio::spawn({
        let a = a.clone();
        async move { a.serve().await }
    });
    a.inner()
        .wait(WAIT)
        .metrics(|m| m.current_leader == Some(0), "founding node leads")
        .await
        .map_err(io::Error::other)?;

    let addr_b = free_addr();
    let b = EzRaft::<Types>::join(&addr_b, &addr_a, KvSm::default(), MemStorage::default(), config()).await?;
    tokio::spawn({
        let b = b.clone();
        async move { b.serve().await }
    });

    let addr_c = free_addr();
    let c = EzRaft::<Types>::join(&addr_c, &addr_a, KvSm::default(), MemStorage::default(), config()).await?;
    tokio::spawn({
        let c = c.clone();
        async move { c.serve().await }
    });

    let voters = BTreeSet::from([0, b.node_id(), c.node_id()]);
    for node in [&a, &b, &c] {
        node.inner()
            .wait(WAIT)
            .metrics(
                |m| *m.membership_config.membership().get_joint_config() == [voters.clone()],
                "three voters before the fourth joins",
            )
            .await
            .map_err(io::Error::other)?;
    }

    // D joins but its server does not start: it cannot catch up, so it stays a
    // learner, holding its promotion open past the founding leader's death.
    let addr_d = free_addr();
    let d = EzRaft::<Types>::join(&addr_d, &addr_a, KvSm::default(), MemStorage::default(), config()).await?;
    let d_id = d.node_id();
    a.inner()
        .wait(WAIT)
        .metrics(
            |m| m.membership_config.membership().learner_ids().any(|id| id == d_id),
            "joiner registered as learner",
        )
        .await
        .map_err(io::Error::other)?;

    a.inner().shutdown().await.map_err(io::Error::other)?;
    b.inner()
        .wait(WAIT)
        .metrics(
            |m| matches!(m.current_leader, Some(id) if id != 0),
            "a surviving node takes over",
        )
        .await
        .map_err(io::Error::other)?;

    // Only now does D serve and catch up. The node that admitted it is dead,
    // so only the new leader can complete the promotion.
    tokio::spawn({
        let d = d.clone();
        async move { d.serve().await }
    });

    let final_voters = BTreeSet::from([0, b.node_id(), c.node_id(), d_id]);
    b.inner()
        .wait(WAIT)
        .metrics(
            |m| m.membership_config.membership().voter_ids().collect::<BTreeSet<_>>() == final_voters,
            "next leader promotes the orphaned learner",
        )
        .await
        .map_err(io::Error::other)?;

    Ok(())
}

/// A snapshot must land on disk when built, and a restarted node must rebuild
/// the full state from that snapshot plus the log entries after it.
#[tokio::test(flavor = "multi_thread")]
async fn snapshot_survives_restart() -> io::Result<()> {
    let addr = free_addr();
    let storage = MemStorage::default();

    let a = EzRaft::<Types>::create(&addr, KvSm::default(), storage.clone(), config()).await?;
    a.inner()
        .wait(WAIT)
        .metrics(|m| m.current_leader == Some(0), "single node leads")
        .await
        .map_err(io::Error::other)?;

    for i in 0..10 {
        a.write(set(&format!("k{}", i), &format!("v{}", i))).await?;
    }

    a.inner().trigger().snapshot().await.map_err(io::Error::other)?;
    a.inner()
        .wait(WAIT)
        .metrics(|m| m.snapshot.is_some(), "snapshot built")
        .await
        .map_err(io::Error::other)?;

    // The snapshot must be on disk with the full applied state: a restart
    // reads only the disk, so an unpersisted snapshot is lost data.
    {
        let disk = storage.disk.lock().unwrap();
        let (meta, data) = disk.snapshot.as_ref().expect("snapshot persisted to storage");
        let snapshot_state: BTreeMap<String, String> = serde_json::from_slice(data)?;
        assert_eq!(expected_map(0..10), snapshot_state);
        assert!(meta.last_log_id.is_some());
    }

    // More writes after the snapshot: the restart must replay these on top.
    for i in 10..15 {
        a.write(set(&format!("k{}", i), &format!("v{}", i))).await?;
    }

    a.inner().shutdown().await.map_err(io::Error::other)?;
    drop(a);

    // Restart on the same disk with an empty state machine.
    let sm = KvSm::default();
    let restarted = EzRaft::<Types>::create(&addr, sm.clone(), storage.clone(), config()).await?;
    restarted
        .inner()
        .wait(WAIT)
        .metrics(|m| m.current_leader == Some(0), "restarted node leads")
        .await
        .map_err(io::Error::other)?;
    restarted
        .inner()
        .wait(WAIT)
        .metrics(
            |m| m.last_applied.map(|log_id| log_id.index) == m.last_log_index,
            "log tail replayed",
        )
        .await
        .map_err(io::Error::other)?;

    assert_eq!(expected_map(0..15), sm.data.lock().unwrap().clone());

    // And the restarted node serves reads over the rebuilt state.
    assert_eq!(
        Response {
            value: Some("v14".into())
        },
        restarted.write(get("k14")).await?
    );

    Ok(())
}

/// A node that joins after the leader purged its log can only be brought up by
/// a full snapshot over the network; the join must still end in a voter with
/// the complete state.
#[tokio::test(flavor = "multi_thread")]
async fn lagging_joiner_catches_up_from_snapshot() -> io::Result<()> {
    let addr_a = free_addr();
    let a = EzRaft::<Types>::create(&addr_a, KvSm::default(), MemStorage::default(), config()).await?;
    tokio::spawn({
        let a = a.clone();
        async move { a.serve().await }
    });
    a.inner()
        .wait(WAIT)
        .metrics(|m| m.current_leader == Some(0), "founding node leads")
        .await
        .map_err(io::Error::other)?;

    for i in 0..10 {
        a.write(set(&format!("k{}", i), &format!("v{}", i))).await?;
    }

    // Snapshot, then purge every covered entry: whoever joins now cannot be
    // caught up by log replay.
    a.inner().trigger().snapshot().await.map_err(io::Error::other)?;
    let snapshot_index = a
        .inner()
        .wait(WAIT)
        .metrics(|m| m.snapshot.is_some(), "snapshot built")
        .await
        .map_err(io::Error::other)?
        .snapshot
        .unwrap()
        .index;
    a.inner().trigger().purge_log(snapshot_index).await.map_err(io::Error::other)?;
    a.inner()
        .wait(WAIT)
        .metrics(
            |m| m.purged.map(|log_id| log_id.index) == Some(snapshot_index),
            "log purged up to the snapshot",
        )
        .await
        .map_err(io::Error::other)?;

    let b_sm = KvSm::default();
    let addr_b = free_addr();
    let b = EzRaft::<Types>::join(&addr_b, &addr_a, b_sm.clone(), MemStorage::default(), config()).await?;
    tokio::spawn({
        let b = b.clone();
        async move { b.serve().await }
    });

    let voters = BTreeSet::from([0, b.node_id()]);
    b.inner()
        .wait(WAIT)
        .metrics(
            |m| *m.membership_config.membership().get_joint_config() == [voters.clone()],
            "snapshot-fed joiner promoted to voter",
        )
        .await
        .map_err(io::Error::other)?;

    // The whole pre-purge state must have arrived through the snapshot.
    assert_eq!(expected_map(0..10), b_sm.data.lock().unwrap().clone());

    // And the pair keeps working past the transfer.
    assert_eq!(Response { value: None }, b.write(set("k10", "v10")).await?);
    assert_eq!(
        Response {
            value: Some("v0".into())
        },
        b.write(get("k0")).await?
    );

    Ok(())
}
