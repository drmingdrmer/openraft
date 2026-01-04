//! HTTP server for EzRaft
//!
//! This module provides the HTTP server that handles:
//! - Internal Raft RPC (append entries, vote)
//! - Admin API (join, change membership, metrics)

use actix_web::web;
use actix_web::web::Data;
use actix_web::App;
use actix_web::HttpServer;
use openraft::raft;
use openraft::ChangeMembers;
use serde::Deserialize;

use crate::raft::EzRaft;
use crate::trait_::EzStateMachine;
use crate::trait_::EzStorage;
use crate::type_config::EzTypes;
use crate::type_config::OpenRaftTypes;

/// Type alias for OpenRaft types
type C<T> = OpenRaftTypes<T>;

/// Run the HTTP server
pub(crate) async fn run<T, S, M>(raft: EzRaft<T, S, M>) -> std::io::Result<()>
where
    T: EzTypes,
    S: EzStorage<T> + 'static,
    M: EzStateMachine<T> + 'static,
{
    let addr = raft.addr().to_string();

    let raft_data = Data::new(raft);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(raft_data.clone())
            // Raft internal RPC
            .route("/raft/append", web::post().to(append::<T, S, M>))
            .route("/raft/vote", web::post().to(vote::<T, S, M>))
            // Admin API
            .route("/api/join", web::post().to(join::<T, S, M>))
            .route("/api/change_membership", web::post().to(change_membership::<T, S, M>))
            .route("/api/metrics", web::get().to(metrics::<T, S, M>))
    })
    .bind(&addr)?;

    server.run().await
}

/// Raft append entries RPC handler
async fn append<T, S, M>(
    req: web::Json<raft::AppendEntriesRequest<C<T>>>,
    data: Data<EzRaft<T, S, M>>,
) -> Result<web::Json<raft::AppendEntriesResponse<C<T>>>, actix_web::Error>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    let resp = data
        .inner()
        .append_entries(req.into_inner())
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("append_entries failed: {}", e)))?;

    Ok(web::Json(resp))
}

/// Raft vote RPC handler
async fn vote<T, S, M>(
    req: web::Json<raft::VoteRequest<C<T>>>,
    data: Data<EzRaft<T, S, M>>,
) -> Result<web::Json<raft::VoteResponse<C<T>>>, actix_web::Error>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    let resp = data
        .inner()
        .vote(req.into_inner())
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("vote failed: {}", e)))?;

    Ok(web::Json(resp))
}

/// Change membership API handler
async fn change_membership<T, S, M>(
    req: web::Json<ChangeMembers<C<T>>>,
    data: Data<EzRaft<T, S, M>>,
) -> Result<web::Json<serde_json::Value>, actix_web::Error>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    data.change_membership(req.into_inner())
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("change_membership failed: {}", e)))?;

    Ok(web::Json(serde_json::json!({ "status": "ok" })))
}

/// Metrics API handler
async fn metrics<T, S, M>(
    data: Data<EzRaft<T, S, M>>,
) -> Result<web::Json<openraft::RaftMetrics<C<T>>>, actix_web::Error>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    let metrics = data.metrics().await;
    Ok(web::Json(metrics))
}

/// Join cluster API handler
///
/// A new node calls this endpoint to join an existing cluster.
/// The leader assigns a unique node ID based on the log index.
async fn join<T, S, M>(
    req: web::Json<JoinRequest>,
    data: Data<EzRaft<T, S, M>>,
) -> Result<web::Json<JoinResponse>, actix_web::Error>
where
    T: EzTypes,
    S: EzStorage<T>,
    M: EzStateMachine<T>,
{
    let metrics = data.metrics().await;

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
        .inner()
        .write_blank()
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("join write failed: {}", e)))?;

    let node_id = write_result.log_id.index;

    // Add the new node as a learner
    data.add_learner(node_id, req.addr.clone())
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(format!("add_learner failed: {}", e)))?;

    Ok(web::Json(Ok(node_id)))
}

/// Join cluster request
#[derive(Debug, Deserialize)]
struct JoinRequest {
    /// New node's HTTP address
    addr: String,
}

/// Join cluster response: Ok(node_id) or Err(leader_addr)
type JoinResponse = Result<u64, Option<String>>;
