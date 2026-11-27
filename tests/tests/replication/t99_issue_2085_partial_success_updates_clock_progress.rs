use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use anyhow::Result;
use openraft::Config;
use openraft::errors::RPCError;
use openraft::errors::Unreachable;
use openraft::type_config::OneshotSender;
use openraft::type_config::TypeConfigExt;
use openraft_memstore::TypeConfig;

use crate::fixtures::RaftRouter;
use crate::fixtures::WAIT_TIMEOUT;
use crate::fixtures::log_id;
use crate::fixtures::rpc_kind::RpcKind;
use crate::fixtures::rpc_request::RpcRequest;
use crate::fixtures::rpc_response::RpcResponse;
use crate::fixtures::ut_harness;

/// A partial acknowledgement `Ok(matching)` with `prev < matching < last` refreshes the follower's
/// clock progress even though it did not accept the complete request.
#[tracing::instrument]
#[test_harness::test(harness = ut_harness)]
async fn partial_success_updates_clock_progress() -> Result<()> {
    check_partial_success_clock_progress(1).await
}

/// A fresh stream's partial acknowledgement at `prev` refreshes the clock without adding entries.
#[tracing::instrument]
#[test_harness::test(harness = ut_harness)]
async fn partial_success_at_prev_updates_clock_progress() -> Result<()> {
    check_partial_success_clock_progress(0).await
}

async fn check_partial_success_clock_progress(quota: u64) -> Result<()> {
    let config = Config {
        enable_tick: false,
        enable_heartbeat: false,
        enable_elect: false,
        backoff: Some("30ms ..100ms".to_string()),
        ..Default::default()
    };

    let mut router = RaftRouter::new(config)?;

    tracing::info!("--- initialize a 3-voter cluster");
    let log_index = router.new_cluster([0, 1, 2], []).await?;
    let prev = Some(log_id(1, 0, log_index));
    let matching = Some(log_id(1, 0, log_index + quota));

    tracing::info!(log_index, "--- isolate voter 1 and append two logs through voter 2");
    let replication_started = {
        router.set_unreachable(1, true);
        let started = TypeConfig::now();
        router.client_request_many(0, "partial", 2).await?;
        started
    };

    tracing::info!(
        ?prev,
        quota,
        "--- allow one truncated request in a fresh stream to voter 1"
    );
    let (partial_tx, partial_rx) = TypeConfig::oneshot();
    {
        router.set_append_entries_quota(Some(quota));

        // Fail the first eligible request so no earlier response in its stream can supply a timestamp.
        let discard_first_batch = AtomicBool::new(true);
        router
            .set_rpc_pre_hook(RpcKind::AppendEntries, move |call| {
                let blocked = call.to == 1
                    && (!matches!(&call.request, RpcRequest::AppendEntries(req) if req.entries.len() == 2)
                        || discard_first_batch.swap(false, Ordering::Relaxed));
                let result = if blocked {
                    Err(RPCError::Unreachable(Unreachable::<TypeConfig>::from_string(
                        "wait for a fresh stream with two entries",
                    )))
                } else {
                    Ok(())
                };
                futures::future::ready(result)
            })
            .await;

        let partial_tx = Mutex::new(Some(partial_tx));
        router
            .set_rpc_post_hook(RpcKind::AppendEntries, move |call| {
                if call.to == 1 {
                    let RpcRequest::AppendEntries(request) = call.request else {
                        panic!("expected an AppendEntries request");
                    };
                    let RpcResponse::AppendEntries(response) = call.response else {
                        panic!("expected an AppendEntries response");
                    };
                    assert_eq!(request.prev_log_id, prev);
                    assert_eq!(request.entries.len() as u64, quota);
                    assert_eq!(response, Ok(matching));

                    call.router.set_unreachable(call.to, true);
                    partial_tx.lock().unwrap().take().expect("only one partial response").send(()).ok();
                }

                futures::future::ready(Ok(()))
            })
            .await;

        router.set_unreachable(1, false);
        TypeConfig::timeout(WAIT_TIMEOUT, partial_rx).await??;
    }

    tracing::info!(
        ?matching,
        "--- only the partial acknowledgement can refresh voter 1's clock"
    );
    {
        router
            .wait(&0)
            .metrics(
                |metrics| {
                    metrics
                        .heartbeat
                        .as_ref()
                        .and_then(|clock| clock.get(&1))
                        .copied()
                        .flatten()
                        .is_some_and(|acked| acked.into_inner() > replication_started)
                        && metrics.replication.as_ref().and_then(|progress| progress.get(&1)).copied() == Some(matching)
                },
                "partial acknowledgement refreshes clock progress and matches only the accepted prefix",
            )
            .await?;
    }

    Ok(())
}
