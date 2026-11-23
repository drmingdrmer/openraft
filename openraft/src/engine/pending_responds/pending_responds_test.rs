use validit::Valid;

use super::*;
use crate::Vote;
use crate::engine::Respond;
use crate::engine::respond_command::PendingRespond;
use crate::engine::testing::UTConfig;
use crate::engine::testing::log_id;
use crate::raft::VoteResponse;
use crate::raft_state::IOId;
use crate::raft_state::io_state::IOState;
use crate::raft_state::io_state::io_progress::IOProgress;
use crate::type_config::TypeConfigExt;

type TestIOId = IOId<UTConfig>;
type TestLogId = LogIdOf<UTConfig>;

fn new_respond() -> Respond<UTConfig> {
    let (tx, _rx) = UTConfig::<()>::oneshot();
    Respond::new(VoteResponse::new(Vote::new(1, 1), None, false), tx)
}

fn new_io_state(
    log_io: Option<TestIOId>,
    applied: Option<TestLogId>,
    snapshot: Option<TestLogId>,
) -> IOState<UTConfig> {
    let vote = Vote::new(1, 1);
    let mut io_state = IOState::new("xx", &vote, applied, snapshot, None);
    if let Some(id) = log_io {
        io_state.log_progress = Valid::new(IOProgress::new_synchronized(Some(id), "xx", "log"));
    }
    io_state
}

#[test]
fn test_pop_if_satisfied() {
    let mut queue = VecDeque::new();
    queue.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));
    queue.push_back(PendingRespond::new(log_id(1, 1, 5), new_respond()));

    assert!(pop_if_satisfied(&mut queue, &log_id(1, 1, 2)).is_none());
    assert_eq!(queue.len(), 2);

    assert!(pop_if_satisfied(&mut queue, &log_id(1, 1, 3)).is_some());
    assert_eq!(queue.len(), 1);

    assert!(pop_if_satisfied(&mut queue, &log_id(1, 1, 10)).is_some());
    assert_eq!(queue.len(), 0);

    assert!(pop_if_satisfied(&mut queue, &log_id(1, 1, 10)).is_none());
}

#[test]
fn test_pop_if_satisfied_reuses_slot_zero_once_empty() {
    let mut queue = VecDeque::with_capacity(8);
    queue.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));
    let first_slot = queue.as_slices().0.as_ptr();

    assert!(pop_if_satisfied(&mut queue, &log_id(1, 1, 3)).is_some());
    queue.push_back(PendingRespond::new(log_id(1, 1, 4), new_respond()));
    assert_eq!(queue.as_slices().0.as_ptr(), first_slot);
}

#[test]
fn test_drain_satisfied_empty_queues() {
    let mut pending = PendingResponds::<UTConfig>::default();
    let io_state = new_io_state(None, None, None);

    let collected: Vec<_> = pending.drain_satisfied(&io_state).collect();
    assert_eq!(collected.len(), 0);
}

#[test]
fn test_drain_satisfied_all_unsatisfied() {
    let mut pending = PendingResponds::<UTConfig>::default();
    pending.on_log_flush.push_back(PendingRespond::new(log_id(1, 1, 5), new_respond()));
    pending.on_apply.push_back(PendingRespond::new(log_id(1, 1, 5), new_respond()));

    let io_state = new_io_state(None, None, None);

    let collected: Vec<_> = pending.drain_satisfied(&io_state).collect();
    assert_eq!(collected.len(), 0);
    assert_eq!(pending.on_log_flush.len(), 1);
    assert_eq!(pending.on_apply.len(), 1);
}

#[test]
fn test_drain_satisfied_log_io() {
    let mut pending = PendingResponds::<UTConfig>::default();
    let io_id = TestIOId::new_log_io(Default::default(), Some(log_id(1, 1, 3)));
    pending.on_log_io.push_back(PendingRespond::new(io_id, new_respond()));

    let flushed_io_id = TestIOId::new_log_io(Default::default(), Some(log_id(1, 1, 5)));
    let io_state = new_io_state(Some(flushed_io_id), None, None);

    let collected: Vec<_> = pending.drain_satisfied(&io_state).collect();
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].0, DrainPhase::LogIO);
    assert_eq!(pending.on_log_io.len(), 0);
}

#[test]
fn test_drain_satisfied_log_flush() {
    let mut pending = PendingResponds::<UTConfig>::default();
    pending.on_log_flush.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));

    let flushed_io_id = TestIOId::new_log_io(Default::default(), Some(log_id(1, 1, 5)));
    let io_state = new_io_state(Some(flushed_io_id), None, None);

    let collected: Vec<_> = pending.drain_satisfied(&io_state).collect();
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].0, DrainPhase::LogFlush);
    assert_eq!(pending.on_log_flush.len(), 0);
}

#[test]
fn test_drain_satisfied_apply() {
    let mut pending = PendingResponds::<UTConfig>::default();
    pending.on_apply.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));

    let io_state = new_io_state(None, Some(log_id(1, 1, 5)), None);

    let collected: Vec<_> = pending.drain_satisfied(&io_state).collect();
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].0, DrainPhase::Apply);
    assert_eq!(pending.on_apply.len(), 0);
}

#[test]
fn test_drain_satisfied_snapshot() {
    let mut pending = PendingResponds::<UTConfig>::default();
    pending.on_snapshot.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));

    let io_state = new_io_state(None, None, Some(log_id(1, 1, 5)));

    let collected: Vec<_> = pending.drain_satisfied(&io_state).collect();
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].0, DrainPhase::Snapshot);
    assert_eq!(pending.on_snapshot.len(), 0);
}

#[test]
fn test_drain_satisfied_multiple_phases() {
    let mut pending = PendingResponds::<UTConfig>::default();
    let io_id = TestIOId::new_log_io(Default::default(), Some(log_id(1, 1, 3)));
    pending.on_log_io.push_back(PendingRespond::new(io_id, new_respond()));
    pending.on_log_flush.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));
    pending.on_apply.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));
    pending.on_snapshot.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));

    let flushed_io_id = TestIOId::new_log_io(Default::default(), Some(log_id(1, 1, 5)));
    let io_state = new_io_state(Some(flushed_io_id), Some(log_id(1, 1, 5)), Some(log_id(1, 1, 5)));

    let collected: Vec<_> = pending.drain_satisfied(&io_state).collect();
    assert_eq!(collected.len(), 4);
    assert_eq!(collected[0].0, DrainPhase::LogIO);
    assert_eq!(collected[1].0, DrainPhase::LogFlush);
    assert_eq!(collected[2].0, DrainPhase::Apply);
    assert_eq!(collected[3].0, DrainPhase::Snapshot);
}

#[test]
fn test_drain_satisfied_partial_satisfaction() {
    let mut pending = PendingResponds::<UTConfig>::default();
    pending.on_log_flush.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));
    pending.on_log_flush.push_back(PendingRespond::new(log_id(1, 1, 5), new_respond()));
    pending.on_log_flush.push_back(PendingRespond::new(log_id(1, 1, 7), new_respond()));

    let flushed_io_id = TestIOId::new_log_io(Default::default(), Some(log_id(1, 1, 5)));
    let io_state = new_io_state(Some(flushed_io_id), None, None);

    let collected: Vec<_> = pending.drain_satisfied(&io_state).collect();
    assert_eq!(collected.len(), 2);
    assert_eq!(pending.on_log_flush.len(), 1);
}

#[test]
fn test_drain_satisfied_multiple_items_per_phase() {
    let mut pending = PendingResponds::<UTConfig>::default();

    let io_id1 = TestIOId::new_log_io(Default::default(), Some(log_id(1, 1, 2)));
    let io_id2 = TestIOId::new_log_io(Default::default(), Some(log_id(1, 1, 3)));
    let io_id3 = TestIOId::new_log_io(Default::default(), Some(log_id(1, 1, 7)));
    pending.on_log_io.push_back(PendingRespond::new(io_id1, new_respond()));
    pending.on_log_io.push_back(PendingRespond::new(io_id2, new_respond()));
    pending.on_log_io.push_back(PendingRespond::new(io_id3, new_respond()));

    pending.on_log_flush.push_back(PendingRespond::new(log_id(1, 1, 2), new_respond()));
    pending.on_log_flush.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));
    pending.on_log_flush.push_back(PendingRespond::new(log_id(1, 1, 4), new_respond()));
    pending.on_log_flush.push_back(PendingRespond::new(log_id(1, 1, 8), new_respond()));

    pending.on_apply.push_back(PendingRespond::new(log_id(1, 1, 2), new_respond()));
    pending.on_apply.push_back(PendingRespond::new(log_id(1, 1, 4), new_respond()));
    pending.on_apply.push_back(PendingRespond::new(log_id(1, 1, 9), new_respond()));

    pending.on_snapshot.push_back(PendingRespond::new(log_id(1, 1, 3), new_respond()));
    pending.on_snapshot.push_back(PendingRespond::new(log_id(1, 1, 10), new_respond()));

    let flushed_io_id = TestIOId::new_log_io(Default::default(), Some(log_id(1, 1, 5)));
    let io_state = new_io_state(Some(flushed_io_id), Some(log_id(1, 1, 5)), Some(log_id(1, 1, 5)));

    let collected: Vec<_> = pending.drain_satisfied(&io_state).collect();
    assert_eq!(collected.len(), 8);

    assert_eq!(collected[0].0, DrainPhase::LogIO);
    assert_eq!(collected[1].0, DrainPhase::LogIO);
    assert_eq!(collected[2].0, DrainPhase::LogFlush);
    assert_eq!(collected[3].0, DrainPhase::LogFlush);
    assert_eq!(collected[4].0, DrainPhase::LogFlush);
    assert_eq!(collected[5].0, DrainPhase::Apply);
    assert_eq!(collected[6].0, DrainPhase::Apply);
    assert_eq!(collected[7].0, DrainPhase::Snapshot);

    assert_eq!(pending.on_log_io.len(), 1);
    assert_eq!(pending.on_log_flush.len(), 1);
    assert_eq!(pending.on_apply.len(), 1);
    assert_eq!(pending.on_snapshot.len(), 1);
}

#[test]
fn test_drain_phase_display() {
    assert_eq!(format!("{}", DrainPhase::LogIO), "log_io");
    assert_eq!(format!("{}", DrainPhase::LogFlush), "log_flush");
    assert_eq!(format!("{}", DrainPhase::Apply), "apply");
    assert_eq!(format!("{}", DrainPhase::Snapshot), "snapshot");
    assert_eq!(format!("{}", DrainPhase::Done), "done");
}
