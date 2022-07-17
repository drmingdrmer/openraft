use std::collections::BTreeSet;

use crate::quorum::QuorumSet;
use crate::NodeId;
use crate::Vote;

/// The reason to reject a vote request
#[derive(Debug)]
pub(crate) enum Rejection {
    /// There is still a active leader.
    /// I.e., the timeout since last heartbeat from some leader has not yet expired.
    // TODO: remove this when heartbeat log is added.
    StillHasLeader,

    /// This vote is not greater than or equal as the one of a remote voter.
    ///
    /// The value of `Vote` can be partial ordered. It is possible both `a !>= b` and `b !>= a` hold.
    Vote,

    /// The last-log-id is less than the the remote voter.
    LastLogId,
}

/// What to do with voting:
/// - to continue this round of voting(i.e., stay in candidate),
/// - restart another round of voting,
/// - or just stop(i.e., revert to follower).
#[derive(Debug)]
pub(crate) enum VotingResult<NID: NodeId> {
    Granted,
    Continue,
    VoteTooSmall(Option<Vote<NID>>),
    LogTooSmall(Option<Vote<NID>>),
}

/// Tracks state of a voting progress.
///
/// It is mainly used by a candidate.
#[derive(Clone, Debug, Default)]
#[derive(PartialEq, Eq)]
pub(crate) struct Voting<NID: NodeId> {
    /// Nodes that have already granted this vote.
    pub(crate) granted_by: BTreeSet<NID>,

    /// Nodes that have not yet reject this vote.
    pub(crate) can_grant: BTreeSet<NID>,

    /// Nodes that are not known to have greater last_log_id.
    pub(crate) log_less_or_equal: BTreeSet<NID>,

    /// The greatest vote this candidate has ever seen.
    pub(crate) greatest_vote: Option<Vote<NID>>,
}

impl<NID: NodeId> Voting<NID> {
    pub(crate) fn new(voter_ids: impl Iterator<Item = NID>) -> Self {
        let voter_ids = voter_ids.collect::<BTreeSet<_>>();

        Voting {
            granted_by: BTreeSet::new(),
            can_grant: voter_ids.clone(),
            log_less_or_equal: voter_ids,
            greatest_vote: None,
        }
    }

    /// Let a voter grant this vote.
    pub(crate) fn grant_by(&mut self, target: NID) {
        tracing::debug!(target = display(target), "vote is granted");
        self.granted_by.insert(target);
    }

    /// Let a voter reject this vote.
    pub(crate) fn reject_by(&mut self, target: NID, rejection: Rejection, resp_vote: Vote<NID>) {
        tracing::debug!(
            target = display(target),
            rejection = debug(rejection),
            resp_vote = debug(&resp_vote),
            "vote is rejected"
        );

        if self.greatest_vote < resp_vote {
            self.greatest_vote = Some(resp_vote);
        }

        self.can_grant.remove(&target);

        match rejection {
            Rejection::StillHasLeader => {
                // nothing to do
            }
            Rejection::Vote => {
                // nothing to do
            }
            Rejection::LastLogId => {
                //
                self.log_less_or_equal.remove(&target);
            }
        }
    }

    pub(crate) fn get_voting_result<QS>(&self, qs: &QS) -> VotingResult<NID>
    where QS: QuorumSet<NID> + 'static {
        if qs.is_quorum(self.granted_by.iter()) {
            return VotingResult::Granted;
        }

        // Not yet granted

        let can_be_leader = qs.is_quorum(self.log_less_or_equal.iter());

        if !can_be_leader {
            // Can never be a leader. Revert to follower.
            return VotingResult::LogTooSmall(self.greatest_vote);
        }

        // Can become a leader

        let can_be_granted_by_a_quorum = qs.is_quorum(self.can_grant.iter());

        if !can_be_granted_by_a_quorum {
            // No quorum left can grant this vote.
            return VotingResult::VoteTooSmall(self.greatest_vote);
        }

        return VotingResult::Continue;
    }
}

#[cfg(test)]
mod tests {
    use crate::leader::voting::Rejection;
    use crate::leader::voting::VotingResult;
    use crate::leader::Voting;
    use crate::quorum::QuorumSet;
    use crate::NodeId;
    use crate::Vote;

    fn voting() -> Voting<u64> {
        Voting::new([1, 2, 3, 4, 5].iter())
    }

    fn qs() -> impl QuorumSet<u64> {
        vec![1, 2, 3, 4, 5]
    }

    fn vote(term: u64, node_id: u64) -> Vote<u64> {
        Vote::new(term, node_id)
    }

    #[test]
    fn test_voting_granted() -> anyhow::Result<()> {
        let mut v = voting();
        v.grant_by(1);
        v.grant_by(2);
        v.grant_by(3);
        v.reject_by(4, Rejection::LastLogId, vote(1, 2));
        v.reject_by(5, Rejection::Vote, vote(1, 2));

        assert_eq!(VotingResult::Granted, v.get_voting_result(&qs()));

        Ok(())
    }

    #[test]
    fn test_voting_continue_when_there_is_leader() -> anyhow::Result<()> {
        // TODO: continue voting is inappropriate when there is a leader.
        //       This can be solved with heartbeat log.
        let mut v = voting();
        v.grant_by(1);
        v.grant_by(2);
        v.reject_by(3, Rejection::StillHasLeader, vote(1, 2));
        v.reject_by(4, Rejection::StillHasLeader, vote(1, 2));

        assert_eq!(
            VotingResult::Continue,
            v.get_voting_result(&qs()),
            "there is already an active leader"
        );

        Ok(())
    }

    #[test]
    fn test_voting_continue() -> anyhow::Result<()> {
        let mut v = voting();
        v.grant_by(1);
        v.grant_by(2);
        v.reject_by(3, Rejection::LastLogId, vote(1, 2));
        v.reject_by(4, Rejection::Vote, vote(1, 2));

        assert_eq!(
            VotingResult::Continue,
            v.get_voting_result(&qs()),
            "a granting quorum can be 1,2,5"
        );
        Ok(())
    }

    #[test]
    fn test_voting_restart() -> anyhow::Result<()> {
        let mut v = voting();
        v.grant_by(1);
        v.grant_by(2);
        v.reject_by(3, Rejection::LastLogId, vote(1, 2));
        v.reject_by(4, Rejection::Vote, vote(3, 2));
        v.reject_by(5, Rejection::Vote, vote(2, 2));

        assert_eq!(
            VotingResult::VoteTooSmall(Some(vote(3, 2))),
            v.get_voting_result(&qs()),
            "there is greater log, but it still can be a leader"
        );

        Ok(())
    }

    #[test]
    fn test_voting_stop() -> anyhow::Result<()> {
        let mut v = voting();
        v.grant_by(1);
        v.grant_by(2);
        v.reject_by(3, Rejection::LastLogId, vote(3, 2));
        v.reject_by(4, Rejection::LastLogId, vote(1, 2));
        v.reject_by(5, Rejection::LastLogId, vote(2, 2));

        assert_eq!(
            VotingResult::LogTooSmall(Some(vote(3, 2))),
            v.get_voting_result(&qs()),
            "can never be a leader"
        );

        Ok(())
    }
}
