use rt::Instant;

use super::*;
use crate::async_runtime::MpscSender;
use crate::batch::Batch;
use crate::engine::testing::UTConfig;
use crate::engine::testing::log_id;
use crate::entry::EntryPayload;
use crate::impls::ProgressResponder;
use crate::raft::WriteOption;
use crate::raft::responder::Responder;
use crate::raft::responder::core_responder::CoreResponder;
use crate::type_config::TypeConfigExt;
use crate::type_config::alias::BatchOf;
use crate::type_config::alias::CommittedLeaderIdOf;
use crate::type_config::alias::PayloadOf;

type C = UTConfig<()>;

fn committed_leader_id(term: u64, node_id: u64) -> CommittedLeaderIdOf<C> {
    *log_id(term, node_id, 0).committed_leader_id()
}

fn default_msg_receiver(receiver: MpscReceiverOf<C, RaftMsg<C>>) -> BatchRaftMsgReceiver<C> {
    BatchRaftMsgReceiver::new(receiver, 4096, Duration::ZERO)
}

fn leader_option(leader: Option<CommittedLeaderIdOf<C>>) -> WriteOption<C> {
    WriteOption {
        expected_leader: leader,
        expected_last_log_id: None,
    }
}

fn client_write(data: u64, leader: Option<CommittedLeaderIdOf<C>>) -> RaftMsg<C> {
    client_write_with_option(data, leader_option(leader))
}

fn client_write_with_option(data: u64, write_option: WriteOption<C>) -> RaftMsg<C> {
    client_write_slots(vec![(data, None)], write_option)
}

/// Builds a `ClientWrite` with one payload per slot, and the slot's responder, if any, at the
/// payload's position.
fn client_write_slots(slots: Vec<(u64, Option<CoreResponder<C>>)>, write_option: WriteOption<C>) -> RaftMsg<C> {
    let (data, responders): (Vec<_>, Vec<_>) = slots.into_iter().unzip();
    RaftMsg::ClientWrite {
        payloads: Batch::of(data.into_iter().map(EntryPayload::Normal)),
        responders: Batch::of(responders),
        write_option,
        #[cfg(feature = "runtime-stats")]
        proposed_at: C::now(),
    }
}

fn extract_payload_data(payloads: &BatchOf<C, PayloadOf<C>>) -> Vec<u64> {
    payloads
        .as_ref()
        .iter()
        .map(|p| match p {
            EntryPayload::Normal(d) => *d,
            _ => panic!("expected Normal payload"),
        })
        .collect()
}

#[test]
fn test_merge_consecutive_client_writes_with_same_leader() {
    C::run(async {
        let (tx, rx) = C::mpsc(100);
        let mut receiver: BatchRaftMsgReceiver<C> = default_msg_receiver(rx);

        let leader = Some(committed_leader_id(1, 1));
        tx.send(client_write(1, leader)).await.unwrap();
        tx.send(client_write(2, leader)).await.unwrap();
        tx.send(client_write(3, leader)).await.unwrap();

        receiver.ensure_buffered().await.unwrap();
        let msg = receiver.try_recv().await.unwrap().unwrap();

        let RaftMsg::ClientWrite {
            payloads,
            responders,
            write_option,
            ..
        } = msg
        else {
            panic!("expected ClientWrite");
        };
        assert_eq!(extract_payload_data(&payloads), vec![1, 2, 3]);
        assert_eq!(responders.len(), payloads.len());
        assert_eq!(write_option, leader_option(leader));
    });
}

/// Merging writes that carry several payloads keeps every responder slot, the empty ones
/// included, at the position of its own payload, so a log id sent through a slot reaches the
/// caller of that payload.
#[test]
fn test_merge_keeps_each_responder_with_its_payload() {
    C::run(async {
        let (tx, rx) = C::mpsc(100);
        let mut receiver: BatchRaftMsgReceiver<C> = default_msg_receiver(rx);

        let (responder10, commit10, _) = ProgressResponder::new();
        let (responder20, commit20, _) = ProgressResponder::new();
        let (responder31, commit31, _) = ProgressResponder::new();
        let (responder32, commit32, _) = ProgressResponder::new();

        let leader = Some(committed_leader_id(1, 1));
        let first = client_write_slots(
            vec![(10, Some(CoreResponder::Progress(responder10))), (11, None)],
            leader_option(leader),
        );
        let second = client_write_slots(
            vec![(20, Some(CoreResponder::Progress(responder20)))],
            leader_option(leader),
        );
        let third = client_write_slots(
            vec![
                (30, None),
                (31, Some(CoreResponder::Progress(responder31))),
                (32, Some(CoreResponder::Progress(responder32))),
            ],
            leader_option(leader),
        );
        tx.send(first).await.unwrap();
        tx.send(second).await.unwrap();
        tx.send(third).await.unwrap();

        receiver.ensure_buffered().await.unwrap();
        let msg = receiver.try_recv().await.unwrap().unwrap();

        let RaftMsg::ClientWrite {
            payloads,
            responders,
            write_option,
            ..
        } = msg
        else {
            panic!("expected ClientWrite");
        };
        let data = extract_payload_data(&payloads);
        let present: Vec<bool> = responders.as_ref().iter().map(Option::is_some).collect();
        assert_eq!(data, vec![10, 11, 20, 30, 31, 32]);
        assert_eq!(present, vec![true, false, true, false, true, true]);
        assert_eq!(write_option, leader_option(leader));

        for (index, responder) in data.into_iter().zip(responders) {
            if let Some(mut responder) = responder {
                responder.on_commit(log_id(1, 1, index));
            }
        }

        let committed = [
            commit10.await.unwrap(),
            commit20.await.unwrap(),
            commit31.await.unwrap(),
            commit32.await.unwrap(),
        ];
        let want = [10, 20, 31, 32].map(|index| log_id(1, 1, index));
        assert_eq!(committed, want);

        let rest = receiver.try_recv().await.unwrap();
        assert!(rest.is_none(), "the three writes are merged into one batch");
    });
}

#[test]
fn test_no_merge_when_expected_leader_differs() {
    C::run(async {
        let (tx, rx) = C::mpsc(100);
        let mut receiver: BatchRaftMsgReceiver<C> = default_msg_receiver(rx);

        let leader1 = Some(committed_leader_id(1, 1));
        let leader2 = Some(committed_leader_id(2, 1));
        tx.send(client_write(1, leader1)).await.unwrap();
        tx.send(client_write(2, leader2)).await.unwrap();

        receiver.ensure_buffered().await.unwrap();

        // First message should not be merged with second
        let msg1 = receiver.try_recv().await.unwrap().unwrap();
        let RaftMsg::ClientWrite {
            payloads,
            responders,
            write_option,
            ..
        } = msg1
        else {
            panic!("expected ClientWrite");
        };
        assert_eq!(extract_payload_data(&payloads), vec![1]);
        assert_eq!(responders.len(), payloads.len());
        assert_eq!(write_option, leader_option(leader1));

        // Second message should be buffered and returned separately
        let msg2 = receiver.try_recv().await.unwrap().unwrap();
        let RaftMsg::ClientWrite {
            payloads,
            responders,
            write_option,
            ..
        } = msg2
        else {
            panic!("expected ClientWrite");
        };
        assert_eq!(extract_payload_data(&payloads), vec![2]);
        assert_eq!(responders.len(), payloads.len());
        assert_eq!(write_option, leader_option(leader2));
    });
}

/// A write that expects a last log id is received alone, even next to a write that expects the
/// same last log id or the entry right after it, while the plain writes after it are still
/// merged with each other.
#[test]
fn test_no_merge_with_expected_last_log_id() {
    C::run(async {
        let (tx, rx) = C::mpsc(100);
        let mut receiver: BatchRaftMsgReceiver<C> = default_msg_receiver(rx);

        let expect2 = WriteOption::<C>::expect_last_log_id(log_id(1, 1, 2));
        let expect3 = WriteOption::<C>::expect_last_log_id(log_id(1, 1, 3));
        tx.send(client_write(1, None)).await.unwrap();
        tx.send(client_write_with_option(2, expect2.clone())).await.unwrap();
        tx.send(client_write_with_option(3, expect2.clone())).await.unwrap();
        tx.send(client_write_with_option(4, expect3.clone())).await.unwrap();
        tx.send(client_write(5, None)).await.unwrap();
        tx.send(client_write(6, None)).await.unwrap();

        receiver.ensure_buffered().await.unwrap();

        let want = [
            (vec![1], WriteOption::default()),
            (vec![2], expect2.clone()),
            (vec![3], expect2),
            (vec![4], expect3),
            (vec![5, 6], WriteOption::default()),
        ];
        for (want_data, want_option) in want {
            let msg = receiver.try_recv().await.unwrap().unwrap();
            let RaftMsg::ClientWrite {
                payloads,
                responders,
                write_option,
                ..
            } = msg
            else {
                panic!("expected ClientWrite");
            };
            assert_eq!(extract_payload_data(&payloads), want_data);
            assert_eq!(responders.len(), payloads.len());
            assert_eq!(write_option, want_option);
        }
    });
}

#[test]
fn test_non_client_write_stops_merging() {
    C::run(async {
        let (tx, rx) = C::mpsc(100);
        let mut receiver: BatchRaftMsgReceiver<C> = default_msg_receiver(rx);

        let leader = Some(committed_leader_id(1, 1));
        tx.send(client_write(1, leader)).await.unwrap();
        tx.send(RaftMsg::WithRaftState { req: Box::new(|_| {}) }).await.unwrap();
        tx.send(client_write(2, leader)).await.unwrap();

        receiver.ensure_buffered().await.unwrap();

        // First ClientWrite should not merge past the WithRaftState
        let msg1 = receiver.try_recv().await.unwrap().unwrap();
        let RaftMsg::ClientWrite {
            payloads, responders, ..
        } = msg1
        else {
            panic!("expected ClientWrite");
        };
        assert_eq!(extract_payload_data(&payloads), vec![1]);
        assert_eq!(responders.len(), payloads.len());

        // WithRaftState should be returned next
        let msg2 = receiver.try_recv().await.unwrap().unwrap();
        assert!(matches!(msg2, RaftMsg::WithRaftState { .. }));

        // Last ClientWrite should be returned
        let msg3 = receiver.try_recv().await.unwrap().unwrap();
        let RaftMsg::ClientWrite {
            payloads, responders, ..
        } = msg3
        else {
            panic!("expected ClientWrite");
        };
        assert_eq!(extract_payload_data(&payloads), vec![2]);
        assert_eq!(responders.len(), payloads.len());
    });
}

#[test]
fn test_try_recv_returns_none_when_empty() {
    C::run(async {
        let (_tx, rx) = C::mpsc::<RaftMsg<C>>(100);
        let mut receiver: BatchRaftMsgReceiver<C> = default_msg_receiver(rx);

        let result = receiver.try_recv().await.unwrap();
        assert!(result.is_none());
    });
}

#[test]
fn test_ensure_buffered_waits_for_message() {
    C::run(async {
        let (tx, rx) = C::mpsc(100);
        let mut receiver: BatchRaftMsgReceiver<C> = default_msg_receiver(rx);

        tx.send(client_write(42, None)).await.unwrap();

        receiver.ensure_buffered().await.unwrap();

        // Message should be in buffer, try_recv should return it
        let msg = receiver.try_recv().await.unwrap().unwrap();
        let RaftMsg::ClientWrite {
            payloads, responders, ..
        } = msg
        else {
            panic!("expected ClientWrite");
        };
        assert_eq!(extract_payload_data(&payloads), vec![42]);
        assert_eq!(responders.len(), payloads.len());
    });
}

#[test]
fn test_ensure_buffered_returns_immediately_if_already_buffered() {
    C::run(async {
        let (tx, rx) = C::mpsc(100);
        let mut receiver: BatchRaftMsgReceiver<C> = default_msg_receiver(rx);

        let leader = Some(committed_leader_id(1, 1));
        tx.send(client_write(1, None)).await.unwrap();
        tx.send(client_write(2, leader)).await.unwrap();

        receiver.ensure_buffered().await.unwrap();
        // try_recv will buffer the second message (different leader)
        let msg1 = receiver.try_recv().await.unwrap().unwrap();
        let RaftMsg::ClientWrite {
            payloads,
            responders,
            write_option,
            ..
        } = msg1
        else {
            panic!("expected ClientWrite");
        };
        assert_eq!(extract_payload_data(&payloads), vec![1]);
        assert_eq!(responders.len(), payloads.len());
        assert_eq!(write_option, leader_option(None));

        // ensure_buffered should return immediately since msg2 is buffered
        receiver.ensure_buffered().await.unwrap();

        let msg2 = receiver.try_recv().await.unwrap().unwrap();
        let RaftMsg::ClientWrite {
            payloads,
            responders,
            write_option,
            ..
        } = msg2
        else {
            panic!("expected ClientWrite");
        };
        assert_eq!(extract_payload_data(&payloads), vec![2]);
        assert_eq!(responders.len(), payloads.len());
        assert_eq!(write_option, leader_option(leader));
    });
}

#[test]
fn test_try_recv_batch_up_to_max_capacity() {
    const BATCH_CAPACITY: u64 = 64;
    const NUM_BATCHES: u64 = 2;
    const NUM_CLIENT_WRITES: u64 = BATCH_CAPACITY * NUM_BATCHES;

    C::run(async {
        let (tx, rx) = C::mpsc(NUM_CLIENT_WRITES as usize);
        let mut receiver: BatchRaftMsgReceiver<C> = BatchRaftMsgReceiver::new(rx, BATCH_CAPACITY, Duration::ZERO);

        let leader = Some(committed_leader_id(1, 1));
        for index in 0..NUM_CLIENT_WRITES {
            tx.send(client_write(index, leader)).await.unwrap();
        }

        receiver.ensure_buffered().await.unwrap();

        for batch in 0..NUM_BATCHES {
            let msg = receiver.try_recv().await.unwrap().unwrap();
            let RaftMsg::ClientWrite {
                payloads,
                responders,
                write_option,
                ..
            } = msg
            else {
                panic!("expected ClientWrite");
            };
            let first = batch * BATCH_CAPACITY;
            let want: Vec<u64> = (first..first + BATCH_CAPACITY).collect();
            assert_eq!(extract_payload_data(&payloads), want);
            assert_eq!(responders.len(), BATCH_CAPACITY as usize);
            assert_eq!(write_option, leader_option(leader));
        }

        let rest = receiver.try_recv().await.unwrap();
        assert!(rest.is_none(), "the two batches hold every write");
    });
}

#[test]
fn test_try_recv_flush_batch_after_linger() {
    const BATCH_CAPACITY: u64 = 100;
    const LINGER: Duration = Duration::from_secs(1);

    C::run(async {
        let (tx, rx) = C::mpsc(BATCH_CAPACITY as usize);
        let mut receiver: BatchRaftMsgReceiver<C> = BatchRaftMsgReceiver::new(rx, BATCH_CAPACITY, LINGER);

        let now = C::now();

        let leader = Some(committed_leader_id(1, 1));
        tx.send(client_write(0, leader)).await.unwrap();

        let msg = receiver.try_recv().await.unwrap().unwrap();
        let RaftMsg::ClientWrite {
            payloads, responders, ..
        } = msg
        else {
            panic!("expected ClientWrite");
        };

        assert_eq!(1, payloads.len());
        assert_eq!(1, responders.len());
        assert_eq!(Duration::ZERO, LINGER.saturating_sub(now.elapsed()))
    });
}
