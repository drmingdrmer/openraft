# EzRaft

A beginner-friendly Raft consensus framework built on [OpenRaft](https://github.com/datafuselabs/openraft). EzRaft handles all Raft complexity internally - users only provide business logic and storage persistence.

## Overview

[Raft](https://raft.github.io/) is a consensus algorithm for distributed systems. EzRaft simplifies building Raft-based applications by:

- **Minimal user API**: 6 methods total (3 storage + 3 state machine) vs 21+ in OpenRaft
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
use ezraft::{EzRaft, EzConfig, EzStorage, EzStateMachine, EzMeta, EzSnapshot, EzEntry, EzStateUpdate, EzTypes};
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;

// 1. Define your request/response types
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Request { Set { key: String, value: String } }

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Response { pub value: Option<String> }

// 2. Implement EzTypes trait
struct AppTypes;
impl EzTypes for AppTypes {
    type Request = Request;
    type Response = Response;
}

// 3. Implement storage persistence (3 methods)
struct AppStorage { base_dir: PathBuf }

#[async_trait]
impl EzStorage<AppTypes> for AppStorage {
    async fn load_state(&mut self) -> Result<(EzMeta<AppTypes>, Option<EzSnapshot<AppTypes>>), io::Error> {
        // Load meta (or default) and optional snapshot from disk
    }

    async fn save_state(&mut self, update: EzStateUpdate<AppTypes>) -> Result<(), io::Error> {
        // Persist state updates to disk
    }

    async fn load_log_range(&mut self, start: u64, end: u64) -> Result<Vec<EzEntry<AppTypes>>, io::Error> {
        // Load log entries in range [start, end)
    }
}

// 4. Implement state machine (3 methods)
struct AppStateMachine { data: BTreeMap<String, String> }

#[async_trait]
impl EzStateMachine<AppTypes> for AppStateMachine {
    async fn apply(&mut self, req: Request) -> Response {
        match req {
            Request::Set { key, value } => {
                self.data.insert(key, value);
                Response { value: None }
            }
        }
    }

    async fn build_snapshot(&self) -> io::Result<Vec<u8>> {
        // Serialize state machine to bytes
    }

    async fn install_snapshot(&mut self, data: &[u8]) -> io::Result<()> {
        // Restore state machine from bytes
    }
}

// 5. Use it
#[tokio::main]
async fn main() -> Result<()> {
    let state_machine = AppStateMachine { data: BTreeMap::new() };
    let storage = AppStorage { base_dir: "./data".into() };

    let raft = EzRaft::<AppTypes, _, _>::new(
        1,
        "127.0.0.1:8080".into(),
        state_machine,
        storage,
        EzConfig::default()
    ).await?;

    raft.initialize(vec![(1, "127.0.0.1:8080".into())]).await?;
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
    T: EzTypes,
{
    async fn load_state(&mut self) -> Result<(EzMeta<T>, Option<EzSnapshot<T>>), io::Error>;
    async fn save_state(&mut self, update: EzStateUpdate<T>) -> Result<(), io::Error>;
    async fn load_log_range(&mut self, start: u64, end: u64) -> Result<Vec<EzEntry<T>>, io::Error>;
}
```

**Framework handles**: Raft logic, when to persist, what to persist

**You handle**: Serialization and I/O

### EzStateMachine

Handles business logic (applying requests to state).

```rust
#[async_trait]
pub trait EzStateMachine<T>: Send + Sync + 'static
where
    T: EzTypes,
{
    async fn apply(&mut self, req: T::Request) -> T::Response;
    async fn build_snapshot(&self) -> io::Result<Vec<u8>>;
    async fn install_snapshot(&mut self, data: &[u8]) -> io::Result<()>;
}
```

**Framework handles**: Sequential application, snapshot scheduling

**You handle**: Business logic, state serialization

## Configuration

`EzConfig` provides sensible defaults for Raft timing parameters:

```rust
pub struct EzConfig {
    pub heartbeat_interval: Duration,  // Default: 500ms
}
```

Election timeout is automatically calculated as 3-6x the heartbeat interval.

Most users can use `EzConfig::default()`.

## HTTP API

EzRaft includes built-in HTTP endpoints:

- **Raft RPC** (`/raft/*`): Internal consensus communication
- **Admin API** (`/api/*`): Initialize, add learners, change membership, metrics
- **Application API**: Propose client requests (user-defined)

## Comparison with OpenRaft

| Aspect | OpenRaft | EzRaft |
|--------|----------|--------|
| Required traits | 7+ (RaftLogStorage, RaftStateMachine, etc.) | 2 (EzStorage, EzStateMachine) |
| Required methods | 21+ | 6 |
| User-defined types | 12 (all generic parameters) | 2 (Request, Response) |
| Network code | User implements (~100 lines) | Built-in (0 lines) |
| Example complexity | ~300 lines | ~50 lines |

## License

MIT OR Apache-2.0
