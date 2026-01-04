//! HTTP server for EzRaft
//!
//! This module provides the HTTP server that handles:
//! - Internal Raft RPC (append entries, vote, install snapshot)
//! - Admin API (initialize, add learner, change membership, metrics)

use std::sync::Arc;

use actix_web::web;
use actix_web::web::Data;
use actix_web::App;
use actix_web::HttpServer;
use openraft::async_runtime::WatchReceiver;
use openraft::raft;
use serde::Deserialize;

use crate::raft::EzRaft;
use crate::type_config::EzTypes;
use crate::type_config::OpenRaftTypes;

/// Type alias for OpenRaft types
type C<T> = OpenRaftTypes<T>;

/// Run the HTTP server
pub(crate) async fn run<T, S, M>(raft: &EzRaft<T, S, M>) -> std::io::Result<()>
where
    T: EzTypes,
    S: crate::trait_::EzStorage<T>,
    M: crate::trait_::EzStateMachine<T>,
{
    let addr = raft.addr().to_string();

    let raft_data = Data::new(raft.inner().clone());

    let server = HttpServer::new(move || {
        App::new()
            .app_data(raft_data.clone())
            // Raft internal RPC
            .route("/raft/append", web::post().to(append::<T>))
            .route("/raft/vote", web::post().to(vote::<T>))
            // Admin API
            .route("/api/init", web::post().to(initialize::<T>))
            .route("/api/join", web::post().to(join::<T>))
            .route("/api/add_learner", web::post().to(add_learner::<T>))
            .route("/api/change_membership", web::post().to(change_membership::<T>))
            .route("/api/metrics", web::get().to(metrics::<T>))
    })
    .bind(&addr)?;

    server.run().await
}

/// Raft append entries RPC handler
async fn append<T>(
    req: web::Json<raft::AppendEntriesRequest<C<T>>>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<raft::AppendEntriesResponse<C<T>>>, actix_web::Error>
where
    T: EzTypes,
{
    let resp = data
        .append_entries(req.into_inner())
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("append_entries failed: {}", e)))?;

    Ok(web::Json(resp))
}

/// Raft vote RPC handler
async fn vote<T>(
    req: web::Json<raft::VoteRequest<C<T>>>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<raft::VoteResponse<C<T>>>, actix_web::Error>
where
    T: EzTypes,
{
    let resp = data
        .vote(req.into_inner())
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("vote failed: {}", e)))?;

    Ok(web::Json(resp))
}

/// Initialize cluster API handler
async fn initialize<T>(
    req: web::Json<InitializeRequest>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<serde_json::Value>, actix_web::Error>
where
    T: EzTypes,
{
    use std::collections::BTreeMap;

    use openraft::BasicNode;

    let members: BTreeMap<u64, BasicNode> =
        req.into_inner().members.into_iter().map(|(id, addr)| (id, BasicNode::new(addr))).collect();

    data.initialize(members)
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("initialize failed: {}", e)))?;

    Ok(web::Json(serde_json::json!({ "status": "ok" })))
}

/// Add learner API handler
async fn add_learner<T>(
    req: web::Json<AddLearnerRequest>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<serde_json::Value>, actix_web::Error>
where
    T: EzTypes,
{
    use openraft::BasicNode;

    let req = req.into_inner();
    let node = BasicNode::new(req.addr);
    data.add_learner(req.node_id, node, true)
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("add_learner failed: {}", e)))?;

    Ok(web::Json(serde_json::json!({ "status": "ok" })))
}

/// Change membership API handler
async fn change_membership<T>(
    req: web::Json<ChangeMembershipRequest>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<serde_json::Value>, actix_web::Error>
where
    T: EzTypes,
{
    use std::collections::BTreeMap;

    use openraft::BasicNode;
    use openraft::ChangeMembers;

    let members: BTreeMap<u64, BasicNode> =
        req.into_inner().members.into_iter().map(|(id, addr)| (id, BasicNode::new(addr))).collect();

    let changes = ChangeMembers::AddVoters(members);

    data.change_membership(changes, false)
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("change_membership failed: {}", e)))?;

    Ok(web::Json(serde_json::json!({ "status": "ok" })))
}

/// Metrics API handler
async fn metrics<T>(
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<openraft::RaftMetrics<C<T>>>, actix_web::Error>
where T: EzTypes {
    let metrics = data.metrics().borrow_watched().clone();
    Ok(web::Json(metrics))
}

/// Join cluster API handler
///
/// A new node calls this endpoint to join an existing cluster.
/// The leader assigns a unique node ID based on the log index.
async fn join<T>(
    req: web::Json<JoinRequest>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<JoinResponse>, actix_web::Error>
where
    T: EzTypes,
{
    use openraft::BasicNode;

    let metrics = data.metrics().borrow_watched().clone();

    // Check if we're the leader
    if metrics.current_leader != Some(metrics.id) {
        // Not the leader - find leader address and return it
        let leader_addr = metrics.current_leader.and_then(|leader_id| {
            metrics
                .membership_config
                .membership()
                .get_node(&leader_id)
                .map(|n| n.addr.clone())
        });
        return Ok(web::Json(Err(leader_addr)));
    }

    // We are the leader - write a blank entry to get a unique log index
    let write_result = data
        .write_blank()
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("join write failed: {}", e)))?;

    let node_id = write_result.log_id.index;

    // Add the new node as a learner
    let node = BasicNode::new(req.addr.clone());
    data.add_learner(node_id, node, true)
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("add_learner failed: {}", e)))?;

    Ok(web::Json(Ok(node_id)))
}

/// Initialize request
#[derive(Debug, Deserialize)]
struct InitializeRequest {
    members: Vec<(u64, String)>,
}

/// Add learner request
#[derive(Debug, Deserialize)]
struct AddLearnerRequest {
    node_id: u64,
    addr: String,
}

/// Change membership request
#[derive(Debug, Deserialize)]
struct ChangeMembershipRequest {
    members: Vec<(u64, String)>,
}

/// Join cluster request
#[derive(Debug, Deserialize)]
struct JoinRequest {
    /// New node's HTTP address
    addr: String,
}

/// Join cluster response: Ok(node_id) or Err(leader_addr)
type JoinResponse = Result<u64, Option<String>>;
