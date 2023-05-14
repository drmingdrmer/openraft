use std::error::Error;
use std::fmt;

use crate::display_ext::DisplayOptionExt;
use crate::validate::Validate;
use crate::LeaderId;
use crate::LogId;
use crate::NodeId;

/// A value that has been accepted by an acceptor(follower or learner).
///
/// In Paxos, an accepted value consists of a ballot number and a value: `(vballot, value)`.
///
/// It implements `PartialOrd` because the value replicated by a newer leader is considered
/// greater than the value replicated by a former leader.
/// And the comparison between values with the same leader id is delegated to the `last_log_id`.
#[derive(Debug, Clone, Copy)]
#[derive(PartialEq, Eq)]
#[derive(PartialOrd, Ord)]
pub(crate) struct Accepted<NID: NodeId> {
    /// The leader id that proposed and replicated the value.
    ///
    /// The leader id is used to determine the `committed` value: only the value proposed by the
    /// leader can be chosen by the next leader and is considered committed.
    /// Therefore, the same value proposed by a former leader cannot be considered committed.
    ///
    /// The lack of leader id in the log replication process is the root cause of the **unusual**
    /// criteria for committed value in the Raft.
    pub(crate) leader_id: LeaderId<NID>,

    /// The value part on an acceptor(follower or learner).
    ///
    /// Raft requires logs to be consecutive thus we only need to record the last log id.
    pub(crate) last_log_id: Option<LogId<NID>>,
}

impl<NID: NodeId> fmt::Display for Accepted<NID> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.leader_id, self.last_log_id.display())
    }
}

impl<NID: NodeId> Accepted<NID> {
    pub(crate) fn new(leader_id: LeaderId<NID>, log_id: Option<LogId<NID>>) -> Self {
        Self {
            leader_id,
            last_log_id: log_id,
        }
    }

    pub(crate) fn accept_value(&mut self, value: Option<LogId<NID>>) {
        self.last_log_id = value;
    }
}

/// A value that has been accepted by an acceptor(either a follower or a learner).
///
/// Similar to [`Accepted`], but this struct contains references to fields instead of owning them.
pub(crate) struct AcceptedBorrow<'a, NID: NodeId> {
    /// The ID of the leader that proposed and replicated the value.
    pub(crate) leader_id: &'a LeaderId<NID>,

    /// The value portion on an acceptor (either a follower or a learner).
    pub(crate) last_log_id: &'a Option<LogId<NID>>,
}

impl<'a, NID: NodeId> AcceptedBorrow<'a, NID> {
    pub(crate) fn new(vballot: &'a LeaderId<NID>, value: &'a Option<LogId<NID>>) -> Self {
        Self {
            leader_id: vballot,
            last_log_id: value,
        }
    }
}
