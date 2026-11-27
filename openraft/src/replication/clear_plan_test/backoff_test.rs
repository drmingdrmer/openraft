//! Exercise retry timing at stream setup, before the transport polls any input.

use std::time::Duration;

use rt::WatchReceiver;
use rt::WatchSender;
use tokio::time::Instant;

use super::C;
use super::Harness;
use super::Session;
use super::expect_cleared;
use super::expect_failed;
use super::spawn_replication;
use super::spawn_replication_with_config;
use crate::Config;
use crate::engine::testing::log_id;
use crate::progress::inflight_id::InflightId;
use crate::replication::EXHAUSTED_BACKOFF_DELAY;
use crate::replication::replicate::Replicate;
use crate::type_config::TypeConfigExt;

const TIMER_SLACK: Duration = Duration::from_millis(10);

fn run_paused(body: impl Future<Output = ()>) {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_time().start_paused(true).build().unwrap();
    // `C::spawn()` uses spawn_local with `single-threaded`.
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        tokio::time::timeout(Duration::from_secs(10), body).await.expect("test hung")
    });
}

async fn clear_failed_plan(h: &mut Harness, attempt: u64) {
    let clear = InflightId::new(2 * attempt);
    h.replicate_tx.send(Some(Replicate::new_clear(clear))).unwrap();
    expect_cleared(&mut h.notifications, clear).await;
}

fn publish_plan(h: &Harness, attempt: u64) -> InflightId {
    let plan = InflightId::new(2 * attempt - 1);
    // The fixture returns one entry per read, so the sequential adapter sends two requests.
    h.replicate_tx.send(Some(Replicate::new_logs_since(Some(log_id(1, 1, 18)), plan))).unwrap();
    plan
}

#[test]
fn failed_stream_setup_backs_off_before_retrying() {
    let custom_delay = Duration::from_millis(100);
    let config_delay = Duration::from_millis(150);
    for (backoff, delays) in [
        (Some(vec![custom_delay]), [custom_delay, EXHAUSTED_BACKOFF_DELAY]),
        (None, [config_delay, config_delay]),
    ] {
        run_paused(async move {
            let config = Config {
                backoff: Some("150ms ...150ms".to_string()),
                ..Default::default()
            };
            let session = Session::SetupThenSequential { failures: 2, backoff };
            let (mut h, _channels) = spawn_replication_with_config(true, false, session, config);

            tracing::info!(?delays, "--- fail the initial setup before the transport polls input");
            {
                let start = Instant::now();
                let plan = publish_plan(&h, 1);
                expect_failed(&mut h.notifications, plan).await;
                assert!(start.elapsed() < TIMER_SLACK, "the initial setup is not delayed");
                assert_eq!(*h.reads.borrow_watched(), 0);
            }

            for (retry, delay) in (2..).zip(delays) {
                tracing::info!(
                    retry,
                    ?delay,
                    "--- clear the failed plan and wait for the retry to open"
                );
                {
                    clear_failed_plan(&mut h, retry - 1).await;
                    assert_eq!(*h.opened.borrow_watched(), retry - 1, "clearing does not open a stream");

                    let start = Instant::now();
                    let plan = publish_plan(&h, retry);
                    C::timeout(delay + TIMER_SLACK, h.opened.wait_until_ge(&retry)).await.unwrap().unwrap();
                    assert!(start.elapsed() >= delay, "setup retried before its selected delay");
                    if retry == 2 {
                        expect_failed(&mut h.notifications, plan).await;
                        assert_eq!(*h.reads.borrow_watched(), 0);
                    }
                }
            }

            tracing::info!(
                "--- the sequential adapter sends immediately, and success clears backoff for the next request"
            );
            {
                C::timeout(TIMER_SLACK, h.pulled.wait_until_ge(&2)).await.unwrap().unwrap();
            }

            tracing::info!("--- success also clears the rank, so the next stream opens without a delay");
            {
                publish_plan(&h, 4);
                C::timeout(TIMER_SLACK, h.opened.wait_until_ge(&4)).await.unwrap().unwrap();
            }
        });
    }
}

#[test]
fn stream_setup_backoff_can_be_reset_or_canceled() {
    for reset in [true, false] {
        run_paused(async move {
            let session = Session::SetupThenSequential {
                failures: 1,
                backoff: Some(vec![Duration::from_secs(5)]),
            };
            let (mut h, _channels) = spawn_replication(true, false, session);

            tracing::info!(reset, "--- fail setup and publish a new plan, which waits in backoff");
            {
                let plan = publish_plan(&h, 1);
                expect_failed(&mut h.notifications, plan).await;
                clear_failed_plan(&mut h, 1).await;
                publish_plan(&h, 2);
                let opened = C::timeout(Duration::from_millis(100), h.opened.wait_until_ge(&2)).await;
                assert!(opened.is_err(), "retry must still be waiting in setup backoff");
            }

            tracing::info!(reset, "--- interrupt the wait without letting the delay elapse");
            {
                if reset {
                    h.backoff_reset_tx.send(()).unwrap();
                    C::timeout(TIMER_SLACK, h.pulled.wait_until_ge(&2)).await.unwrap().unwrap();
                    assert_eq!(*h.opened.borrow_watched(), 2);
                } else {
                    drop(h.cancel_tx);
                    let closed = C::timeout(TIMER_SLACK, h.join).await.unwrap().unwrap();
                    assert!(closed.is_err());
                    assert_eq!(*h.opened.borrow_watched(), 1, "no stream opens after cancellation");
                    assert_eq!(*h.reads.borrow_watched(), 0);
                }
            }
        });
    }
}
