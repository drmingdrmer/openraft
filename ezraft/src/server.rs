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
use crate::trait_::EzStorage;
use crate::type_config::EzTypes;
use crate::type_config::OpenRaftTypes;

/// Type alias for OpenRaft types
type C<T> = OpenRaftTypes<T>;

/// HTTP server wrapper for EzRaft
pub struct EzServer<T, S>
where
    T: EzTypes,
    S: EzStorage<T>,
{
    raft: EzRaft<T, S>,
}

impl<T, S> EzServer<T, S>
where
    T: EzTypes,
    S: EzStorage<T> + 'static,
{
    pub fn new(raft: EzRaft<T, S>) -> Self {
        Self { raft }
    }

    /// Run the HTTP server
    pub async fn run(self) -> std::io::Result<()> {
        let addr = self.raft.addr().to_string();
        let server_data = Data::new(self);

        let server = HttpServer::new(move || {
            App::new()
                .app_data(server_data.clone())
                // Raft internal RPC
                .route("/raft/append", web::post().to(Self::handle_append))
                .route("/raft/vote", web::post().to(Self::handle_vote))
                // Admin API
                .route("/api/join", web::post().to(Self::handle_join))
                .route("/api/change_membership", web::post().to(Self::handle_change_membership))
                .route("/api/metrics", web::get().to(Self::handle_metrics))
        })
        .bind(&addr)?;

        server.run().await
    }

    /// Raft append entries RPC handler
    async fn handle_append(
        req: web::Json<raft::AppendEntriesRequest<C<T>>>,
        ez: Data<Self>,
    ) -> Result<web::Json<raft::AppendEntriesResponse<C<T>>>, actix_web::Error> {
        let resp = ez
            .raft
            .inner()
            .append_entries(req.into_inner())
            .await
            .map_err(|e| actix_web::error::ErrorInternalServerError(format!("append_entries failed: {}", e)))?;

        Ok(web::Json(resp))
    }

    /// Raft vote RPC handler
    async fn handle_vote(
        req: web::Json<raft::VoteRequest<C<T>>>,
        ez: Data<Self>,
    ) -> Result<web::Json<raft::VoteResponse<C<T>>>, actix_web::Error> {
        let resp = ez
            .raft
            .inner()
            .vote(req.into_inner())
            .await
            .map_err(|e| actix_web::error::ErrorInternalServerError(format!("vote failed: {}", e)))?;

        Ok(web::Json(resp))
    }

    /// Change membership API handler
    async fn handle_change_membership(
        req: web::Json<ChangeMembers<C<T>>>,
        ez: Data<Self>,
    ) -> Result<web::Json<serde_json::Value>, actix_web::Error> {
        ez.raft
            .change_membership(req.into_inner())
            .await
            .map_err(|e| actix_web::error::ErrorInternalServerError(format!("change_membership failed: {}", e)))?;

        Ok(web::Json(serde_json::json!({ "status": "ok" })))
    }

    /// Metrics API handler
    async fn handle_metrics(ez: Data<Self>) -> Result<web::Json<openraft::RaftMetrics<C<T>>>, actix_web::Error> {
        let metrics = ez.raft.metrics().await;
        Ok(web::Json(metrics))
    }

    /// Join cluster API handler
    ///
    /// A new node calls this endpoint to join an existing cluster.
    /// The leader assigns a unique node ID based on the log index.
    async fn handle_join(
        req: web::Json<JoinRequest>,
        ez: Data<Self>,
    ) -> Result<web::Json<JoinResponse>, actix_web::Error> {
        let metrics = ez.raft.metrics().await;

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
        let write_result = ez
            .raft
            .inner()
            .write_blank()
            .await
            .map_err(|e| actix_web::error::ErrorInternalServerError(format!("join write failed: {}", e)))?;

        let node_id = write_result.log_id.index;

        // Add the new node as a learner
        ez.raft
            .add_learner(node_id, req.addr.clone())
            .await
            .map_err(|e| actix_web::error::ErrorInternalServerError(format!("add_learner failed: {}", e)))?;

        Ok(web::Json(Ok(node_id)))
    }
}

/// Run the HTTP server (convenience function)
pub(crate) async fn run<T, S>(raft: EzRaft<T, S>) -> std::io::Result<()>
where
    T: EzTypes,
    S: EzStorage<T> + 'static,
{
    EzServer::new(raft).run().await
}

/// Join cluster request
#[derive(Debug, Deserialize)]
struct JoinRequest {
    /// New node's HTTP address
    addr: String,
}

/// Join cluster response: Ok(node_id) or Err(leader_addr)
type JoinResponse = Result<u64, Option<String>>;
