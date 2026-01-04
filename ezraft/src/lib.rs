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
//! use ezraft::{EzRaft, EzConfig, EzStorage, EzStateMachine, EzMeta, EzSnapshot, EzStateUpdate, EzTypes};
//! use serde::{Serialize, Deserialize};
//!
//! // 1. Define your request/response types
//! #[derive(Serialize, Deserialize, Debug, Clone)]
//! pub enum Request { Set { key: String, value: String } }
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
//!     async fn restore(&mut self) -> Result<(EzMeta<AppTypes>, Option<EzSnapshot<AppTypes>>), io::Error> {
//!         // Restore meta (or default) and snapshot from disk
//!     }
//!     async fn persist(&mut self, update: EzStateUpdate<AppTypes>) -> Result<(), io::Error> {
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
//! }
//!
//! // 5. Use it
//! let state_machine = AppStateMachine { data: BTreeMap::new() };
//! let storage = AppStorage { base_dir: "./data".into() };
//!
//! let raft = EzRaft::<AppTypes, _, _>::new(1, "127.0.0.1:8080".into(), state_machine, storage, EzConfig::default()).await?;
//! raft.initialize(vec![(1, "127.0.0.1:8080".into())]).await?;
//! raft.serve().await?;
//! ```

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
pub use type_config::EzEntryOf;
pub use type_config::EzLogIdOf;
pub use type_config::EzMembershipOf;
pub use type_config::EzSnapshotDataOf;
pub use type_config::EzTypes;
pub use type_config::EzVote;
pub use type_config::OpenRaftTypes;
pub use types::EzEntry;
pub use types::EzLogId;
pub use types::EzMeta;
pub use types::EzSnapshot;
pub use types::EzSnapshotMeta;
pub use types::EzStateUpdate;
