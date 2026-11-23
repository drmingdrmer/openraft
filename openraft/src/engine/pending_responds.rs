use std::collections::VecDeque;
use std::fmt;

use crate::RaftTypeConfig;
use crate::engine::Respond;
use crate::engine::respond_command::PendingRespond;
use crate::raft_state::IOId;
use crate::raft_state::io_state::IOState;
use crate::type_config::alias::LogIdOf;

/// Queues of pending responds waiting for IO conditions to be satisfied.
#[derive(Debug, Default)]
pub(crate) struct PendingResponds<C>
where C: RaftTypeConfig
{
    /// Responds waiting for log IO to be flushed to storage.
    pub(crate) on_log_io: VecDeque<PendingRespond<C, IOId<C>>>,

    /// Responds waiting for log entries to be flushed to storage.
    pub(crate) on_log_flush: VecDeque<PendingRespond<C, LogIdOf<C>>>,

    /// Responds waiting for log entries to be applied to state machine.
    pub(crate) on_apply: VecDeque<PendingRespond<C, LogIdOf<C>>>,

    /// Responds waiting for snapshot to be built.
    pub(crate) on_snapshot: VecDeque<PendingRespond<C, LogIdOf<C>>>,
}

impl<C> PendingResponds<C>
where C: RaftTypeConfig
{
    /// Drain all satisfied responds based on the current IO state.
    ///
    /// Returns an iterator that yields all responds whose conditions are met by the provided
    /// IO state. The responds are removed from their respective queues as they are yielded.
    pub(crate) fn drain_satisfied<'a>(&'a mut self, io_state: &'a IOState<C>) -> DrainSatisfied<'a, C> {
        DrainSatisfied {
            pending_responds: self,
            io_state,
            phase: DrainPhase::LogIO,
            from: 0,
        }
    }
}

/// Remove and return the first respond at or after `*from` whose condition `actual` satisfies.
///
/// A condition is satisfied when `actual >= wait_for`; a condition that `actual` cannot be
/// compared with is not. The unsatisfied responds before the removed one stay in the queue, in
/// order, and `*from` moves to the place of the removed one, so that the next call goes on from
/// there instead of scanning the queue again from the front.
pub(crate) fn pop_first_satisfied<C, V>(
    queue: &mut VecDeque<PendingRespond<C, V>>,
    from: &mut usize,
    actual: &V,
) -> Option<Respond<C>>
where
    C: RaftTypeConfig,
    V: PartialOrd,
{
    let offset = queue.range(*from..).position(|p| actual >= p.wait_for())?;
    *from += offset;

    let respond = queue.remove(*from).map(|p| p.into_respond());
    // Reset the head so a queue that has grown does not walk its whole buffer.
    if queue.is_empty() {
        queue.clear();
    }
    respond
}

/// Iterator that drains satisfied responds from pending queues.
///
/// Iterates through all phases (LogIO, LogFlush, Apply, Snapshot) and yields responds
/// whose waiting conditions are satisfied by the current IO state. A respond whose condition is
/// not satisfied does not hold back a satisfied one behind it in its queue, because responds are
/// queued in the order their requests arrived, not in the order of their conditions.
///
/// Each respond is removed from its queue only when it is yielded, so dropping the iterator
/// early keeps every respond it has not yielded.
pub(crate) struct DrainSatisfied<'a, C>
where C: RaftTypeConfig
{
    pending_responds: &'a mut PendingResponds<C>,
    io_state: &'a IOState<C>,
    phase: DrainPhase,

    /// Where the current phase goes on scanning its queue: every respond before it is unsatisfied.
    from: usize,
}

/// Phases for draining satisfied responds from pending queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrainPhase {
    /// Draining responds waiting for log IO to be flushed.
    LogIO,
    /// Draining responds waiting for log entries to be flushed.
    LogFlush,
    /// Draining responds waiting for log entries to be applied.
    Apply,
    /// Draining responds waiting for snapshot to be built.
    Snapshot,
    /// All phases completed.
    Done,
}

impl fmt::Display for DrainPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DrainPhase::LogIO => write!(f, "log_io"),
            DrainPhase::LogFlush => write!(f, "log_flush"),
            DrainPhase::Apply => write!(f, "apply"),
            DrainPhase::Snapshot => write!(f, "snapshot"),
            DrainPhase::Done => write!(f, "done"),
        }
    }
}

impl<C> DrainSatisfied<'_, C>
where C: RaftTypeConfig
{
    /// Go on to `phase`, whose queue is scanned from its front.
    fn next_phase(&mut self, phase: DrainPhase) {
        self.phase = phase;
        self.from = 0;
    }
}

impl<'a, C> Iterator for DrainSatisfied<'a, C>
where C: RaftTypeConfig
{
    type Item = (DrainPhase, Respond<C>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.phase {
                DrainPhase::LogIO => {
                    if let Some(flushed) = self.io_state.log_progress.flushed()
                        && let Some(respond) =
                            pop_first_satisfied(&mut self.pending_responds.on_log_io, &mut self.from, flushed)
                    {
                        return Some((self.phase, respond));
                    }
                    self.next_phase(DrainPhase::LogFlush);
                }
                DrainPhase::LogFlush => {
                    if let Some(flushed) = self.io_state.log_progress.flushed()
                        && let Some(log_id) = flushed.last_log_id()
                        && let Some(respond) =
                            pop_first_satisfied(&mut self.pending_responds.on_log_flush, &mut self.from, log_id)
                    {
                        return Some((self.phase, respond));
                    }
                    self.next_phase(DrainPhase::Apply);
                }
                DrainPhase::Apply => {
                    if let Some(applied) = self.io_state.apply_progress.flushed()
                        && let Some(respond) =
                            pop_first_satisfied(&mut self.pending_responds.on_apply, &mut self.from, applied)
                    {
                        return Some((self.phase, respond));
                    }
                    self.next_phase(DrainPhase::Snapshot);
                }
                DrainPhase::Snapshot => {
                    if let Some(snapshot) = self.io_state.snapshot.flushed()
                        && let Some(respond) =
                            pop_first_satisfied(&mut self.pending_responds.on_snapshot, &mut self.from, snapshot)
                    {
                        return Some((self.phase, respond));
                    }
                    self.next_phase(DrainPhase::Done);
                }
                DrainPhase::Done => {
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod pending_responds_test;
