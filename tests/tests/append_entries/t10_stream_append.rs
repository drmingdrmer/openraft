use std::pin::pin;

use anyhow::Result;
use futures::StreamExt;
use openraft::Config;
use openraft::Vote;
use openraft::async_runtime::MpscReceiver;
use openraft::async_runtime::MpscSender;
use openraft::errors::Fatal;
use openraft::raft::AppendEntriesRequest;
use openraft::raft::AppendError;
use openraft::raft::ConflictHint;
use openraft::raft::VoteRequest;
use openraft::testing::blank_ent;
use openraft::type_config::TypeConfigExt;
use openraft_memstore::TypeConfig;

use crate::fixtures::RaftRouter;
use crate::fixtures::WAIT_TIMEOUT;
use crate::fixtures::log_id;
use crate::fixtures::ut_harness;

/// Test stream_append with successful append entries.
#[tracing::instrument]
#[test_harness::test(harness = ut_harness)]
async fn stream_append_success() -> Result<()> {
    let config = Config {
        enable_heartbeat: false,
        ..Default::default()
    };

    let mut router = RaftRouter::new(config)?;
    router.new_raft_node(0).await?;

    let raft = router.get_raft_handle(&0)?;

    let requests = vec![
        AppendEntriesRequest::<openraft_memstore::TypeConfig> {
            vote: Vote::new_committed(1, 1),
            prev_log_id: None,
            entries: vec![
                blank_ent::<openraft_memstore::TypeConfig>(0, 0, 0),
                blank_ent::<openraft_memstore::TypeConfig>(1, 1, 1),
            ],
            leader_commit: None,
        },
        AppendEntriesRequest::<openraft_memstore::TypeConfig> {
            vote: Vote::new_committed(1, 1),
            prev_log_id: Some(log_id(1, 1, 1)),
            entries: vec![
                blank_ent::<openraft_memstore::TypeConfig>(1, 1, 2),
                blank_ent::<openraft_memstore::TypeConfig>(1, 1, 3),
            ],
            leader_commit: None,
        },
        AppendEntriesRequest::<openraft_memstore::TypeConfig> {
            vote: Vote::new_committed(1, 1),
            prev_log_id: Some(log_id(1, 1, 3)),
            entries: vec![blank_ent::<openraft_memstore::TypeConfig>(1, 1, 4)],
            leader_commit: Some(log_id(1, 1, 4)),
        },
    ];

    let input_stream = futures::stream::iter(requests);
    let output_stream = pin!(raft.stream_append(input_stream));

    let results: Vec<_> = output_stream.collect().await;
    assert_eq!(results, vec![
        Ok(Ok(Some(log_id(1, 1, 1)))),
        Ok(Ok(Some(log_id(1, 1, 3)))),
        Ok(Ok(Some(log_id(1, 1, 4)))),
    ]);

    Ok(())
}

/// Report a failed submission after shutdown even while input remains open.
#[tracing::instrument]
#[test_harness::test(harness = ut_harness)]
async fn stream_append_shutdown_after_success() -> Result<()> {
    let config = Config {
        enable_tick: false,
        enable_heartbeat: false,
        enable_elect: false,
        ..Default::default()
    };
    let mut router = RaftRouter::new(config)?;
    router.new_raft_node(0).await?;
    let raft = router.get_raft_handle(&0)?;
    let (input_tx, input_rx) = TypeConfig::mpsc(1);
    let input = futures::stream::unfold(input_rx, |mut rx| async move { Some((rx.recv().await?, rx)) });
    let mut output = pin!(raft.stream_append(input));
    let last_log = log_id(1, 1, 1);

    tracing::info!(%last_log, "receive a successful append before shutting down node 0");
    {
        let request = AppendEntriesRequest {
            vote: Vote::new_committed(1, 1),
            prev_log_id: None,
            entries: vec![blank_ent::<TypeConfig>(0, 0, 0), blank_ent::<TypeConfig>(1, 1, 1)],
            leader_commit: None,
        };
        input_tx.send(request).await?;
        let response = TypeConfig::timeout(WAIT_TIMEOUT, output.next()).await?;
        assert_eq!(Some(Ok(Ok(Some(last_log)))), response);
    }

    tracing::info!(%last_log, "submit the next request only after node 0 has shut down");
    {
        TypeConfig::timeout(WAIT_TIMEOUT, raft.try_shutdown()).await??;
        let request = AppendEntriesRequest {
            vote: Vote::new_committed(1, 1),
            prev_log_id: Some(last_log),
            entries: vec![],
            leader_commit: None,
        };
        input_tx.send(request).await?;
        let results: Vec<_> = TypeConfig::timeout(WAIT_TIMEOUT, output.collect()).await?;
        assert_eq!(vec![Err(Fatal::Stopped)], results);
        drop(input_tx);
    }

    tracing::info!("empty input must end normally even on a stopped node");
    {
        let mut output = pin!(raft.stream_append(futures::stream::empty()));
        let response = TypeConfig::timeout(WAIT_TIMEOUT, output.next()).await?;
        assert_eq!(None, response);
    }

    Ok(())
}

/// Test stream_append terminates on conflict.
#[tracing::instrument]
#[test_harness::test(harness = ut_harness)]
async fn stream_append_conflict() -> Result<()> {
    let config = Config {
        enable_heartbeat: false,
        ..Default::default()
    };

    let mut router = RaftRouter::new(config)?;
    router.new_raft_node(0).await?;

    let raft = router.get_raft_handle(&0)?;

    // First request succeeds, second has conflicting prev_log_id
    let requests = vec![
        AppendEntriesRequest::<openraft_memstore::TypeConfig> {
            vote: Vote::new_committed(1, 1),
            prev_log_id: None,
            entries: vec![
                blank_ent::<openraft_memstore::TypeConfig>(0, 0, 0),
                blank_ent::<openraft_memstore::TypeConfig>(1, 1, 1),
            ],
            leader_commit: None,
        },
        // This will conflict: prev_log_id at index 5 doesn't exist
        AppendEntriesRequest::<openraft_memstore::TypeConfig> {
            vote: Vote::new_committed(1, 1),
            prev_log_id: Some(log_id(1, 1, 5)),
            entries: vec![blank_ent::<openraft_memstore::TypeConfig>(1, 1, 6)],
            leader_commit: None,
        },
        // This should never be processed because stream terminates on conflict
        AppendEntriesRequest::<openraft_memstore::TypeConfig> {
            vote: Vote::new_committed(1, 1),
            prev_log_id: Some(log_id(1, 1, 6)),
            entries: vec![blank_ent::<openraft_memstore::TypeConfig>(1, 1, 7)],
            leader_commit: None,
        },
    ];

    let input_stream = futures::stream::iter(requests);
    let output_stream = pin!(raft.stream_append(input_stream));

    let results: Vec<_> = output_stream.collect().await;
    // The hint describes the follower's whole log: it has no entry at the conflicting index.
    let hint = ConflictHint::new(vec![log_id(0, 0, 0), log_id(1, 1, 1)]);
    assert_eq!(results, vec![
        Ok(Ok(Some(log_id(1, 1, 1)))),
        Ok(Err(AppendError::Conflict {
            log_id: log_id(1, 1, 5),
            hint: Some(hint),
        })),
    ]);

    Ok(())
}

/// Test stream_append terminates on higher vote.
#[tracing::instrument]
#[test_harness::test(harness = ut_harness)]
async fn stream_append_higher_vote() -> Result<()> {
    let config = Config {
        enable_heartbeat: false,
        ..Default::default()
    };

    let mut router = RaftRouter::new(config)?;
    router.new_raft_node(0).await?;

    let raft = router.get_raft_handle(&0)?;

    // Establish a higher vote on the node
    let resp = raft
        .vote(VoteRequest {
            vote: Vote::new(10, 2),
            last_log_id: Some(log_id(10, 2, 100)),
            leadership_transfer_from: None,
        })
        .await?;
    assert!(resp.is_granted_to(&Vote::new(10, 2)));

    // Now try to append with a lower vote
    let requests = vec![
        AppendEntriesRequest::<openraft_memstore::TypeConfig> {
            vote: Vote::new_committed(1, 1), // Lower than (10, 2)
            prev_log_id: None,
            entries: vec![blank_ent::<openraft_memstore::TypeConfig>(0, 0, 0)],
            leader_commit: None,
        },
        // This should never be processed
        AppendEntriesRequest::<openraft_memstore::TypeConfig> {
            vote: Vote::new_committed(1, 1),
            prev_log_id: Some(log_id(0, 0, 0)),
            entries: vec![blank_ent::<openraft_memstore::TypeConfig>(1, 1, 1)],
            leader_commit: None,
        },
    ];

    let input_stream = futures::stream::iter(requests);
    let output_stream = pin!(raft.stream_append(input_stream));

    let results: Vec<_> = output_stream.collect().await;
    assert_eq!(results, vec![Ok(Err(AppendError::HigherVote(Vote::new(10, 2)))),]);

    Ok(())
}
