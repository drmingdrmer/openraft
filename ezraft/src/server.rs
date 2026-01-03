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
            .route("/raft/append", web::post().to(append::<T, S, M>))
            .route("/raft/vote", web::post().to(vote::<T, S, M>))
            // Admin API
            .route("/api/init", web::post().to(initialize::<T, S, M>))
            .route("/api/add_learner", web::post().to(add_learner::<T, S, M>))
            .route("/api/change_membership", web::post().to(change_membership::<T, S, M>))
            .route("/api/metrics", web::get().to(metrics::<T, S, M>))
    })
    .bind(&addr)?;

    server.run().await
}

/// Raft append entries RPC handler
async fn append<T, S, M>(
    req: web::Json<raft::AppendEntriesRequest<C<T>>>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<raft::AppendEntriesResponse<C<T>>>, actix_web::Error>
where
    T: EzTypes,
    S: crate::trait_::EzStorage<T>,
    M: crate::trait_::EzStateMachine<T>,
{
    let resp = data
        .append_entries(req.into_inner())
        .await
        .map_err(|e| {
            actix_web::error::ErrorInternalServerError(format!("append_entries failed: {}", e))
        })?;

    Ok(web::Json(resp))
}

/// Raft vote RPC handler
async fn vote<T, S, M>(
    req: web::Json<raft::VoteRequest<C<T>>>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<raft::VoteResponse<C<T>>>, actix_web::Error>
where
    T: EzTypes,
    S: crate::trait_::EzStorage<T>,
    M: crate::trait_::EzStateMachine<T>,
{
    let resp = data
        .vote(req.into_inner())
        .await
        .map_err(|e| {
            actix_web::error::ErrorInternalServerError(format!("vote failed: {}", e))
        })?;

    Ok(web::Json(resp))
}

/// Initialize cluster API handler
async fn initialize<T, S, M>(
    req: web::Json<InitializeRequest>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<serde_json::Value>, actix_web::Error>
where
    T: EzTypes,
    S: crate::trait_::EzStorage<T>,
    M: crate::trait_::EzStateMachine<T>,
{
    use openraft::BasicNode;
    use std::collections::BTreeMap;

    let members: BTreeMap<u64, BasicNode> = req
        .into_inner()
        .members
        .into_iter()
        .map(|(id, addr)| (id, BasicNode::new(addr)))
        .collect();

    data.initialize(members)
        .await
        .map_err(|e| {
            actix_web::error::ErrorInternalServerError(format!("initialize failed: {}", e))
        })?;

    Ok(web::Json(serde_json::json!({ "status": "ok" })))
}

/// Add learner API handler
async fn add_learner<T, S, M>(
    req: web::Json<AddLearnerRequest>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<serde_json::Value>, actix_web::Error>
where
    T: EzTypes,
    S: crate::trait_::EzStorage<T>,
    M: crate::trait_::EzStateMachine<T>,
{
    use openraft::BasicNode;

    let req = req.into_inner();
    let node = BasicNode::new(req.addr);
    data.add_learner(req.node_id, node, true)
        .await
        .map_err(|e| {
            actix_web::error::ErrorInternalServerError(format!("add_learner failed: {}", e))
        })?;

    Ok(web::Json(serde_json::json!({ "status": "ok" })))
}

/// Change membership API handler
async fn change_membership<T, S, M>(
    req: web::Json<ChangeMembershipRequest>,
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<serde_json::Value>, actix_web::Error>
where
    T: EzTypes,
    S: crate::trait_::EzStorage<T>,
    M: crate::trait_::EzStateMachine<T>,
{
    use openraft::BasicNode;
    use openraft::ChangeMembers;
    use std::collections::BTreeMap;

    let members: BTreeMap<u64, BasicNode> = req
        .into_inner()
        .members
        .into_iter()
        .map(|(id, addr)| (id, BasicNode::new(addr)))
        .collect();

    let changes = ChangeMembers::AddVoters(members);

    data.change_membership(changes, false)
        .await
        .map_err(|e| {
            actix_web::error::ErrorInternalServerError(format!(
                "change_membership failed: {}",
                e
            ))
        })?;

    Ok(web::Json(serde_json::json!({ "status": "ok" })))
}

/// Metrics API handler
async fn metrics<T, S, M>(
    data: Data<Arc<openraft::Raft<C<T>>>>,
) -> Result<web::Json<openraft::RaftMetrics<C<T>>>, actix_web::Error>
where
    T: EzTypes,
    S: crate::trait_::EzStorage<T>,
    M: crate::trait_::EzStateMachine<T>,
{
    let metrics = data.metrics().borrow_watched().clone();
    Ok(web::Json(metrics))
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

