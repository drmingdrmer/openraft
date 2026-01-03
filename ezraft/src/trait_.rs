//! User-facing traits for EzRaft
//!
//! This module defines the two traits that users must implement:
//! - [`EzStorage`]: Handles persistence of Raft state (meta, logs, snapshots)
//! - [`EzStateMachine`]: Handles business logic (applying requests to state)

use std::io;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::type_config::EzTypes;
use crate::types::EzEntry;
use crate::types::EzFullState;
use crate::types::EzStateUpdate;

/// Storage persistence trait
///
/// Implement this to handle how Raft state is persisted to disk.
/// The framework handles all Raft logic - you only handle serialization and I/O.
///
/// # Example (file-based storage)
///
/// ```ignore
/// struct FileStorage { base_dir: PathBuf }
///
/// #[async_trait]
/// impl EzStorage<MyAppTypes> for FileStorage {
///     async fn load_state(&mut self) -> Result<Option<EzFullState<MyAppTypes>>, io::Error> {
///         // 1. Load meta from base_dir/meta.json
///         // 2. Load log entries from base_dir/log-{term}-{index}
///         // 3. Optionally load snapshot from base_dir/snapshot.meta + snapshot.data
///         // Return None if this is the first run (no existing state)
///     }
///
///     async fn save_state(&mut self, update: EzStateUpdate<MyAppTypes>) -> Result<(), io::Error> {
///         match update {
///             EzStateUpdate::WriteMeta(meta) => {
///                 // Serialize and write meta to disk
///             }
///             EzStateUpdate::WriteLog { log_id, payload } => {
///                 // Serialize payload and write to log-{term}-{index}
///                 // Payload can be Normal (user request), Blank, or Membership
///             }
///             EzStateUpdate::WriteSnapshot(meta, data) => {
///                 // Write meta to snapshot.meta and data to snapshot.data
///             }
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait EzStorage<T>: Send + Sync + 'static
where
    T: EzTypes,
    T::Request: Serialize + DeserializeOwned,
{
    /// Load complete state on startup
    ///
    /// Return `Ok(None)` if this is the first run (no existing state).
    /// Return `Ok(Some(state))` with all previously persisted state.
    async fn load_state(&mut self) -> Result<Option<EzFullState<T>>, io::Error>;

    /// Persist a state update
    ///
    /// Each call represents one atomic operation that should be durably persisted.
    /// The framework calls this method when state changes.
    async fn save_state(&mut self, update: EzStateUpdate<T>) -> Result<(), io::Error>;

    /// Load log entries within a specific index range
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
    async fn load_log_range(&mut self, start: u64, end: u64) -> Result<Vec<EzEntry<T>>, io::Error>;
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
}
