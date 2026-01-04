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
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::PathBuf;

use clap::Parser;
use ezraft::EzConfig;
use ezraft::EzEntry;
use ezraft::EzMeta;
use ezraft::EzRaft;
use ezraft::EzSnapshot;
use ezraft::EzSnapshotMeta;
use ezraft::EzStateMachine;
use ezraft::EzStateUpdate;
use ezraft::EzStorage;
use ezraft::EzTypes;
use serde::Deserialize;
use serde::Serialize;
use tokio::fs;

// Define application request types
#[derive(Serialize, Deserialize, Debug, Clone, derive_more::Display)]
pub enum Request {
    #[display("Set({key})")]
    Set { key: String, value: String },
    #[display("Get({key})")]
    Get { key: String },
    #[display("Delete({key})")]
    Delete { key: String },
}

// Define application response type
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Response {
    pub value: Option<String>,
}

// Define type configuration
#[derive(serde::Serialize, serde::Deserialize)]
struct KvTypes;
impl EzTypes for KvTypes {
    type Request = Request;
    type Response = Response;
}

// In-memory state machine
struct KvStateMachine {
    data: BTreeMap<String, String>,
}

impl KvStateMachine {
    fn new() -> Self {
        Self { data: BTreeMap::new() }
    }
}

#[async_trait::async_trait]
impl EzStateMachine<KvTypes> for KvStateMachine {
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

    async fn build_snapshot(&self) -> io::Result<Vec<u8>> {
        serde_json::to_vec(&self.data).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    async fn install_snapshot(&mut self, data: &[u8]) -> io::Result<()> {
        self.data = serde_json::from_slice(data)?;
        Ok(())
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
impl EzStorage<KvTypes> for FileStorage {
    async fn load_state(&mut self) -> io::Result<(EzMeta<KvTypes>, Option<EzSnapshot<KvTypes>>)> {
        // Load meta (use default if not found)
        let meta = match fs::read(&self.meta_path()).await {
            Ok(data) => serde_json::from_slice(&data)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => EzMeta::default(),
            Err(e) => return Err(e),
        };

        // Load snapshot (optional)
        let snapshot = match fs::read(&self.snapshot_meta_path()).await {
            Ok(meta_data) => {
                let snap_meta: EzSnapshotMeta<KvTypes> = serde_json::from_slice(&meta_data)?;
                let data = fs::read(&self.snapshot_data_path()).await?;
                Some(EzSnapshot {
                    meta: snap_meta,
                    snapshot: Cursor::new(data),
                })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };

        Ok((meta, snapshot))
    }

    async fn save_state(&mut self, update: EzStateUpdate<KvTypes>) -> io::Result<()> {
        match update {
            EzStateUpdate::WriteMeta(meta) => {
                fs::write(&self.meta_path(), serde_json::to_vec_pretty(&meta)?).await?;
            }
            EzStateUpdate::WriteLog(entry) => {
                fs::create_dir_all(&self.logs_dir()).await?;
                let (_, index) = entry.log_id;
                fs::write(self.log_path(index), serde_json::to_vec(&entry)?).await?;
            }
            EzStateUpdate::WriteSnapshot(snapshot) => {
                fs::write(&self.snapshot_meta_path(), serde_json::to_vec(&snapshot.meta)?).await?;
                // Extract data from cursor
                let mut cursor = snapshot.snapshot;
                cursor.seek(SeekFrom::Start(0))?;
                let mut data = Vec::new();
                cursor.read_to_end(&mut data)?;
                fs::write(&self.snapshot_data_path(), data).await?;
            }
        }
        Ok(())
    }

    async fn load_log_range(&mut self, start: u64, end: u64) -> io::Result<Vec<EzEntry<KvTypes>>> {
        let mut logs = Vec::new();

        for index in start..end {
            let data = match fs::read(&self.log_path(index)).await {
                Ok(d) => d,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            logs.push(serde_json::from_slice(&data)?);
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
    let store = KvStateMachine::new();
    let storage = FileStorage::new(base_dir);

    // Create EzRaft instance
    let raft = EzRaft::<KvTypes, _, _>::new(node_id, addr.clone(), store, storage, EzConfig::default()).await?;

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
