//! HTTP network layer for EzRaft
//!
//! This module provides the built-in HTTP networking that connects Raft nodes.
//! Users don't need to implement anything - the framework handles all RPC communication.

use std::fmt::Display;

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
use openraft::BasicNode;
use openraft::RaftTypeConfig;
use openraft_legacy::network_v1::Adapter;
use openraft_legacy::network_v1::RaftNetwork as RaftNetworkV1;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::AsyncRead;
use tokio::io::AsyncSeek;
use tokio::io::AsyncWrite;

/// HTTP network factory
///
/// Creates HTTP clients to communicate with other Raft nodes.
/// Implements OpenRaft's `RaftNetworkFactory` trait.
pub struct EzNetworkFactory<C>(std::marker::PhantomData<C>);

impl<C> Default for EzNetworkFactory<C> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<C> EzNetworkFactory<C> {
    /// Create a new network factory
    pub fn new() -> Self {
        Self::default()
    }
}

impl<C> RaftNetworkFactory<C> for EzNetworkFactory<C>
where
    C: RaftTypeConfig<Node = BasicNode> + 'static,
    C::SnapshotData: AsyncRead + AsyncWrite + AsyncSeek + Unpin,
{
    type Network = Adapter<C, Network<C>>;

    async fn new_client(&mut self, target: C::NodeId, node: &BasicNode) -> Self::Network {
        let addr = node.addr.clone();
        let client = Client::builder().no_proxy().build().unwrap();

        Network { addr, client, target }.into_v2()
    }
}

/// HTTP network client for a single Raft node
pub struct Network<C>
where C: RaftTypeConfig
{
    addr: String,
    client: Client,
    target: C::NodeId,
}

impl<C> Network<C>
where C: RaftTypeConfig
{
    /// Send an HTTP POST request to a target node
    async fn request<Req, Resp, Err>(&mut self, uri: impl Display, req: Req) -> Result<Result<Resp, Err>, RPCError<C>>
    where
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

        let res: Result<Resp, Err> = resp.json().await.map_err(|e| NetworkError::new(&e))?;

        Ok(res)
    }
}

/// Implement RaftNetwork (v1 API) for HTTP transport
#[allow(clippy::blocks_in_conditions)]
impl<C> RaftNetworkV1<C> for Network<C>
where C: RaftTypeConfig
{
    async fn append_entries(
        &mut self,
        req: AppendEntriesRequest<C>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<C>, RPCError<C, RaftError<C>>> {
        let res = self.request::<_, _, Infallible>("raft/append", req).await.map_err(RPCError::with_raft_error)?;
        Ok(res.unwrap())
    }

    async fn install_snapshot(
        &mut self,
        req: InstallSnapshotRequest<C>,
        _option: RPCOption,
    ) -> Result<InstallSnapshotResponse<C>, RPCError<C, RaftError<C, InstallSnapshotError>>> {
        let res = self.request("raft/snapshot", req).await.map_err(RPCError::with_raft_error)?;
        match res {
            Ok(resp) => Ok(resp),
            Err(e) => Err(RPCError::RemoteError(RemoteError::new(
                self.target.clone(),
                RaftError::APIError(e),
            ))),
        }
    }

    async fn vote(
        &mut self,
        req: VoteRequest<C>,
        _option: RPCOption,
    ) -> Result<VoteResponse<C>, RPCError<C, RaftError<C>>> {
        let res = self.request::<_, _, Infallible>("raft/vote", req).await.map_err(RPCError::with_raft_error)?;
        Ok(res.unwrap())
    }
}
