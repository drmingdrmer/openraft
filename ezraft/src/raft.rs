//! Main EzRaft API
//!
//! This module provides the primary [`EzRaft`] struct that users interact with.

use std::io;
use std::sync::Arc;

use openraft::async_runtime::WatchReceiver;
use openraft::BasicNode;
use openraft::Raft;
use serde::Serialize;

use crate::config::EzConfig;
use crate::network::EzNetworkFactory;
use crate::storage::StorageAdapter;
use crate::trait_::EzStateMachine;
use crate::trait_::EzStorage;
use crate::type_config::EzTypes;
use crate::type_config::OpenRaftTypes;

/// Type alias for OpenRaft types (more readable than ORTypes<T>)
type ORTypes<T> = OpenRaftTypes<T>;

/// EzRaft - A simplified Raft interface
///
/// This struct wraps OpenRaft's `Raft` and provides a simplified API.
/// Users create an instance with their storage and state machine, then call
/// methods to initialize the cluster, write data, and serve HTTP requests.
///
/// # Type Parameters
///
/// - `T`: Type configuration (implements `EzTypes`)
/// - `S`: User's storage implementation (implements `EzStorage<T>`)
/// - `M`: User's state machine implementation (implements `EzStateMachine<T>`)
pub struct EzRaft<T, S, M>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    /// Node ID
    node_id: u64,

    /// HTTP bind address
    addr: String,

    /// Storage adapter (bridges user storage/state machine to OpenRaft)
    storage: Arc<StorageAdapter<T, S, M>>,

    /// Internal OpenRaft instance
    raft: Raft<ORTypes<T>>,
}

impl<T, S, M> Clone for EzRaft<T, S, M>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
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

impl<T, S, M> EzRaft<T, S, M>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    /// Create a new EzRaft instance
    ///
    /// This initializes the internal Raft instance with the user's storage and state machine.
    /// The node automatically joins a cluster or initializes as the first node.
    ///
    /// # Arguments
    ///
    /// * `http_addr` - Address to bind HTTP server (e.g., "127.0.0.1:8080")
    /// * `state_machine` - User's state machine implementation
    /// * `storage` - User's storage implementation
    /// * `config` - EzRaft configuration (use `EzConfig::default()` for sensible defaults)
    /// * `seed_addr` - Optional seed node address to join existing cluster
    ///
    /// # Behavior
    ///
    /// - If `seed_addr` is `None`: Initialize as single-node cluster with node_id = 0
    /// - If `seed_addr` is `Some`: Join existing cluster via seed node
    ///
    /// # Example
    ///
    /// ```ignore
    /// // First node (creates new cluster)
    /// let raft = EzRaft::new("127.0.0.1:8080", sm, storage, config, None).await?;
    ///
    /// // Joining node (joins existing cluster)
    /// let raft = EzRaft::new("127.0.0.1:8081", sm, storage, config, Some("127.0.0.1:8080".into())).await?;
    /// ```
    pub async fn new(
        http_addr: impl ToString,
        state_machine: M,
        storage: S,
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
        let raft_config = config.to_raft_config().map_err(|e| io::Error::other(e.to_string()))?;
        let raft_config = Arc::new(raft_config);

        // Create network factory
        let network = EzNetworkFactory::new();

        // Create OpenRaft instance
        let raft = Raft::new(node_id, raft_config, network, log_store, sm_store)
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;

        // Auto-initialize if first node (node_id == 0 means truly first node)
        // On restart, node_id is loaded from storage, so this only runs on first run of first node
        if node_id == 0 {
            use std::collections::BTreeMap;
            let nodes = BTreeMap::from_iter([(node_id, BasicNode::new(http_addr.clone()))]);
            // Ignore error if already initialized (restart case)
            raft.initialize(nodes).await.ok();
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
        let resp = self.raft.client_write(req).await.map_err(|e| io::Error::other(e.to_string()))?;

        Ok(resp.data)
    }

    /// Add a learner node to the cluster
    ///
    /// Learners receive log replication but don't participate in voting.
    /// This is useful for adding read-only nodes or preparing a node for membership.
    ///
    /// # Arguments
    ///
    /// * `node_id` - ID of the new learner node
    /// * `addr` - Address of the new learner node
    pub async fn add_learner(&self, node_id: u64, addr: String) -> Result<(), io::Error> {
        let node = BasicNode::new(addr);
        self.raft.add_learner(node_id, node, true).await.map_err(|e| io::Error::other(e.to_string()))?;

        Ok(())
    }

    /// Change the cluster membership
    ///
    /// This modifies the cluster membership using OpenRaft's `ChangeMembers`.
    pub async fn change_membership(
        &self,
        change: openraft::ChangeMembers<ORTypes<T>>,
    ) -> Result<(), io::Error> {
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
    pub fn inner(&self) -> &Raft<ORTypes<T>> {
        &self.raft
    }

    /// Get a reference to the storage adapter
    ///
    /// This provides access to the underlying storage and state machine.
    /// Use `storage.storage_state` and `storage.sm_state` to access them.
    pub fn storage(&self) -> &Arc<StorageAdapter<T, S, M>> {
        &self.storage
    }
}

/// Request to join a cluster
#[derive(Debug, Serialize)]
struct JoinRequest {
    addr: String,
}

/// Join response: Ok(node_id) or Err(leader_addr)
type JoinResponse = Result<u64, Option<String>>;

/// Request to join a cluster via seed node
///
/// Retries with leader if seed is not the leader.
async fn request_join(seed_addr: &str, my_addr: &str) -> Result<u64, io::Error> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| io::Error::other(e.to_string()))?;

    let mut target_addr = seed_addr.to_string();

    loop {
        let url = format!("http://{}/api/join", target_addr);
        let req = JoinRequest { addr: my_addr.to_string() };

        let resp = client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| io::Error::other(format!("join request failed: {}", e)))?;

        if !resp.status().is_success() {
            return Err(io::Error::other(format!(
                "join request failed with status: {}",
                resp.status()
            )));
        }

        let join_resp: JoinResponse = resp
            .json()
            .await
            .map_err(|e| io::Error::other(format!("failed to parse join response: {}", e)))?;

        match join_resp {
            Ok(node_id) => return Ok(node_id),
            Err(Some(leader)) => target_addr = leader,
            Err(None) => return Err(io::Error::other("no leader available")),
        }
    }
}
