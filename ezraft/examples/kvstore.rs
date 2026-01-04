//! EzRaft KV Store Example
//!
//! A simple distributed key-value store built on EzRaft.
//! Run multiple instances to form a cluster:
//!
//! ```bash
//! # Terminal 1 (first node - creates cluster)
//! cargo run --example kvstore -- --addr 127.0.0.1:8080
//!
//! # Terminal 2 (joins via seed node)
//! cargo run --example kvstore -- --addr 127.0.0.1:8081 --seed 127.0.0.1:8080
//!
//! # Terminal 3 (joins via seed node)
//! cargo run --example kvstore -- --addr 127.0.0.1:8082 --seed 127.0.0.1:8080
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
use ezraft::EzStorage;
use ezraft::EzTypes;
use ezraft::Persist;
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
struct Types;
impl EzTypes for Types {
    type Request = Request;
    type Response = Response;
}

// In-memory state machine
#[derive(Default)]
struct KvStateMachine {
    data: BTreeMap<String, String>,
}

#[async_trait::async_trait]
impl EzStateMachine<Types> for KvStateMachine {
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
    async fn new(base_dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&base_dir).await?;
        Ok(Self { base_dir })
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
impl EzStorage<Types> for FileStorage {
    async fn restore(&mut self) -> io::Result<(EzMeta<Types>, Option<EzSnapshot<Types>>)> {
        // Load meta (use default if not found)
        let meta = match fs::read(&self.meta_path()).await {
            Ok(data) => serde_json::from_slice(&data)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => EzMeta::default(),
            Err(e) => return Err(e),
        };

        // Load snapshot (optional)
        let snapshot = match fs::read(&self.snapshot_meta_path()).await {
            Ok(meta_data) => {
                let snap_meta: EzSnapshotMeta<Types> = serde_json::from_slice(&meta_data)?;
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

    async fn persist(&mut self, op: Persist<Types>) -> io::Result<()> {
        match op {
            Persist::Meta(meta) => {
                fs::write(&self.meta_path(), serde_json::to_vec_pretty(&meta)?).await?;
            }
            Persist::LogEntry(entry) => {
                fs::create_dir_all(&self.logs_dir()).await?;
                let (_, index) = entry.log_id;
                fs::write(self.log_path(index), serde_json::to_vec(&entry)?).await?;
            }
            Persist::Snapshot(snapshot) => {
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

    async fn read_logs(&mut self, start: u64, end: u64) -> io::Result<Vec<EzEntry<Types>>> {
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
    /// HTTP bind address (e.g., "127.0.0.1:8080")
    #[arg(long, default_value = "127.0.0.1:8080")]
    addr: String,

    /// Seed node address to join existing cluster
    #[arg(long)]
    seed: Option<String>,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    let addr = args.addr;
    let seed = args.seed;

    // Create state machine and storage (use addr for directory name)
    let base_dir = PathBuf::from(format!("./data/{}", addr.replace(':', "-")));
    let state_machine = KvStateMachine::default();
    let storage = FileStorage::new(base_dir).await?;

    // Create EzRaft instance (auto-joins or initializes based on seed)
    let raft = EzRaft::<Types, _, _>::new(&addr, state_machine, storage, EzConfig::default(), seed).await?;

    println!("Node {} listening on {}", raft.node_id(), addr);

    // Start HTTP server
    raft.serve().await?;

    Ok(())
}
