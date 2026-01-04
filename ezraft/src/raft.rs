//! Main EzRaft API
//!
//! This module provides the primary [`EzRaft`] struct that users interact with.

use std::io;
use std::sync::Arc;

use openraft::async_runtime::WatchReceiver;
use openraft::BasicNode;
use openraft::Raft;
use tokio::sync::Mutex;

use crate::config::EzConfig;
use crate::network::EzNetworkFactory;
use crate::storage::StateMachineState;
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

    /// User's storage state (storage + metadata, protected by single mutex)
    storage_state: Arc<Mutex<crate::storage::StorageState<S, T>>>,

    /// State machine state (user's state machine + Raft metadata)
    sm_state: Arc<Mutex<StateMachineState<T, M>>>,

    /// Internal OpenRaft instance
    raft: Raft<ORTypes<T>>,
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
    ///
    /// # Arguments
    ///
    /// * `node_id` - Unique identifier for this node
    /// * `http_addr` - Address to bind HTTP server (e.g., "127.0.0.1:8080")
    /// * `user_state` - User's state machine implementation
    /// * `user_storage` - User's storage implementation
    /// * `config` - EzRaft configuration (use `EzConfig::default()` for sensible defaults)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let raft = MyEzRaft::new(
    ///     1,
    ///     "127.0.0.1:8080".into(),
    ///     my_state_machine,
    ///     my_storage,
    ///     EzConfig::default(),
    /// ).await?;
    /// ```
    pub async fn new(
        node_id: u64,
        http_addr: String,
        user_state: M,
        user_storage: S,
        config: EzConfig,
    ) -> Result<Self, io::Error> {
        // Create storage adapter that bridges user traits to OpenRaft
        let adapter = StorageAdapter::new(user_storage, user_state).await?;

        // Keep references to user storage/state before splitting
        let storage = adapter.storage_state().clone();
        let sm_state = adapter.sm_state().clone();

        let (log_store, sm_store) = adapter.split();

        // Convert EzConfig to OpenRaft Config
        let raft_config = config.to_raft_config().map_err(|e| io::Error::other(e.to_string()))?;
        let raft_config = Arc::new(raft_config);

        // Create network factory
        let network = EzNetworkFactory::new();

        // Create OpenRaft instance
        let raft = Raft::new(node_id, raft_config, network, log_store, sm_store)
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;

        Ok(Self {
            node_id,
            addr: http_addr,
            storage_state: storage,
            sm_state,
            raft,
        })
    }

    /// Initialize the Raft cluster
    ///
    /// This should be called once when setting up a new cluster.
    /// It registers the initial members of the cluster.
    ///
    /// # Arguments
    ///
    /// * `members` - List of (node_id, address) tuples for all cluster members
    ///
    /// # Example
    ///
    /// ```ignore
    /// raft.initialize(vec![
    ///     (1, "127.0.0.1:8080".into()),
    ///     (2, "127.0.0.1:8081".into()),
    ///     (3, "127.0.0.1:8082".into()),
    /// ]).await?;
    /// ```
    pub async fn initialize(&self, members: Vec<(u64, String)>) -> Result<(), io::Error> {
        use std::collections::BTreeMap;

        use openraft::BasicNode;

        let nodes: BTreeMap<u64, BasicNode> =
            members.into_iter().map(|(id, addr)| (id, BasicNode::new(addr))).collect();

        self.raft.initialize(nodes).await.map_err(|e| io::Error::other(e.to_string()))?;

        Ok(())
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
    /// This replaces the current membership configuration with a new one.
    /// Use this to add or remove voting members from the cluster.
    ///
    /// # Arguments
    ///
    /// * `members` - List of (node_id, address) tuples for the new membership
    pub async fn change_membership(&self, members: Vec<(u64, String)>) -> Result<(), io::Error> {
        use std::collections::BTreeMap;

        use openraft::ChangeMembers;

        let nodes: BTreeMap<u64, BasicNode> =
            members.into_iter().map(|(id, addr)| (id, BasicNode::new(addr))).collect();

        let changes = ChangeMembers::AddVoters(nodes);

        self.raft.change_membership(changes, false).await.map_err(|e| io::Error::other(e.to_string()))?;

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
    /// - Admin API (initialize, add learner, change membership, metrics)
    /// - Application API (write, read)
    ///
    /// This method blocks until the server is stopped.
    pub async fn serve(&self) -> Result<(), io::Error> {
        crate::server::run(self).await
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

    /// Get a reference to the state machine state
    ///
    /// This provides access to the user's state machine and Raft metadata (last_applied,
    /// membership). Use `.lock().await.user_sm` to access the user's state machine directly.
    pub fn sm_state(&self) -> &Arc<Mutex<StateMachineState<T, M>>> {
        &self.sm_state
    }

    /// Get a reference to the storage state (storage + cached metadata)
    ///
    /// This provides access to the underlying storage implementation and metadata.
    pub fn storage_state(&self) -> &Arc<Mutex<crate::storage::StorageState<S, T>>> {
        &self.storage_state
    }
}
