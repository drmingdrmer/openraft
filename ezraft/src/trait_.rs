//! User-facing traits for EzRaft
//!
//! This module defines the two traits that users must implement:
//! - [`EzApp`]: The application - request/response types, state, and business logic
//! - [`EzStorage`]: Handles persistence of Raft state (meta, logs, snapshots)

use std::io;

use async_trait::async_trait;
use openraft::AppData;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::types::EzEntry;
use crate::types::Loaded;
use crate::types::Persist;

/// The application: request/response types, state, and one method of business logic
///
/// The implementing type IS the application state - a struct holding your data. The framework
/// derives snapshots from it via serde: a snapshot is the serialized state, installing one
/// replaces the state with the deserialized bytes. That makes whole-state serialization the
/// scope of this crate; it serves the coordination/metadata class of app whose state fits in
/// memory (ZooKeeper snapshots the same way). An app whose snapshot is a streamed checkpoint
/// of something larger builds on openraft directly.
///
/// # Example (KV store)
///
/// ```ignore
/// use std::collections::BTreeMap;
///
/// #[derive(Default, Serialize, Deserialize)]
/// struct KvApp { data: BTreeMap<String, String> }
///
/// #[async_trait]
/// impl EzApp for KvApp {
///     type Request = Request;
///     type Response = Response;
///
///     async fn apply(&mut self, req: Request) -> Response {
///         match req {
///             Request::Set { key, value } => {
///                 self.data.insert(key, value);
///                 Response { value: None }
///             }
///             Request::Get { key } => Response {
///                 value: self.data.get(&key).cloned(),
///             },
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait EzApp: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Application request type
    ///
    /// Serde carries it over the wire, `Clone` keeps a copy for forwarding to the leader, and
    /// [`AppData`] asks for `Debug + Display` because openraft prints requests in its logs and
    /// errors. Derive `Display` (e.g. with `derive_more`) or write a short impl - see
    /// `examples/kvstore.rs`.
    type Request: AppData + Serialize + for<'de> Deserialize<'de> + Send + Sync + Clone;

    /// Application response type
    ///
    /// Produced by [`apply`](Self::apply) and carried back over the wire to whichever node
    /// forwarded the write, hence the serde bounds.
    type Response: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static;

    /// Apply a committed request to the state machine
    ///
    /// This is where your business logic goes. The method is called sequentially, in log
    /// order, exactly once per committed entry.
    async fn apply(&mut self, req: Self::Request) -> Self::Response;
}

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
///   deletions requested by [`Persist::DeleteLogs`].
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
/// impl EzStorage<KvApp> for FileStorage {
///     async fn load(&mut self) -> Result<Loaded, io::Error> {
///         // 1. Load meta from base_dir/meta.json (use default if first run)
///         // 2. Optionally load snapshot from base_dir/snapshot.meta + snapshot.data
///         // Log entries are read separately via read_logs()
///         Ok(Loaded { meta, snapshot })
///     }
///
///     async fn persist(&mut self, op: Persist<KvApp>) -> Result<(), io::Error> {
///         match op {
///             Persist::Meta(meta) => { /* write meta */ }
///             Persist::LogEntry(entry) => { /* write log entry */ }
///             Persist::Snapshot(snapshot) => { /* write snapshot.meta and snapshot.snapshot */ }
///             Persist::DeleteLogs { from, to } => { /* delete entries with from <= index < to */ }
///         }
///     }
///
///     async fn read_logs(&mut self, start: u64, end: u64) -> Result<Vec<EzEntry<KvApp>>, io::Error> {
///         // Read log entries in range [start, end)
///     }
/// }
/// ```
#[async_trait]
pub trait EzStorage<T>: Send + Sync + 'static
where T: EzApp
{
    /// Load metadata and snapshot on startup
    ///
    /// Returns the persisted [`Loaded`] state: metadata (or default if first run) and optional
    /// snapshot. Log entries are read separately via [`Self::read_logs`].
    ///
    /// Called exactly once, before the node starts. The framework keeps the snapshot it gets
    /// here, so serving one to a lagging peer does not call back into this method.
    async fn load(&mut self) -> Result<Loaded, io::Error>;

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
