# EzRaft

A beginner-friendly Raft consensus framework built on [OpenRaft](https://github.com/datafuselabs/openraft). EzRaft handles all Raft complexity internally - users only provide business logic and storage persistence.

## Overview

[Raft](https://raft.github.io/) is a consensus algorithm for distributed systems. EzRaft simplifies building Raft-based applications by:

- **Minimal user API**: 3 methods total (2 storage + 1 state machine) vs 21+ in OpenRaft
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
use ezraft::{EzRaft, EzConfig, EzStorage, EzStateMachine, EzMeta, EzFullState, EzStateUpdate, EzTypes};
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;

// 1. Define your request/response types
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Request { Set { key: String, value: String } }

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response { pub value: Option<String> }

// 2. Implement EzTypes trait
struct MyAppTypes;
impl EzTypes for MyAppTypes {
    type Request = Request;
    type Response = Response;
}

// 3. Implement storage persistence (2 methods)
struct FileStorage { base_dir: PathBuf }

#[async_trait]
impl EzStorage<MyAppTypes> for FileStorage {
    async fn load_state(&mut self) -> Result<Option<EzFullState<MyAppTypes>>, io::Error> {
        // Load meta, logs, and snapshot from disk
        // Return None if first run
    }

    async fn save_state(&mut self, update: EzStateUpdate<MyAppTypes>) -> Result<(), io::Error> {
        // Persist state updates to disk
    }
}

// 4. Implement state machine (1 method)
struct MyStore { data: BTreeMap<String, String> }

#[async_trait]
impl EzStateMachine<MyAppTypes> for MyStore {
    async fn apply(&mut self, req: Request) -> Response {
        match req {
            Request::Set { key, value } => {
                self.data.insert(key, value);
                Response { value: None }
            }
        }
    }
}

// 5. Use it
#[tokio::main]
async fn main() -> Result<()> {
    let store = MyStore { data: BTreeMap::new() };
    let storage = FileStorage { base_dir: "./data".into() };

    let raft = EzRaft::<MyAppTypes, _, _>::new(
        1,
        "127.0.0.1:8080".into(),
        store,
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
    async fn load_state(&mut self) -> Result<Option<EzFullState<T>>, io::Error>;
    async fn save_state(&mut self, update: EzStateUpdate<T>) -> Result<(), io::Error>;
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
}
```

**Framework handles**: Sequential application, error responses

**You handle**: Business logic

## Configuration

`EzConfig` provides sensible defaults for Raft timing parameters:

```rust
pub struct EzConfig {
    pub heartbeat_interval_ms: u64,  // Default: 500
    pub election_timeout_ms: u64,    // Default: 1500
}
```

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
| Required methods | 21+ | 3 |
| User-defined types | 12 (all generic parameters) | 2 (Request, Response) |
| Network code | User implements (~100 lines) | Built-in (0 lines) |
| Example complexity | ~300 lines | ~50 lines |

## License

MIT OR Apache-2.0
