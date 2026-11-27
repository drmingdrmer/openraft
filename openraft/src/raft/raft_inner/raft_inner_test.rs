use std::pin::pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Poll;
use std::time::Duration;

use futures_util::FutureExt;
use futures_util::StreamExt;
use futures_util::stream;

use super::RaftInner;
use crate::Config;
use crate::Extensions;
use crate::Raft;
use crate::RaftMetrics;
use crate::StorageError;
use crate::async_runtime::MpscReceiver;
use crate::async_runtime::MpscSender;
use crate::async_runtime::OneshotSender;
use crate::config::RuntimeConfig;
use crate::core::io_flush_tracking::IoProgressWatcher;
use crate::core::raft_msg::RaftMsg;
use crate::core::raft_msg::external_command::ExternalCommand;
use crate::errors::Fatal;
use crate::errors::Infallible;
use crate::errors::ShutdownError;
use crate::impls::Vote;
use crate::metrics::RaftServerMetrics;
use crate::raft::AppendEntriesRequest;
use crate::raft::AppendError;
use crate::raft::core_state::CoreState;
use crate::type_config::TypeConfigExt;
use crate::type_config::alias::JoinHandleOf;

crate::declare_raft_types!(UTConfig: AsyncRuntime = openraft_rt_tokio::TokioRuntime);

fn raft_with_core(core: JoinHandleOf<UTConfig, Result<Infallible, Fatal<UTConfig>>>) -> Raft<UTConfig> {
    let config = Arc::new(Config {
        enable_tick: false,
        enable_heartbeat: false,
        enable_elect: false,
        ..Default::default()
    });
    let (tx_control, _rx_control) = UTConfig::watch_channel(());
    let inner = RaftInner {
        id: 0,
        runtime_config: Arc::new(RuntimeConfig::new(&config)),
        config,
        tx_api: UTConfig::mpsc(1).0,
        rx_lease_read_state: UTConfig::watch_channel(None).1,
        rx_metrics: UTConfig::watch_channel(RaftMetrics::new_initial(0)).1,
        rx_data_metrics: UTConfig::watch_channel(Default::default()).1,
        rx_server_metrics: UTConfig::watch_channel(RaftServerMetrics::new_initial(0)).1,
        progress_watcher: IoProgressWatcher::new().1,
        tx_control: Mutex::new(Some(tx_control)),
        core_state: Mutex::new(CoreState::started(core)),
        extensions: Extensions::default(),
    };
    Raft {
        inner: Arc::new(inner),
        sm_cmd_tx: UTConfig::mpsc(1).0.downgrade(),
        install_snapshot_tx: UTConfig::mpsc(1).0,
    }
}

#[test]
fn try_shutdown_core_panic_after_internal_join() {
    UTConfig::run(async {
        let core = UTConfig::spawn(async { panic!("injected core task panic") });
        let raft = raft_with_core(core);

        tracing::info!("retain the core panic even when a request joins it before shutdown");
        {
            let fatal = raft.inner.get_core_stop_error().await;
            assert_eq!(Fatal::Panicked, fatal);
            let result = UTConfig::timeout(Duration::from_secs(1), raft.try_shutdown()).await.unwrap();
            let expected = Err(ShutdownError);
            assert_eq!(expected, result);
            let repeated = raft.try_shutdown().await;
            assert_eq!(result, repeated);
        }
    });
}

#[test]
fn try_shutdown_aborted_core() {
    UTConfig::run(async {
        tracing::info!("abort the core task so shutdown must retain a join failure");
        {
            let core = UTConfig::spawn(std::future::pending());
            core.abort();
            let raft = raft_with_core(core);
            let result = raft.try_shutdown().await;
            let expected = Err(ShutdownError);
            assert_eq!(expected, result);
            let repeated = raft.try_shutdown().await;
            assert_eq!(result, repeated);
        }
    });
}

#[test]
fn try_shutdown_resumes_cancelled_join() {
    UTConfig::run(async {
        let (core_tx, core_rx) = UTConfig::oneshot();
        let raft = raft_with_core(UTConfig::spawn(async move {
            core_rx.await.unwrap();
            Err(Fatal::Stopped)
        }));

        tracing::info!("drop a shutdown waiting on the core; its join must remain available");
        {
            let pending = raft.try_shutdown().now_or_never();
            assert!(pending.is_none());
            let mut first = Box::pin(raft.inner.join_core_task());
            let pending = first.as_mut().now_or_never();
            assert!(pending.is_none());
            let mut second = Box::pin(raft.inner.join_core_task());
            let pending = second.as_mut().now_or_never();
            assert!(pending.is_none());
            core_tx.send(()).unwrap();
            let (first, second) = UTConfig::timeout(Duration::from_secs(1), async {
                futures_util::future::join(first, second).await
            })
            .await
            .unwrap();
            let expected = (Err(Fatal::Stopped), false);
            assert_eq!(expected, first);
            assert_eq!(expected, second);
        }

        tracing::info!("later shutdown callers share the retained join");
        {
            let (first, second) = UTConfig::timeout(Duration::from_secs(1), async {
                futures_util::future::join(raft.try_shutdown(), raft.try_shutdown()).await
            })
            .await
            .unwrap();
            let expected = Ok(());
            assert_eq!(expected, first);
            assert_eq!(first, second);
        }
    });
}

#[test]
fn shutdown_retry_waits_for_core_after_cancelled_shutdown() {
    UTConfig::run(async {
        let (core_tx, core_rx) = UTConfig::oneshot();
        let raft = raft_with_core(UTConfig::spawn(async move {
            core_rx.await.unwrap();
            Err(Fatal::Stopped)
        }));

        tracing::info!("cancel shutdown while the core is blocked, then retry the public API");
        let retry = {
            let pending = raft.shutdown().now_or_never();
            assert!(pending.is_none());
            let mut retry = Box::pin(raft.shutdown());
            let pending = retry.as_mut().now_or_never();
            assert!(pending.is_none());
            retry
        };

        tracing::info!("release the core so the retried shutdown can finish and retain its result");
        {
            core_tx.send(()).unwrap();
            UTConfig::timeout(Duration::from_secs(1), retry).await.unwrap().unwrap();
            let fatal = raft.inner.get_core_stop_error().await;
            assert_eq!(Fatal::Stopped, fatal);
            let result = raft.try_shutdown().await;
            let expected = Ok(());
            assert_eq!(expected, result);
        }
    });
}

const STREAM_APPEND_TIMEOUT: Duration = Duration::from_secs(1);

#[test]
fn stream_append_rejected_admission_preserves_core_error() {
    UTConfig::run(async {
        let source = UTConfig::err_from_error(&std::io::Error::other("injected log write failure"));
        let storage_error = Fatal::StorageError(StorageError::write_logs(source));
        for fatal in [Fatal::Stopped, Fatal::Panicked, storage_error] {
            tracing::info!(%fatal, "stop the core before submitting the first request");
            let raft = {
                let core_error = fatal.clone();
                let core = UTConfig::spawn(async move {
                    match core_error {
                        Fatal::Panicked => panic!("injected core task panic"),
                        error => Err(error),
                    }
                });
                // The fixture leaves no API receiver, so no request can be admitted.
                let raft = raft_with_core(core);
                let (result, _) = UTConfig::timeout(STREAM_APPEND_TIMEOUT, raft.inner.join_core_task()).await.unwrap();
                assert_eq!(Err(fatal.clone()), result);
                raft
            };

            tracing::info!(%fatal, "receive the exact failure and then EOF");
            {
                let request = AppendEntriesRequest {
                    vote: Vote::new_committed(1, 1),
                    prev_log_id: None,
                    entries: vec![],
                    leader_commit: None,
                };
                let output = raft.stream_append(stream::iter([request]));
                let results: Vec<_> = UTConfig::timeout(STREAM_APPEND_TIMEOUT, output.collect()).await.unwrap();
                assert_eq!(vec![Err(fatal)], results);
            }
        }
    });
}

#[test]
fn stream_append_earlier_failure_wins_over_failed_admission() {
    UTConfig::run(async {
        let rejection = AppendError::HigherVote(Vote::new(2, 2));
        for reply in [Some(Err(rejection)), None] {
            let mut raft = raft_with_core(UTConfig::spawn(async { Err(Fatal::Stopped) }));
            let (tx_api, mut api_rx) = UTConfig::mpsc(1);
            Arc::get_mut(&mut raft.inner).unwrap().tx_api = tx_api;

            let request = AppendEntriesRequest {
                vote: Vote::new_committed(1, 1),
                prev_log_id: None,
                entries: vec![],
                leader_commit: None,
            };
            let (input_tx, input_rx) = UTConfig::mpsc(1);
            let (dropped_tx, dropped_rx) = UTConfig::oneshot::<()>();
            let input = stream::unfold((input_rx, dropped_tx), |(mut rx, dropped_tx)| async move {
                Some((rx.recv().await?, (rx, dropped_tx)))
            });
            let mut output = pin!(raft.stream_append(input));

            tracing::info!(?reply, "admit the first request but hold its responder");
            let responder = {
                input_tx.send(request.clone()).await.unwrap();
                let Some(RaftMsg::AppendEntries { tx, .. }) =
                    UTConfig::timeout(STREAM_APPEND_TIMEOUT, api_rx.recv()).await.unwrap()
                else {
                    panic!("the first append must reach the core");
                };
                tx
            };

            tracing::info!(?reply, "queue the failed admission behind the outstanding response");
            {
                drop(api_rx);
                input_tx.send(request).await.unwrap();
                // Input stays open, so its drop signals that the producer handled the failure.
                assert!(UTConfig::timeout(STREAM_APPEND_TIMEOUT, dropped_rx).await.unwrap().is_err());
                assert_eq!(
                    None,
                    output.next().now_or_never(),
                    "the earlier response is still pending"
                );
            }

            tracing::info!(
                ?reply,
                "terminate on the earlier rejection or lost reply, not the later failure"
            );
            {
                let expected = match reply {
                    Some(reply) => {
                        responder.send(reply.clone()).unwrap();
                        Ok(reply)
                    }
                    None => {
                        drop(responder);
                        Err(Fatal::Stopped)
                    }
                };
                let results: Vec<_> = UTConfig::timeout(STREAM_APPEND_TIMEOUT, output.collect()).await.unwrap();
                assert_eq!(vec![expected], results);
            }
        }
    });
}

#[test]
fn stream_append_drop_releases_producer() {
    UTConfig::run(async {
        for blocked_api in [false, true] {
            tracing::info!(
                blocked_api,
                "drop the response stream while its producer cannot advance"
            );
            let core = UTConfig::spawn(async { Err(Fatal::Stopped) });
            let mut raft = raft_with_core(core);
            let (tx_api, mut rx_api) = UTConfig::mpsc(1);
            let inner = Arc::get_mut(&mut raft.inner).unwrap();
            inner.tx_api = tx_api;

            let vote = Vote::new_committed(1, 1);
            let request = AppendEntriesRequest::<UTConfig> {
                vote,
                prev_log_id: None,
                entries: vec![],
                leader_commit: None,
            };
            let mut request = if blocked_api { Some(request) } else { None };
            if blocked_api {
                let sentinel = RaftMsg::ExternalCommand {
                    cmd: ExternalCommand::Tick,
                };
                raft.inner.tx_api.send(sentinel).await.unwrap();
            }

            let (polled_tx, polled_rx) = UTConfig::oneshot();
            let (drop_tx, drop_rx) = UTConfig::oneshot::<()>();
            let mut polled_tx = Some(polled_tx);
            let input = stream::poll_fn(move |_cx| {
                // Keep the sender in the input until the producer drops it.
                let _keepalive = &drop_tx;
                if let Some(tx) = polled_tx.take() {
                    tx.send(()).unwrap();
                }
                match request.take() {
                    Some(request) => Poll::Ready(Some(request)),
                    None => Poll::Pending,
                }
            });
            let output = raft.stream_append(input);
            UTConfig::timeout(STREAM_APPEND_TIMEOUT, polled_rx).await.unwrap().unwrap();

            tracing::info!(
                blocked_api,
                "release the response stream while preserving API backpressure"
            );
            {
                drop(output);
                drop(raft);
                let dropped = UTConfig::timeout(STREAM_APPEND_TIMEOUT, drop_rx).await.unwrap();
                let input_closed = dropped.is_err();
                assert!(input_closed, "the producer must drop its input");
                if blocked_api {
                    let first = rx_api.recv().await;
                    let sentinel = matches!(
                        first,
                        Some(RaftMsg::ExternalCommand {
                            cmd: ExternalCommand::Tick
                        })
                    );
                    assert!(sentinel, "API capacity must stay occupied until the producer stops");
                }
                let received = UTConfig::timeout(STREAM_APPEND_TIMEOUT, rx_api.recv()).await.unwrap();
                let api_closed = received.is_none();
                assert!(api_closed, "the producer must release its Raft handle");
            }
        }
    });
}
