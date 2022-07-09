use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use maplit::btreeset;
use openraft::Config;
use openraft::LogIdOptionExt;
use openraft::ServerState;
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;

use crate::fixtures::init_default_ut_tracing;
use crate::fixtures::RaftRouter;

/// Cluster concurrently write new logs and change membership.
#[async_entry::test(worker_threads = 32, init = "init_default_ut_tracing()", tracing_span = "debug")]
async fn concurrent_write_and_add_learner() -> Result<()> {
    // Setup test dependencies.
    let config = Arc::new(Config::default().validate()?);
    let mut router = RaftRouter::new(config.clone());

    let mut log_index = router.new_nodes_from_single(btreeset! {0}, btreeset! {1,2,3}).await?;

    // Concurrently add Learner and write another log.
    tracing::info!("--- concurrently add learner and write another log");
    {
        let r = router.clone();

        let (tx, mut rx) = oneshot::channel::<()>();
        let (tx2, mut rx2) = oneshot::channel::<()>();

        let handle_change = {
            tokio::spawn(
                async move {
                    loop {
                        let recv = rx.try_recv();
                        match recv {
                            Err(TryRecvError::Closed) => {
                                tracing::info!("--- change quit");
                                break;
                            }
                            _ => {}
                        }

                        if let Some(leader_id) = r.leader() {
                            let leader = r.get_raft_handle(&leader_id).unwrap();
                            let res = leader.change_membership(btreeset! {1,2,3}, true, true).await;
                            tracing::info!("change to 123 res: {:?}", res);
                        }

                        if let Some(leader_id) = r.leader() {
                            let leader = r.get_raft_handle(&leader_id).unwrap();
                            let res = leader.change_membership(btreeset! {0}, true, true).await;
                            tracing::info!("change to 0 res: {:?}", res);
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                }
                .instrument(tracing::debug_span!("spawn-change-membership")),
            )
        };

        let r = router.clone();

        let handle_write = {
            tokio::spawn(
                async move {
                    loop {
                        let recv = rx2.try_recv();
                        match recv {
                            Err(TryRecvError::Closed) => {
                                tracing::info!("--- write quit");
                                break;
                            }
                            _ => {}
                        }

                        if let Some(leader_id) = r.leader() {
                            let res = r.client_request_many(leader_id, "client", 10).await;
                            tracing::info!("write res: {:?}", res);
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                }
                .instrument(tracing::debug_span!("spawn-write")),
            )
        };

        tokio::time::sleep(Duration::from_secs(10)).await;

        drop(tx);
        drop(tx2);
    };

    Ok(())
}
