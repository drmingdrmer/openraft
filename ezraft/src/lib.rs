//! EzRaft - A beginner-friendly Raft framework built on openraft
//!
//! EzRaft simplifies distributed consensus by handling all Raft complexity internally.
//! Users only provide:
//! - Business logic via [`EzStateMachine`]
//! - Storage persistence via [`EzStorage`]
//!
//! # Quick Start
//!
//! ```ignore
//! use ezraft::{EzRaft, EzConfig, EzStorage, EzStateMachine, EzEntry, Loaded, Persist, EzTypes};
//! use serde::{Serialize, Deserialize};
//!
//! // 1. Define your request/response types; openraft logs requests, hence the Display
//! #[derive(Serialize, Deserialize, Debug, Clone, derive_more::Display)]
//! pub enum Request {
//!     #[display("Set({key})")]
//!     Set { key: String, value: String },
//! }
//!
//! #[derive(Serialize, Deserialize, Debug, Clone)]
//! pub struct Response { pub value: Option<String> }
//!
//! // 2. Implement EzTypes trait
//! struct AppTypes;
//! impl EzTypes for AppTypes {
//!     type Request = Request;
//!     type Response = Response;
//! }
//!
//! // 3. Implement storage persistence (3 methods)
//! struct AppStorage { base_dir: PathBuf }
//!
//! #[async_trait]
//! impl EzStorage<AppTypes> for AppStorage {
//!     async fn load(&mut self) -> Result<Loaded, io::Error> {
//!         // Load meta (or default) and snapshot from disk
//!         Ok(Loaded { meta, snapshot })
//!     }
//!     async fn persist(&mut self, op: Persist<AppTypes>) -> Result<(), io::Error> {
//!         // Persist state update to disk
//!     }
//!     async fn read_logs(&mut self, start: u64, end: u64) -> Result<Vec<EzEntry<AppTypes>>, io::Error> {
//!         // Read log entries in range [start, end)
//!     }
//! }
//!
//! // 4. Implement state machine (3 methods)
//! struct AppStateMachine { data: BTreeMap<String, String> }
//!
//! #[async_trait]
//! impl EzStateMachine<AppTypes> for AppStateMachine {
//!     async fn apply(&mut self, req: Request) -> Response {
//!         // Apply business logic
//!     }
//!     async fn build_snapshot(&self) -> io::Result<Vec<u8>> {
//!         // Serialize state machine to bytes
//!     }
//!     async fn install_snapshot(&mut self, data: &[u8]) -> io::Result<()> {
//!         // Restore state machine from bytes
//!     }
//! }
//!
//! // 5. Use it
//! let state_machine = AppStateMachine { data: BTreeMap::new() };
//! let storage = AppStorage { base_dir: "./data".into() };
//!
//! // First node (creates cluster)
//! let raft = EzRaft::<AppTypes>::create("127.0.0.1:8080", state_machine, storage, EzConfig::default()).await?;
//! // Or join existing cluster via seed node
//! // let raft = EzRaft::<AppTypes>::join("127.0.0.1:8081", "127.0.0.1:8080", sm, storage, config).await?;
//! raft.serve().await?;
//! ```
//!
//! # Errors
//!
//! Every fallible method returns [`std::io::Error`], including the ones that fail for reasons
//! that have nothing to do with I/O. This is deliberate: a caller cannot usefully branch on the
//! difference. A write that finds no leader, one that cannot reach the leader, and one issued to
//! a stopped node all mean the same thing to an application -- try again later -- because
//! [`EzRaft::write`] already forwards to the leader on its own.
//!
//! [`EzStorage`] is where a user's own errors originate, and those are I/O errors already, so a
//! second error type would buy nothing and cost a concept. Code that does need to tell the cases
//! apart can reach the underlying openraft node and its typed errors through
//! [`EzRaft::inner`](raft::EzRaft::inner).

pub mod config;
pub mod network;
pub mod raft;
pub mod server;
pub mod storage;
pub mod trait_;
pub mod type_config;
pub mod types;

// Re-export public API
pub use config::EzConfig;
pub use openraft::RaftTypeConfig;
pub use raft::EzRaft;
pub use trait_::EzStateMachine;
pub use trait_::EzStorage;
pub use type_config::EzTypes;
pub use type_config::EzVote;
pub use type_config::OpenRaftTypes;
pub use types::EzEntry;
pub use types::EzLogId;
pub use types::EzMeta;
pub use types::EzSnapshot;
pub use types::EzSnapshotMeta;
pub use types::Loaded;
pub use types::Persist;
