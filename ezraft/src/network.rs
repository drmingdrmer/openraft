//! HTTP network layer for EzRaft
//!
//! This module provides the built-in HTTP networking that connects Raft nodes.
//! Users don't need to implement anything - the framework handles all RPC communication.

use std::fmt::Display;
use std::io;

use openraft::error::Infallible;
use openraft::error::InstallSnapshotError;
use openraft::error::NetworkError;
use openraft::error::RPCError;
use openraft::error::RaftError;
use openraft::error::RemoteError;
use openraft::error::Unreachable;
use openraft::network::RPCOption;
use openraft::network::RaftNetworkFactory;
use openraft::raft::AppendEntriesRequest;
use openraft::raft::AppendEntriesResponse;
use openraft::raft::InstallSnapshotRequest;
use openraft::raft::InstallSnapshotResponse;
use openraft::raft::VoteRequest;
use openraft::raft::VoteResponse;
use openraft::AnyError;
use openraft::BasicNode;
use openraft_legacy::network_v1::Adapter;
use openraft_legacy::network_v1::RaftNetwork as RaftNetworkV1;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::type_config::EzTypes;
use crate::type_config::OpenRaftTypes;
use crate::types::EzSnapshotData;

/// Type alias for OpenRaft types
type C<T> = OpenRaftTypes<T>;

/// HTTP network factory
///
/// Creates HTTP clients to communicate with other Raft nodes.
/// Implements OpenRaft's `RaftNetworkFactory` trait.
pub struct EzNetworkFactory {
    client: Client,
}

impl EzNetworkFactory {
    /// Create a new network factory
    ///
    /// The HTTP client is built once here and handed to every peer, so the whole node shares one
    /// connection pool instead of opening a fresh one per peer.
    pub fn new() -> Result<Self, io::Error> {
        let client = Client::builder().no_proxy().build().map_err(io::Error::other)?;

        Ok(Self { client })
    }
}

impl<T: EzTypes> RaftNetworkFactory<C<T>> for EzNetworkFactory {
    type Network = Adapter<C<T>, Network, EzSnapshotData>;

    async fn new_client(&mut self, target: u64, node: &BasicNode) -> Self::Network {
        let addr = node.addr.clone();
        let client = self.client.clone();

        Network { addr, client, target }.into_v2()
    }
}

/// HTTP network client for a single Raft node
pub struct Network {
    addr: String,
    client: Client,
    target: u64,
}

impl Network {
    /// Send an HTTP POST request to a target node
    async fn request<Req, Resp, Err, Cfg>(
        &mut self,
        uri: impl Display,
        req: Req,
    ) -> Result<Result<Resp, Err>, RPCError<Cfg>>
    where
        Cfg: openraft::RaftTypeConfig,
        Req: Serialize + 'static,
        Resp: Serialize + DeserializeOwned,
        Err: std::error::Error + Serialize + DeserializeOwned,
    {
        let url = format!("http://{}/{}", self.addr, uri);

        let resp = self.client.post(url.clone()).json(&req).send().await.map_err(|e| {
            if e.is_connect() {
                RPCError::Unreachable(Unreachable::new(&e))
            } else {
                RPCError::Network(NetworkError::new(&e))
            }
        })?;

        // The body is a serialized `Result` only on success; any other status carries a
        // plain-text reason that would otherwise surface as an opaque deserialization failure.
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let err = AnyError::error(format!("{} responded {}: {}", url, status, body));
            return Err(RPCError::Network(NetworkError::new(&err)));
        }

        let res: Result<Resp, Err> = resp.json().await.map_err(|e| NetworkError::new(&e))?;

        Ok(res)
    }
}

/// Implement RaftNetwork (v1 API) for HTTP transport
#[allow(clippy::blocks_in_conditions)]
impl<T: EzTypes> RaftNetworkV1<C<T>> for Network {
    async fn append_entries(
        &mut self,
        req: AppendEntriesRequest<C<T>>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<C<T>>, RPCError<C<T>, RaftError<C<T>>>> {
        let res = self
            .request::<_, _, Infallible, C<T>>("raft/append", req)
            .await
            .map_err(RPCError::with_raft_error)?;
        Ok(res.unwrap())
    }

    async fn install_snapshot(
        &mut self,
        req: InstallSnapshotRequest<C<T>>,
        _option: RPCOption,
    ) -> Result<InstallSnapshotResponse<C<T>>, RPCError<C<T>, RaftError<C<T>, InstallSnapshotError>>> {
        let res = self.request::<_, _, _, C<T>>("raft/snapshot", req).await.map_err(RPCError::with_raft_error)?;
        match res {
            Ok(resp) => Ok(resp),
            Err(e) => Err(RPCError::RemoteError(RemoteError::new(
                self.target,
                RaftError::APIError(e),
            ))),
        }
    }

    async fn vote(
        &mut self,
        req: VoteRequest<C<T>>,
        _option: RPCOption,
    ) -> Result<VoteResponse<C<T>>, RPCError<C<T>, RaftError<C<T>>>> {
        let res = self.request::<_, _, Infallible, C<T>>("raft/vote", req).await.map_err(RPCError::with_raft_error)?;
        Ok(res.unwrap())
    }
}
