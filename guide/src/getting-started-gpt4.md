# Getting Started

In this chapter, we will build a key-value store cluster using [openraft](https://github.com/datafuselabs/openraft).

The complete example applications, including the server, client, and a demo cluster, can be found at [examples/raft-kv-memstore](https://github.com/datafuselabs/openraft/tree/main/examples/raft-kv-memstore) and [examples/raft-kv-rocksdb](https://github.com/datafuselabs/openraft/tree/main/examples/raft-kv-rocksdb), with the latter using RocksDB for persistent storage.

---

Raft is a distributed consensus protocol designed to manage a replicated log containing state machine commands from clients.

<p>
    <img style="max-width:600px;" src="./images/raft-overview.png"/>
</p>

Raft consists of two main components:

- Log replication among nodes,
- and log consumption, which is primarily defined in the state machine.

Implementing your own Raft-based application with openraft is straightforward and involves:

- Defining client requests and responses;
- Implementing storage for Raft to store its state;
- Implementing a network layer for Raft to transmit messages.

## 1. Define client requests and responses

A request is data that modifies the Raft state machine, while a response is data that the Raft state machine returns to the client.

Request and response types should implement `AppData` and `AppDataResponse`, respectively. For example:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExampleRequest {/* fields */}
impl AppData for ExampleRequest {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExampleResponse(Result<Option<String>, ClientError>);
impl AppDataResponse for ExampleResponse {}
```

These two types are application-specific and mainly related to the state machine implementation in `RaftStorage`.

## 2. Implement `RaftStorage`

The `RaftStorage` trait defines how data is stored and consumed. It can be a wrapper for a local KV store like [RocksDB](https://docs.rs/rocksdb/latest/rocksdb/) or a remote SQL DB.

`RaftStorage` requires implementing four sets of APIs:

- Read/write Raft state, e.g., term or vote.
    ```rust
    fn save_vote(vote:&Vote)
    fn read_vote() -> Result<Option<Vote>>
    ```

- Read/write logs.
    ```rust
    fn get_log_state() -> Result<LogState>
    fn try_get_log_entries(range) -> Result<Vec<Entry>>

    fn append_to_log(entries)

    fn delete_conflict_logs_since(since:LogId)
    fn purge_logs_upto(upto:LogId)
    ```

- Apply log entry to the state machine.
    ```rust
    fn last_applied_state() -> Result<(Option<LogId>, Option<EffectiveMembership>)>
    fn apply_to_state_machine(entries) -> Result<Vec<AppResponse>>
    ```

- Build and install a snapshot.
    ```rust
    fn build_snapshot() -> Result<Snapshot>
    fn get_current_snapshot() -> Result<Option<Snapshot>>

    fn begin_receiving_snapshot() -> Result<Box<SnapshotData>>
    fn install_snapshot(meta, snapshot)
    ```

The APIs are self-explanatory, and a good example is [`ExampleStore`](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/src/store/mod.rs), a pure in-memory implementation that demonstrates what should be done when a method is called.

### How to implement RaftStorage correctly?

A [Test suite for RaftStorage](https://github.com/datafuselabs/openraft/blob/main/memstore/src/test.rs) is available. If an implementation passes the test, openraft will work seamlessly with it.

To test your implementation with this suite, do the following:

```rust
#[test]
pub fn test_mem_store() -> anyhow::Result<()> {
  openraft::testing::Suite::test_all(MemStore::new)
}
```

A second example in [Test suite for RaftStorage](https://github.com/datafuselabs/openraft/blob/main/rocksstore/src/test.rs) demonstrates building a RocksDB-backed store.

### Race condition regarding RaftStorage

In our design, at most one thread writes data at a time. However, multiple threads may read from it concurrently, e.g., more than one replication task reading log entries from the store.

### An implementation must guarantee data durability.

The caller always assumes a completed write is persistent. Raft's correctness highly depends on a reliable store.

## 3. Implement `RaftNetwork`

Raft nodes need to communicate with each other to achieve consensus about the logs. The `RaftNetwork` trait defines the data transmission requirements.

An implementation of `RaftNetwork` can be considered a wrapper that invokes the corresponding methods of a remote `Raft`.

```rust
pub trait RaftNetwork<D>: Send + Sync + 'static
where D: AppData
{
    async fn send_append_entries(&mut self, rpc: AppendEntriesRequest<D>) -> Result<AppendEntriesResponse, RPCError<RaftError>>;
    async fn send_install_snapshot(&mut self, rpc: InstallSnapshotRequest,) -> Result<InstallSnapshotResponse, RPCError<RaftError<InstallSnapshotError>>>;
    async fn send_vote(&mut self, rpc: VoteRequest) -> Result<VoteResponse, RPCError<RaftError>>;
}
```

[ExampleNetwork](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/src/network/raft_network_impl.rs) demonstrates how to forward messages to other Raft nodes.

There should also be a server endpoint for each of these RPCs. When the server receives a Raft RPC, it passes it to its `raft` instance and replies with the returned result: [raft-server-endpoint](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/src/network/raft.rs).

For a real-world implementation, you may want to use [Tonic gRPC](https://github.com/hyperium/tonic). [databend-meta](https://github.com/datafuselabs/databend/blob/6603392a958ba8593b1f4b01410bebedd484c6a9/metasrv/src/network.rs#L89) is an excellent real-world example.

### Implement `RaftNetworkFactory`

`RaftNetworkFactory` is a singleton that creates `RaftNetwork` instances for each replication stream.

```rust
pub trait RaftNetworkFactory<C>: Send + Sync + 'static
    where C: RaftTypeConfig
{
    type Network: RaftNetwork<C>;
    async fn new_client(&mut self, target: C::NodeId, node: &C::Node) -> Self::Network;
}
```

`RaftNetworkFactory::Network` is the application implementation of the `RaftNetwork` trait. The only method `new_client` creates a new network instance for sending RPCs to the target node.

This function should **not** create a connection but rather a client that will connect when required.

### Find the address of the target node.

An `RaftNetwork` implementation needs to connect to the remote raft peer,
using TCP, etc.

There are two ways to find the address of a remote peer:

1. Manage the mapping from node-id to address by yourself.

2. `openraft` allows you to store additional info in its Membership,
   which is automatically replicated as regular logs.

   To use this feature, you need to specify the type in `RaftTypeConfig`, with:

   ```rust
   openraft::declare_raft_types!(
      pub TypeConfig:
          D = Request,
          R = Response,
          NodeId = NodeId,
          Node = openraft::BasicNode,
          Entry = openraft::Entry<TypeConfig>,
          SnapshotData = Cursor<Vec<u8>>
   );
   ```

   - `Raft::add_learner(node_id, Node::new("127.0.0.1"), ...)` tells `openraft`
     to store node-id and its address in `Membership` too:

     ```json
     {
        "configs": [ [ 1, 2, 3 ] ],
        "nodes": {
          "1": { "addr": "127.0.0.1:21001", "data": {} },
          "2": { "addr": "127.0.0.1:21002", "data": {} },
          "3": { "addr": "127.0.0.1:21003", "data": {} }
        }
     }
     ```

## 4. Define types for the application

Openraft is a generic implementation of Raft, requiring the application to define
concrete types for these generic arguments, and most types are parameterized by [`RaftTypeConfig`]:

```rust
openraft::declare_raft_types!(
   pub Config:
    D = ClientRequest,
    R = ClientResponse,
    NodeId = MemNodeId,
    Node = openraft::BasicNode,
    Entry = openraft::Entry<Config>,
    SnapshotData = Cursor<Vec<u8>>
);
```

The above declaration defines a `RaftTypeConfig` implementation as follows and will be used by the `Raft` struct:

```rust
pub struct TypeConfig {}
impl openraft::RaftTypeConfig for TypeConfig {
    type D = Request;
    type R = Response;
    type NodeId = NodeId;
    type Node = BasicNode;
    type Entry = openraft::Entry<TypeConfig>;
    type SnapshotData = Cursor<Vec<u8>>;
}
```

```rust
pub struct Raft<C: RaftTypeConfig, N, LS, SM> {}
```

Openraft provides default implementations for `Node`(`EmptyNode` and `BasicNode`) and `Entry`(`Entry`),
which you can use directly or define your own types.

## 5. Put everything together

Finally, we put these parts together and boot up a raft node
[main.rs](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/src/lib.rs)
:

```rust
// Define the types used in the application.
openraft::declare_raft_types!(
    pub TypeConfig: D = Request, R = Response, NodeId = NodeId, Node = BasicNode, Entry = openraft::Entry<TypeConfig>
);

#[tokio::main]
async fn main() {
  #[actix_web::main]
  async fn main() -> std::io::Result<()> {
    // Set up the logger
    env_logger::init_from_env(Env::default().default_filter_or("info"));

    // Parse the parameters passed by arguments.
    let options = Opt::parse();
    let node_id = options.id;

    // Create a configuration for the raft instance.
    let config = Arc::new(Config::default().validate().unwrap());

    // Create an instance of where the Raft data will be stored.
    let store = Arc::new(ExampleStore::default());
      
    let (log_store, state_machine) = Adaptor::new(store.clone());

    // Create the network layer that will connect and communicate the raft instances and
    // will be used in conjunction with the store created above.
    let network = Arc::new(ExampleNetwork {});

    // Create a local raft instance.
    let raft = openraft::Raft::new(node_id, config.clone(), network, log_store, state_machine).await.unwrap();

    // Create an application that will store all the instances created above, this will
    // be later used on the actix-web services.
    let app = Data::new(ExampleApp {
      id: options.id,
      raft,
      store,
      config,
    });

    // Start the actix-web server.
    HttpServer::new(move || {
      App::new()
              .wrap(Logger::default())
              .wrap(Logger::new("%a %{User-Agent}i"))
              .wrap(middleware::Compress::default())
              .app_data(app.clone())
              // raft internal RPC
              .service(raft::append)
              .service(raft::snapshot)
              .service(raft::vote)
              // admin API
              .service(management::init)
              .service(management::add_learner)
              .service(management::change_membership)
              .service(management::metrics)
              .service(management::list_nodes)
              // application API
              .service(api::write)
              .service(api::read)
    })
            .bind(options.http_addr)?
            .run()
            .await
  }
}

```

## 6. Run the cluster

To set up a demo raft cluster, you need to:
- Bring up three uninitialized raft nodes;
- Initialize a single-node cluster;
- Add more raft nodes to it;
- Update the membership config.

[examples/raft-kv-memstore](https://github.com/datafuselabs/openraft/tree/main/examples/raft-kv-memstore) describes these steps in detail.

Additionally, two test scripts for setting up a cluster are provided:

- [test-cluster.sh](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/test-cluster.sh)
  is a simplified bash script that uses curl for communication with the raft cluster,
  demonstrating the messages sent and received in clear HTTP.

- [test_cluster.rs](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/tests/cluster/test_cluster.rs)
  Utilizes ExampleClient to establish a cluster, store data, and subsequently read it.