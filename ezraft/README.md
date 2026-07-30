# EzRaft

A beginner-friendly Raft consensus framework built on [OpenRaft](https://github.com/databendlabs/openraft). EzRaft handles all Raft complexity internally - users only provide business logic and storage persistence.

## Overview

[Raft](https://raft.github.io/) is a consensus algorithm for distributed systems. EzRaft simplifies building Raft-based applications by:

- **Minimal user API**: 4 methods total (3 storage + 1 app) vs 21+ in OpenRaft
- **Smart defaults**: 10/12 Raft types pre-configured, users specify only Request/Response
- **Built-in networking**: HTTP layer included, no user code needed
- **Type-safe**: Works directly with your types, not byte vectors

## Status

**Experimental.** EzRaft is primarily an API design laboratory for exploring intuitive interface patterns. The APIs may change until the crate stabilizes. Production applications are not the primary audience.

**Next phase: Stable API.** Once the design exploration matures, EzRaft will provide a stable API with well-considered abstractions—exposing what users need while hiding unnecessary complexity.

## Goals

**API design exploration.** EzRaft turns abstract ideas about "intuitive APIs" into concrete code. By testing different patterns—parameter organization, naming conventions, simplicity vs extensibility trade-offs—we discover what truly matches user intuition. These insights will guide future OpenRaft improvements.

**Fast prototyping.** As a secondary benefit, EzRaft lets beginners build working prototypes without understanding Raft internals or OpenRaft's architecture.

## Quick Start

```rust
use ezraft::{EzRaft, EzConfig, EzApp, EzStorage, EzEntry, Loaded, Persist};
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;

// 1. Define your request/response types; openraft logs requests, hence the Display
#[derive(Serialize, Deserialize, Debug, Clone, derive_more::Display)]
pub enum Request {
    #[display("Set({key})")]
    Set { key: String, value: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response { pub value: Option<String> }

// 2. Define your app: the state plus one method of business logic.
//    Snapshots are derived from the state via serde.
#[derive(Default, Serialize, Deserialize)]
struct App { data: BTreeMap<String, String> }

#[async_trait]
impl EzApp for App {
    type Request = Request;
    type Response = Response;

    async fn apply(&mut self, req: Request) -> Response {
        match req {
            Request::Set { key, value } => {
                self.data.insert(key, value);
                Response { value: None }
            }
        }
    }
}

// 3. Implement storage persistence (3 methods)
struct AppStorage { base_dir: PathBuf }

#[async_trait]
impl EzStorage<App> for AppStorage {
    async fn load(&mut self) -> Result<Loaded, io::Error> {
        // Load meta (or default) and optional snapshot from disk
        Ok(Loaded { meta, snapshot })
    }

    async fn persist(&mut self, op: Persist<App>) -> Result<(), io::Error> {
        // Persist operation to disk
    }

    async fn read_logs(&mut self, start: u64, end: u64) -> Result<Vec<EzEntry<App>>, io::Error> {
        // Read log entries in range [start, end)
    }
}

// 4. Use it
#[tokio::main]
async fn main() -> Result<()> {
    let storage = AppStorage { base_dir: "./data".into() };

    // First node (creates cluster)
    let raft = EzRaft::create(
        "127.0.0.1:8080",
        App::default(),
        storage,
        EzConfig::default(),
    ).await?;

    // Every other node joins it:
    // EzRaft::join("127.0.0.1:8081", "127.0.0.1:8080", app, storage, config).await?
    raft.serve().await?;
}
```

See `examples/kvstore.rs` for a complete working example.

## User Traits

### EzStorage

Handles persistence of Raft state (metadata, logs, snapshots).

```rust
#[async_trait]
pub trait EzStorage<T>: Send + Sync + 'static
where
    T: EzApp,
{
    async fn load(&mut self) -> Result<Loaded, io::Error>;
    async fn persist(&mut self, op: Persist<T>) -> Result<(), io::Error>;
    async fn read_logs(&mut self, start: u64, end: u64) -> Result<Vec<EzEntry<T>>, io::Error>;
}
```

**Framework handles**: Raft logic, when to persist, what to persist

**You handle**: Serialization and I/O

### EzApp

The application itself: the implementing type is the replicated state, and
`apply` is the business logic. Snapshots are derived from the state via serde,
so there is nothing to implement for them.

```rust
#[async_trait]
pub trait EzApp: Serialize + DeserializeOwned + Send + Sync + 'static {
    type Request: ...;
    type Response: ...;

    async fn apply(&mut self, req: Self::Request) -> Self::Response;
}
```

**Framework handles**: Sequential application, snapshot build/restore and scheduling

**You handle**: Business logic

## Configuration

`EzConfig` provides sensible defaults for Raft timing parameters:

```rust
pub struct EzConfig {
    pub heartbeat_interval: Duration,  // Default: 500ms
    pub snapshot_interval: u64,        // Log entries between snapshots. Default: 500
}
```

Election timeout is automatically calculated as 3-6x the heartbeat interval,
and the log-purge distance is derived from `snapshot_interval`.

Most users can use `EzConfig::default()`.

## HTTP API

EzRaft includes built-in HTTP endpoints:

- **Raft RPC** (`/raft/*`): Internal consensus communication
- **Admin API** (`/api/*`): Join, change membership, metrics
- **Application API** (`/api/write`): Propose client requests, using your request type as JSON
- **Application read** (`/api/read`): The serialized application state, read from local memory without a consensus round

## Comparison with OpenRaft

| Aspect | OpenRaft | EzRaft |
|--------|----------|--------|
| Required traits | 7+ (RaftLogStorage, RaftStateMachine, etc.) | 2 (EzStorage, EzApp) |
| Required methods | 21+ | 4 |
| User-defined types | 12 (all generic parameters) | 2 (Request, Response) |
| Network code | User implements (~100 lines) | Built-in (0 lines) |
| Example complexity | ~400 lines | ~280 lines |

## License

MIT OR Apache-2.0
