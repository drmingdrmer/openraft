//! Main EzRaft API
//!
//! This module provides the primary [`EzRaft`] struct that users interact with.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use openraft::async_runtime::WatchReceiver;
use openraft::errors::ClientWriteError;
use openraft::errors::InitializeError;
use openraft::errors::RaftError;
use openraft::BasicNode;
use openraft::ChangeMembers;
use openraft::Raft;
use serde::Serialize;
use tokio::time::sleep;

use crate::config::EzConfig;
use crate::network::EzNetworkFactory;
use crate::storage::StorageAdapter;
use crate::trait_::EzStateMachine;
use crate::trait_::EzStorage;
use crate::type_config::EzTypes;
use crate::type_config::OpenRaftTypes;

/// Type alias for OpenRaft types (more readable than ORTypes<T>)
type ORTypes<T> = OpenRaftTypes<T>;

/// The internal OpenRaft instance, with EzRaft's storage adapter as its state machine
pub type ORRaft<T> = Raft<ORTypes<T>, Arc<StorageAdapter<T>>>;

/// EzRaft - A simplified Raft interface
///
/// This struct wraps OpenRaft's `Raft` and provides a simplified API.
/// Users create an instance with their storage and state machine, then call
/// methods to initialize the cluster, write data, and serve HTTP requests.
///
/// # Type Parameters
///
/// - `T`: Type configuration (implements `EzTypes`)
pub struct EzRaft<T>
where T: EzTypes
{
    /// Node ID
    node_id: u64,

    /// HTTP bind address
    addr: String,

    /// Storage adapter (bridges user storage/state machine to OpenRaft)
    storage: Arc<StorageAdapter<T>>,

    /// Internal OpenRaft instance
    raft: ORRaft<T>,
}

impl<T> Clone for EzRaft<T>
where T: EzTypes
{
    fn clone(&self) -> Self {
        Self {
            node_id: self.node_id,
            addr: self.addr.clone(),
            storage: self.storage.clone(),
            raft: self.raft.clone(),
        }
    }
}

impl<T> EzRaft<T>
where T: EzTypes
{
    /// Start a new cluster with this node as its only member
    ///
    /// Exactly one node of a cluster is created this way; every other node uses [`Self::join`].
    /// Creating two nodes separately gives two one-node clusters that will never merge.
    ///
    /// # Arguments
    ///
    /// * `http_addr` - Address to bind HTTP server (e.g., "127.0.0.1:8080")
    /// * `state_machine` - User's state machine implementation
    /// * `storage` - User's storage implementation
    /// * `config` - EzRaft configuration (use `EzConfig::default()` for sensible defaults)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let raft = EzRaft::create("127.0.0.1:8080", sm, storage, config).await?;
    /// ```
    pub async fn create(
        http_addr: impl ToString,
        state_machine: impl EzStateMachine<T> + 'static,
        storage: impl EzStorage<T> + 'static,
        config: EzConfig,
    ) -> Result<Self, io::Error> {
        Self::new(http_addr, state_machine, storage, config, None).await
    }

    /// Join the cluster that `seed_addr` belongs to
    ///
    /// The seed assigns this node an id and adds it to the cluster; the seed does not have to be
    /// the leader. On restart the persisted id is reused and the seed is not contacted again, so
    /// passing an address that has since left the cluster is harmless.
    ///
    /// # Arguments
    ///
    /// * `http_addr` - Address to bind HTTP server (e.g., "127.0.0.1:8081")
    /// * `seed_addr` - Address of any node already in the cluster
    /// * `state_machine` - User's state machine implementation
    /// * `storage` - User's storage implementation
    /// * `config` - EzRaft configuration (use `EzConfig::default()` for sensible defaults)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let raft = EzRaft::join("127.0.0.1:8081", "127.0.0.1:8080", sm, storage, config).await?;
    /// ```
    pub async fn join(
        http_addr: impl ToString,
        seed_addr: impl ToString,
        state_machine: impl EzStateMachine<T> + 'static,
        storage: impl EzStorage<T> + 'static,
        config: EzConfig,
    ) -> Result<Self, io::Error> {
        Self::new(http_addr, state_machine, storage, config, Some(seed_addr.to_string())).await
    }

    async fn new(
        http_addr: impl ToString,
        state_machine: impl EzStateMachine<T> + 'static,
        storage: impl EzStorage<T> + 'static,
        config: EzConfig,
        seed_addr: Option<String>,
    ) -> Result<Self, io::Error> {
        let http_addr = http_addr.to_string();

        // Create storage adapter that bridges user traits to OpenRaft
        let adapter = StorageAdapter::new(storage, state_machine).await?;
        let adapter = Arc::new(adapter);

        // Determine node_id
        let node_id = if let Some(id) = adapter.node_id().await {
            // Use persisted node_id (restart case)
            id
        } else if let Some(seed) = &seed_addr {
            // Join existing cluster via seed node
            let id = request_join(seed, &http_addr).await?;
            adapter.save_meta(|m| m.node_id = Some(id)).await?;
            id
        } else {
            // First node in cluster
            let id = 0;
            adapter.save_meta(|m| m.node_id = Some(id)).await?;
            id
        };

        let (log_store, sm_store) = (adapter.clone(), adapter.clone());

        // Convert EzConfig to OpenRaft Config
        let raft_config = config.to_raft_config()?;
        let raft_config = Arc::new(raft_config);

        // Create network factory
        let network = EzNetworkFactory::new()?;

        // Create OpenRaft instance
        let raft = Raft::new(node_id, raft_config, network, log_store, sm_store)
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;

        // The created node starts the cluster with itself as its only member. On restart it loads
        // id 0 from storage and comes back here, where initializing is rightly refused; every
        // other refusal means the node cannot run.
        if node_id == 0 {
            let nodes = BTreeMap::from_iter([(node_id, BasicNode::new(http_addr.clone()))]);
            match raft.initialize(nodes).await {
                Ok(()) | Err(RaftError::APIError(InitializeError::NotAllowed(_))) => {}
                Err(e) => return Err(io::Error::other(e.to_string())),
            }
        }

        Ok(Self {
            node_id,
            addr: http_addr,
            storage: adapter,
            raft,
        })
    }

    /// Write a request to the Raft log
    ///
    /// This proposes a client request to the Raft cluster.
    /// The request will be replicated and applied to the state machine once committed.
    ///
    /// Only a leader can accept a write. Calling this on a follower forwards the request to the
    /// leader over HTTP and returns the leader's answer, so a caller never has to track which
    /// node is currently in charge.
    ///
    /// # Arguments
    ///
    /// * `req` - User's request type
    ///
    /// # Returns
    ///
    /// The response from the state machine's `apply()` method
    ///
    /// # Example
    ///
    /// ```ignore
    /// let req = Request::Set { key: "foo".into(), value: "bar".into() };
    /// let resp = raft.write(req).await?;
    /// ```
    pub async fn write(&self, req: T::Request) -> Result<T::Response, io::Error> {
        let err = match self.raft.client_write(req.clone()).await {
            Ok(resp) => return Ok(resp.data),
            Err(e) => e,
        };

        let RaftError::APIError(ClientWriteError::ForwardToLeader(forward)) = &err else {
            return Err(io::Error::other(err.to_string()));
        };

        // Forwarding to ourselves would repeat this call over HTTP forever.
        match forward.leader_node.as_ref().map(|n| n.addr.as_str()) {
            Some(leader) if leader != self.addr => forward_write::<T>(leader, &req).await,
            _ => Err(io::Error::other(err.to_string())),
        }
    }

    /// Add a learner node to the cluster
    ///
    /// Learners receive log replication but don't participate in voting.
    /// This is useful for adding read-only nodes or preparing a node for membership.
    ///
    /// Returns as soon as replication to the new node is set up; the node catches up in the
    /// background. Waiting here would deadlock the join handler, whose caller cannot answer any
    /// Raft RPC until it gets its node id back.
    ///
    /// # Arguments
    ///
    /// * `node_id` - ID of the new learner node
    /// * `addr` - Address of the new learner node
    pub async fn add_learner(&self, node_id: u64, addr: String) -> Result<(), io::Error> {
        let node = BasicNode::new(addr);
        self.raft.add_learner(node_id, node, false).await.map_err(|e| io::Error::other(e.to_string()))?;

        Ok(())
    }

    /// Wait for a learner to catch up, then make it a voter
    ///
    /// Only voters count towards a quorum, so a cluster tolerates a node failure only once its
    /// nodes have been promoted. Promoting a node that is still far behind would stall the
    /// membership change, hence the wait.
    ///
    /// Returns without changing anything if this node is no longer the leader; the new leader
    /// owns the promotion from that point on.
    pub async fn promote_to_voter(&self, node_id: u64) -> Result<(), io::Error> {
        let caught_up = |m: &openraft::RaftMetrics<ORTypes<T>>| {
            let Some(replication) = m.replication.as_ref() else {
                // Not the leader anymore, stop waiting.
                return true;
            };
            let matched = replication.get(&node_id).and_then(|log_id| log_id.as_ref()).map(|log_id| log_id.index);
            matched >= m.last_log_index
        };

        let metrics = self
            .raft
            .wait(None)
            .metrics(caught_up, "learner catches up before promotion")
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;

        if metrics.current_leader != Some(self.node_id) {
            return Ok(());
        }

        self.change_membership(ChangeMembers::AddVoterIds(BTreeSet::from([node_id]))).await
    }

    /// Change the cluster membership
    ///
    /// This modifies the cluster membership using OpenRaft's `ChangeMembers`.
    pub async fn change_membership(&self, change: ChangeMembers<u64, BasicNode>) -> Result<(), io::Error> {
        self.raft.change_membership(change, false).await.map_err(|e| io::Error::other(e.to_string()))?;
        Ok(())
    }

    /// Check if this node is the leader
    ///
    /// Returns `true` if this node is the current cluster leader.
    pub async fn is_leader(&self) -> bool {
        use openraft::raft::ReadPolicy;
        self.raft.ensure_linearizable(ReadPolicy::LeaseRead).await.is_ok()
    }

    /// Get the current cluster metrics
    ///
    /// Returns information about the Raft cluster state.
    pub async fn metrics(&self) -> openraft::RaftMetrics<ORTypes<T>> {
        self.raft.metrics().borrow_watched().clone()
    }

    /// Start the HTTP server
    ///
    /// This starts the HTTP server that handles:
    /// - Internal Raft RPC (append entries, vote, install snapshot)
    /// - Admin API (join, add learner, change membership, metrics)
    ///
    /// This method blocks until the server is stopped.
    pub async fn serve(&self) -> Result<(), io::Error> {
        crate::server::run(self.clone()).await
    }

    /// Get the node ID
    pub fn node_id(&self) -> u64 {
        self.node_id
    }

    /// Get the HTTP address
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Get a reference to the internal OpenRaft instance
    ///
    /// This provides access to advanced OpenRaft APIs if needed.
    pub fn inner(&self) -> &ORRaft<T> {
        &self.raft
    }

    /// Get a reference to the storage adapter
    ///
    /// This provides access to the underlying storage and state machine.
    /// Use `storage.storage` and `storage.sm_state` to access them.
    pub fn storage(&self) -> &Arc<StorageAdapter<T>> {
        &self.storage
    }
}

/// How long a forwarded write may take before the leader is given up on
///
/// Generous, because the leader has to replicate and commit the request before answering.
const FORWARD_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Send a write to the leader's `/api/write` endpoint and return what it applied
///
/// The leader is asked to do the write on this node's behalf, so the answer is the same one the
/// caller would have got from writing to the leader directly.
async fn forward_write<T>(leader_addr: &str, req: &T::Request) -> Result<T::Response, io::Error>
where T: EzTypes {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(FORWARD_WRITE_TIMEOUT)
        .build()
        .map_err(|e| io::Error::other(e.to_string()))?;

    let url = format!("http://{}/api/write", leader_addr);

    let resp = client
        .post(&url)
        .json(req)
        .send()
        .await
        .map_err(|e| io::Error::other(format!("forwarding write to {} failed: {}", url, e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(io::Error::other(format!("{} responded {}: {}", url, status, body)));
    }

    resp.json().await.map_err(|e| io::Error::other(format!("failed to parse write response: {}", e)))
}

/// Request to join a cluster
#[derive(Debug, Serialize)]
struct JoinRequest {
    addr: String,
}

/// Join response: Ok(node_id) or Err(leader_addr)
type JoinResponse = Result<u64, Option<String>>;

/// How many times a join is attempted before the node gives up and reports the last failure
const JOIN_ATTEMPTS: usize = 20;

/// How long to wait before attempting a join again
const JOIN_RETRY_INTERVAL: Duration = Duration::from_millis(500);

/// How long a single join request may take before the target is given up on
const JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Request to join a cluster via seed node
///
/// Follows the seed's redirect if it is not the leader, and retries the transient conditions a
/// starting cluster is full of: no leader elected yet, or another node's membership change still
/// in flight. A cluster admits one member at a time, so nodes started together take turns here
/// instead of failing.
async fn request_join(seed_addr: &str, my_addr: &str) -> Result<u64, io::Error> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(JOIN_TIMEOUT)
        .build()
        .map_err(|e| io::Error::other(e.to_string()))?;

    let mut target_addr = seed_addr.to_string();
    let mut last_err = "cluster did not accept the join".to_string();

    for _ in 0..JOIN_ATTEMPTS {
        let url = format!("http://{}/api/join", target_addr);
        let req = JoinRequest {
            addr: my_addr.to_string(),
        };

        let resp = client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| io::Error::other(format!("join request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            last_err = format!("{} responded {}: {}", url, status, body);
            sleep(JOIN_RETRY_INTERVAL).await;
            continue;
        }

        let join_resp: JoinResponse =
            resp.json().await.map_err(|e| io::Error::other(format!("failed to parse join response: {}", e)))?;

        match join_resp {
            Ok(node_id) => return Ok(node_id),
            Err(Some(leader)) => {
                last_err = format!("{} redirected to {}", url, leader);
                target_addr = leader;
            }
            Err(None) => {
                last_err = format!("{} knows of no leader", url);
                sleep(JOIN_RETRY_INTERVAL).await;
            }
        }
    }

    Err(io::Error::other(format!(
        "join gave up after {} attempts: {}",
        JOIN_ATTEMPTS, last_err
    )))
}
