# Getting Started

This chapter explains how to build a key-value store cluster using [openraft](https://github.com/datafuselabs/openraft). The complete example application, including the server, the client, and a demo cluster, can be found in [examples/raft-kv-memstore](https://github.com/datafuselabs/openraft/tree/main/examples/raft-kv-memstore). Another example application, [examples/raft-kv-rocksdb](https://github.com/datafuselabs/openraft/tree/main/examples/raft-kv-rocksdb), uses rocksdb for persistent storage.

---

Raft is a distributed consensus protocol designed to manage a replicated log containing state machine commands from clients.

<p>
    <img style="max-width:600px;" src="./images/raft-overview.png"/>
</p>


Raft consists of two major parts:

- How to replicate logs consistently among nodes,
- and how to consume the logs, which is defined mainly in state machine.

To implement your own raft-based application with openraft, follow these steps:

## 1. Define client request and response

A request is some data that modifies the raft state machine. A response is some data that the raft state machine returns to the client.

Request and response can be any types that implement `AppData` and `AppDataResponse`, respectively. For example:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExampleRequest {/* fields */}
impl AppData for ExampleRequest {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExampleResponse(Result<Option<String>, ClientError>);
impl AppDataResponse for ExampleResponse {}
```

These two types are application-specific and are mainly related to the state machine implementation in `RaftStorage`.


## 2. Implement `RaftStorage`

The trait `RaftStorage` defines the way that data is stored and consumed. It could be a wrapper of some local KV store such as [RocksDB](https://docs.rs/rocksdb/latest/rocksdb/) or a wrapper of a remote SQL DB.

`RaftStorage` defines four sets of APIs that an application needs to implement:

- Read/write raft state, e.g., term or vote.
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

- Building and installing a snapshot.
    ```rust
    fn build_snapshot() -> Result<Snapshot>
    fn get_current_snapshot() -> Result<Option<Snapshot>>

    fn begin_receiving_snapshot() -> Result<Box<SnapshotData>>
    fn install_snapshot(meta, snapshot)
    ```

The APIs are straightforward, and there is a good example [`ExampleStore`](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/src/store/mod.rs), which is a pure-in-memory implementation that shows what should be done when a method is called.


### How to implement RaftStorage correctly

There is a [Test suite for RaftStorage](https://github.com/datafuselabs/openraft/blob/main/memstore/src/test.rs). If an implementation passes the test, openraft will work happily with it.

To test your implementation with this suite, use the following code:

```rust
#[test]
pub fn test_mem_store() -> anyhow::Result<()> {
  openraft::testing::Suite::test_all(MemStore::new)
}
```

There is a second example in [Test suite for RaftStorage](https://github.com/datafuselabs/openraft/blob/main/rocksstore/src/test.rs) that showcases building a rocksdb backed store.

### Race condition about RaftStorage

In our design, there is at most one thread at a time writing data to it. But there may be several threads reading from it concurrently, e.g., more than one replication task reading log entries from the store.


### An implementation has to guarantee data durability.

The caller always assumes a completed write is persistent. The raft correctness highly depends on a reliable store.


## 3. Implement `RaftNetwork`

Raft nodes need to communicate with each other to achieve consensus about the logs. The trait `RaftNetwork` defines the data transmission requirements.

An implementation of `RaftNetwork` can be considered as a wrapper that invokes the corresponding methods of a remote `Raft`.

```rust
pub trait RaftNetwork<D>: Send + Sync + 'static
where D: AppData
{
    async fn send_append_entries(&mut self, rpc: AppendEntriesRequest<D>) -> Result<AppendEntriesResponse, RPCError<RaftError>>;
    async fn send_install_snapshot(&mut self, rpc: InstallSnapshotRequest,) -> Result<InstallSnapshotResponse, RPCError<RaftError<InstallSnapshotError>>>;
    async fn send_vote(&mut self, rpc: VoteRequest) -> Result<VoteResponse, RPCError<RaftError>>;
}
```

[ExampleNetwork](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/src/network/raft_network_impl.rs) shows how to forward messages to other raft nodes.

There should be a server endpoint for each of these RPCs. When the server receives a raft RPC, it just passes it to its `raft` instance and replies with what returned: [raft-server-endpoint](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/src/network/raft.rs).

As a real-world implementation, you may want to use [Tonic gRPC](https://github.com/hyperium/tonic). [databend-meta](https://github.com/datafuselabs/databend/blob/6603392a958ba8593b1f4b01410bebedd484c6a9/metasrv/src/network.rs#L89) would be an excellent real-world example.


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

`RaftNetworkFactory::Network` is the application implementation of trait `RaftNetwork`. The only method `new_client` creates a new network instance sending RPCs to the target node.

This function should **not** create a connection but rather a client that will connect when required..


### Find the address of the target node.

# RaftNetwork Implementation

In order to connect to a remote raft peer, an implementation of `RaftNetwork` needs to be created through TCP or other means.

There are two ways to find the address of a remote peer:

1. Manage the mapping from node-id to address manually.
2. Use `openraft` to store additional info in its Membership, which is automatically replicated as regular logs.

To use the second option, the type needs to be specified in `RaftTypeConfig` as follows:

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

The `Raft::add_learner(node_id, Node::new("127.0.0.1"), ...)` method tells `openraft` to store the node-id and its address in `Membership` too.

Openraft is a generic implementation of Raft, so the application needs to define concrete types for the generic arguments, and most types are parameterized by [`RaftTypeConfig`].

```rust
openraft::declare_raft_types!(
   pub Config:
    D = ClientRequest,
    R = ClientResponse,
    NodeId = MemNodeId,
    Node = openraft::BasiceNode,
    Entry = openraft::Entry<Config>,
    SnapshotData = Cursor<Vec<u8>>
);
```

The above declaration defines a `RaftTypeConfig` implementation as the following and will be used by struct `Raft`:

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

Openraft provides default implementation for `Node`(`EmptyNode` and `BasiceNode`) and `Entry`(`Entry`), you can use them directly or define your own types.

Finally, all these parts are put together to boot up a raft node in [main.rs](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/src/lib.rs).

To set up a demo raft cluster, including bringing up three uninitialized raft nodes, initializing a single-node cluster, adding more raft nodes into it, and updating the membership config, refer to [examples/raft-kv-memstore](https://github.com/datafuselabs/openraft/tree/main/examples/raft-kv-memstore).

Two test scripts for setting up a cluster are provided.

- The script [test-cluster.sh](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/test-cluster.sh) is written in bash and uses curl to interact with the raft cluster. It displays the HTTP messages that are sent and received in a plain format.

- [test_cluster.rs](https://github.com/datafuselabs/openraft/blob/main/examples/raft-kv-memstore/tests/cluster/test_cluster.rs) sets up a cluster using ExampleClient, writes data to it, and then reads the data.