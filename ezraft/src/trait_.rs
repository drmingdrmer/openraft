//! User-facing traits for EzRaft
//!
//! This module defines the two traits that users must implement:
//! - [`EzStorage`]: Handles persistence of Raft state (meta, logs, snapshots)
//! - [`EzStateMachine`]: Handles business logic (applying requests to state)

use std::io;

use async_trait::async_trait;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::type_config::EzTypes;
use crate::types::EzEntry;
use crate::types::EzMeta;
use crate::types::EzSnapshot;
use crate::types::Persist;

/// Storage persistence trait
///
/// Implement this to handle how Raft state is persisted to disk.
/// The framework handles all Raft logic - you only handle serialization and I/O.
///
/// # What the framework assumes
///
/// Raft's safety guarantees rest on these, so an implementation that breaks one can lose
/// acknowledged writes or elect two leaders for the same term:
///
/// - **Durability.** [`persist`] must return only once the data would survive a crash of the
///   machine, not just of the process. Anything weaker means a node can forget a vote it cast or a
///   log entry it acknowledged. The bundled example writes files without `fsync`, which is fine for
///   a demo and not enough for a real deployment.
/// - **Ordering.** Operations are applied in the order [`persist`] receives them. A later one must
///   not become durable before an earlier one.
/// - **Read-your-writes.** [`read_logs`] returns what [`persist`] last wrote, including the
///   deletions requested by [`Persist::TruncateLogs`] and [`Persist::PurgeLogs`].
///
/// [`persist`]: Self::persist
/// [`read_logs`]: Self::read_logs
///
/// # Example (file-based storage)
///
/// ```ignore
/// struct FileStorage { base_dir: PathBuf }
///
/// #[async_trait]
/// impl EzStorage<AppTypes> for FileStorage {
///     async fn load(&mut self) -> Result<(EzMeta, Option<EzSnapshot>), io::Error> {
///         // 1. Load meta from base_dir/meta.json (use default if first run)
///         // 2. Optionally load snapshot from base_dir/snapshot.meta + snapshot.data
///         // Log entries are read separately via read_logs()
///     }
///
///     async fn persist(&mut self, op: Persist<AppTypes>) -> Result<(), io::Error> {
///         match op {
///             Persist::Meta(meta) => { /* write meta */ }
///             Persist::LogEntry(entry) => { /* write log entry */ }
///             Persist::Snapshot(snapshot) => { /* write snapshot.meta and snapshot.snapshot */ }
///             Persist::TruncateLogs(from) => { /* delete entries at index >= from */ }
///             Persist::PurgeLogs(upto) => { /* delete entries at index <= upto */ }
///         }
///     }
///
///     async fn read_logs(&mut self, start: u64, end: u64) -> Result<Vec<EzEntry<AppTypes>>, io::Error> {
///         // Read log entries in range [start, end)
///     }
/// }
/// ```
#[async_trait]
pub trait EzStorage<T>: Send + Sync + 'static
where
    T: EzTypes,
    T::Request: Serialize + DeserializeOwned,
{
    /// Load metadata and snapshot on startup
    ///
    /// Returns persisted metadata (or default if first run) and optional snapshot.
    /// Log entries are read separately via [`Self::read_logs`].
    ///
    /// Called exactly once, before the node starts. The framework keeps the snapshot it gets
    /// here, so serving one to a lagging peer does not call back into this method.
    async fn load(&mut self) -> Result<(EzMeta, Option<EzSnapshot>), io::Error>;

    /// Persist a state update
    ///
    /// Each call represents one atomic operation that should be durably persisted.
    /// The framework calls this method when state changes. See [`Persist`] for what each
    /// operation requires.
    async fn persist(&mut self, op: Persist<T>) -> Result<(), io::Error>;

    /// Read log entries within a specific index range
    ///
    /// Returns log entries where `start <= entry.index < end`.
    /// Called during replication to read specific entries without loading all logs.
    ///
    /// # Arguments
    /// * `start` - Start index (inclusive)
    /// * `end` - End index (exclusive)
    ///
    /// # Returns
    /// Log entries in the range, sorted by index. Empty vec if range is empty or
    /// no entries exist in range.
    async fn read_logs(&mut self, start: u64, end: u64) -> Result<Vec<EzEntry<T>>, io::Error>;
}

/// State machine trait for business logic
///
/// Implement this to define how your application processes requests.
/// The state machine is kept in memory by the framework - you only implement the logic.
///
/// # Example (KV store)
///
/// ```ignore
/// use std::collections::BTreeMap;
///
/// struct MyStore { data: BTreeMap<String, String> }
///
/// #[async_trait]
/// impl EzStateMachine<MyAppTypes> for MyStore {
///     async fn apply(&mut self, req: <MyAppTypes as EzTypes>::Request) -> <MyAppTypes as EzTypes>::Response {
///         match req {
///             Request::Set { key, value } => {
///                 self.data.insert(key, value);
///                 Response { value: None }
///             }
///             Request::Get { key } => {
///                 let value = self.data.get(&key).cloned();
///                 Response { value }
///             }
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait EzStateMachine<T>: Send + Sync + 'static
where T: EzTypes
{
    /// Apply a request to the state machine
    ///
    /// This is where your business logic goes.
    /// The method is called sequentially for committed log entries.
    async fn apply(&mut self, req: T::Request) -> T::Response;

    /// Build a snapshot of the current state machine state
    ///
    /// Serialize your state machine to bytes for persistence.
    /// This is called periodically to create checkpoints.
    async fn build_snapshot(&self) -> io::Result<Vec<u8>>;

    /// Install a snapshot to replace the current state machine state
    ///
    /// Deserialize and replace your state machine from snapshot bytes.
    /// This is called when receiving a snapshot from the leader.
    async fn install_snapshot(&mut self, data: &[u8]) -> io::Result<()>;
}
