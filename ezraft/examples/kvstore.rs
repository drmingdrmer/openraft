//! EzRaft KV Store Example
//!
//! A simple distributed key-value store built on EzRaft.
//! Run multiple instances to form a cluster:
//!
//! ```bash
//! # Terminal 1
//! cargo run --example kvstore -- --node-id 1 --addr 127.0.0.1:8080
//!
//! # Terminal 2
//! cargo run --example kvstore -- --node-id 2 --addr 127.0.0.1:8081
//!
//! # Terminal 3
//! cargo run --example kvstore -- --node-id 3 --addr 127.0.0.1:8082
//! ```

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use clap::Parser;
use ezraft::EzConfig;
use ezraft::EzEntry;
use ezraft::EzFullState;
use ezraft::EzMeta;
use ezraft::EzRaft;
use ezraft::EzSnapshotMeta;
use ezraft::EzStateMachine;
use ezraft::EzStateUpdate;
use ezraft::EzStorage;
use ezraft::EzTypes;
use serde::Deserialize;
use serde::Serialize;
use tokio::fs;

// Define application request types
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Request {
    Set { key: String, value: String },
    Get { key: String },
    Delete { key: String },
}

impl std::fmt::Display for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Request::Set { key, .. } => write!(f, "Set({})", key),
            Request::Get { key } => write!(f, "Get({})", key),
            Request::Delete { key } => write!(f, "Delete({})", key),
        }
    }
}

// Define application response type
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response {
    pub value: Option<String>,
}

// Define type configuration
#[derive(serde::Serialize, serde::Deserialize)]
struct KvStoreTypes;
impl EzTypes for KvStoreTypes {
    type Request = Request;
    type Response = Response;
}

// In-memory state machine
struct KvStore {
    data: BTreeMap<String, String>,
}

impl KvStore {
    fn new() -> Self {
        Self { data: BTreeMap::new() }
    }
}

#[async_trait::async_trait]
impl EzStateMachine<KvStoreTypes> for KvStore {
    async fn apply(&mut self, req: Request) -> Response {
        match req {
            Request::Set { key, value } => {
                self.data.insert(key.clone(), value);
                Response { value: None }
            }
            Request::Get { key } => {
                let value = self.data.get(&key).cloned();
                Response { value }
            }
            Request::Delete { key } => {
                let value = self.data.remove(&key);
                Response { value }
            }
        }
    }
}

// File-based storage implementation
struct FileStorage {
    base_dir: PathBuf,
}

impl FileStorage {
    fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn meta_path(&self) -> PathBuf {
        self.base_dir.join("meta.json")
    }

    fn logs_dir(&self) -> PathBuf {
        self.base_dir.join("logs")
    }

    fn log_path(&self, index: u64) -> PathBuf {
        self.logs_dir().join(format!("log-{}", index))
    }

    fn snapshot_meta_path(&self) -> PathBuf {
        self.base_dir.join("snapshot.meta")
    }

    fn snapshot_data_path(&self) -> PathBuf {
        self.base_dir.join("snapshot.data")
    }
}

#[async_trait::async_trait]
impl EzStorage<KvStoreTypes> for FileStorage {
    async fn load_state(&mut self) -> io::Result<Option<EzFullState<KvStoreTypes>>> {
        // Load meta
        let meta_data = match fs::read(&self.meta_path()).await {
            Ok(data) => data,
            Err(_) => return Ok(None), // First run
        };
        let meta: EzMeta<KvStoreTypes> = serde_json::from_slice(&meta_data)?;

        // Load all log entries
        let mut logs = vec![];
        let logs_dir = self.logs_dir();
        let mut entries = match fs::read_dir(&logs_dir).await {
            Ok(e) => e,
            Err(_) => {
                return Ok(Some(EzFullState {
                    meta,
                    logs,
                    snapshot: None,
                }))
            }
        };

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("log-") {
                let index: u64 = name[4..].parse().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                let data = fs::read(entry.path()).await?;

                // Deserialize struct containing both term and payload
                #[derive(serde::Deserialize)]
                struct StoredEntry {
                    term: u64,
                    payload: openraft::EntryPayload<ezraft::OpenRaftTypes<KvStoreTypes>>,
                }
                let stored: StoredEntry = serde_json::from_slice(&data)?;

                // Create EzEntry
                logs.push(ezraft::EzEntry {
                    log_id: (stored.term, index),
                    payload: stored.payload,
                });
            }
        }
        logs.sort_by_key(|entry| entry.log_id);

        // Load snapshot (optional)
        let snapshot = match fs::read(&self.snapshot_meta_path()).await {
            Ok(meta_data) => {
                let meta: EzSnapshotMeta<KvStoreTypes> = serde_json::from_slice(&meta_data)?;
                let data = fs::read(&self.snapshot_data_path()).await?;
                Some((meta, data))
            }
            Err(_) => None,
        };

        Ok(Some(EzFullState { meta, logs, snapshot }))
    }

    async fn save_state(&mut self, update: EzStateUpdate<KvStoreTypes>) -> io::Result<()> {
        match update {
            EzStateUpdate::WriteMeta(meta) => {
                fs::write(&self.meta_path(), serde_json::to_vec_pretty(&meta)?).await?;
            }
            EzStateUpdate::WriteLog(entry) => {
                // Ensure logs directory exists
                fs::create_dir_all(&self.logs_dir()).await?;

                // Extract log_id and payload from EzEntry
                let (term, index) = entry.log_id;

                // Serialize struct containing both term and payload
                #[derive(serde::Serialize)]
                struct StoredEntry<'a> {
                    term: u64,
                    payload: &'a openraft::EntryPayload<ezraft::OpenRaftTypes<KvStoreTypes>>,
                }
                let stored = StoredEntry {
                    term,
                    payload: &entry.payload,
                };
                let data = serde_json::to_vec(&stored)?;
                let path = self.log_path(index);
                fs::write(path, data).await?;
            }
            EzStateUpdate::WriteSnapshot(meta, data) => {
                fs::write(&self.snapshot_meta_path(), serde_json::to_vec(&meta)?).await?;
                fs::write(&self.snapshot_data_path(), data).await?;
            }
        }
        Ok(())
    }

    async fn load_log_range(&mut self, start: u64, end: u64) -> io::Result<Vec<EzEntry<KvStoreTypes>>> {
        let mut logs = Vec::new();

        for index in start..end {
            let path = self.log_path(index);
            let data = match fs::read(&path).await {
                Ok(d) => d,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };

            #[derive(serde::Deserialize)]
            struct StoredEntry {
                term: u64,
                payload: openraft::EntryPayload<ezraft::OpenRaftTypes<KvStoreTypes>>,
            }
            let stored: StoredEntry = serde_json::from_slice(&data)?;

            logs.push(EzEntry {
                log_id: (stored.term, index),
                payload: stored.payload,
            });
        }

        Ok(logs)
    }
}

/// Command-line arguments for the KV store
#[derive(clap::Parser)]
struct Args {
    /// Node ID (unique identifier for this Raft node)
    #[arg(long, default_value_t = 1)]
    node_id: u64,

    /// HTTP bind address (e.g., "127.0.0.1:8080")
    #[arg(long, default_value = "127.0.0.1:8080")]
    addr: String,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    let node_id = args.node_id;
    let addr = args.addr;

    // Create data directory for this node
    let base_dir = PathBuf::from(format!("./data/node-{}", node_id));
    fs::create_dir_all(&base_dir).await?;

    // Create state machine and storage
    let store = KvStore::new();
    let storage = FileStorage::new(base_dir);

    // Create EzRaft instance
    let raft = EzRaft::<KvStoreTypes, _, _>::new(node_id, addr.clone(), store, storage, EzConfig::default()).await?;

    // Initialize if first node
    if node_id == 1 {
        raft.initialize(vec![(1, addr.clone())]).await?;
        println!("Node 1 initialized as single-node cluster");
    }

    println!("Node {} listening on {}", node_id, addr);

    // Start HTTP server
    raft.serve().await?;

    Ok(())
}
