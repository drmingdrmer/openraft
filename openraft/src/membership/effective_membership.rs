use std::collections::BTreeSet;
use std::fmt;
use std::fmt::Debug;
use std::sync::Arc;

use crate::Membership;
use crate::RaftPrimitives;
use crate::StoredMembership;
use crate::display_ext::DisplayOptionExt;
use crate::log_id::raft_log_id::RaftLogId;
use crate::log_id::raft_log_id_ext::RaftLogIdExt;
use crate::quorum::Joint;
use crate::quorum::QuorumSet;
use crate::type_config::alias::LogIdOf;

/// The currently active membership config.
///
/// It includes:
/// - the id of the log that sets this membership config,
/// - and the config.
///
/// An active config is just the last seen config in raft spec.
#[derive(Clone, Eq)]
pub struct EffectiveMembership<P>
where P: RaftPrimitives
{
    stored_membership: Arc<StoredMembership<P>>,

    /// The quorum set built from `membership`.
    quorum_set: Joint<P::NodeId, Vec<P::NodeId>, Vec<Vec<P::NodeId>>>,

    /// Cache of the union of all members
    voter_ids: BTreeSet<P::NodeId>,
}

impl<P> Default for EffectiveMembership<P>
where P: RaftPrimitives
{
    fn default() -> Self {
        Self {
            stored_membership: Arc::new(StoredMembership::default()),
            quorum_set: Joint::default(),
            voter_ids: Default::default(),
        }
    }
}

impl<P> Debug for EffectiveMembership<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectiveMembership")
            .field("log_id", self.log_id())
            .field("membership", self.membership())
            .field("voter_ids", &self.voter_ids)
            .finish()
    }
}

impl<P> PartialEq for EffectiveMembership<P>
where P: RaftPrimitives
{
    fn eq(&self, other: &Self) -> bool {
        self.stored_membership == other.stored_membership && self.voter_ids == other.voter_ids
    }
}

impl<P, LID> From<(&LID, Membership<P>)> for EffectiveMembership<P>
where
    P: RaftPrimitives,
    LID: RaftLogId<P>,
{
    fn from(v: (&LID, Membership<P>)) -> Self {
        EffectiveMembership::new(Some(v.0.to_log_id()), v.1)
    }
}

impl<P> EffectiveMembership<P>
where P: RaftPrimitives
{
    pub(crate) fn new_arc(log_id: Option<LogIdOf<P>>, membership: Membership<P>) -> Arc<Self> {
        Arc::new(Self::new(log_id, membership))
    }

    /// Create a new EffectiveMembership from a log ID and membership configuration.
    pub fn new(log_id: Option<LogIdOf<P>>, membership: Membership<P>) -> Self {
        let voter_ids = membership.voter_ids().collect();

        let configs = membership.get_joint_config();
        let mut joint = vec![];
        for c in configs {
            joint.push(c.iter().cloned().collect::<Vec<_>>());
        }

        let quorum_set = Joint::from(joint);

        Self {
            stored_membership: Arc::new(StoredMembership::new(log_id, membership)),
            quorum_set,
            voter_ids,
        }
    }

    pub(crate) fn new_from_stored_membership(stored: StoredMembership<P>) -> Self {
        Self::new(stored.log_id().clone(), stored.membership().clone())
    }

    pub(crate) fn stored_membership(&self) -> &Arc<StoredMembership<P>> {
        &self.stored_membership
    }

    /// Get the log ID at which this membership was stored.
    pub fn log_id(&self) -> &Option<LogIdOf<P>> {
        self.stored_membership.log_id()
    }

    /// Get the membership configuration.
    pub fn membership(&self) -> &Membership<P> {
        self.stored_membership.membership()
    }
}

/// Membership API
impl<P> EffectiveMembership<P>
where P: RaftPrimitives
{
    #[allow(dead_code)]
    pub(crate) fn is_voter(&self, nid: &P::NodeId) -> bool {
        self.membership().is_voter(nid)
    }

    /// Returns an Iterator of all voter node ids. Learners are not included.
    pub fn voter_ids(&self) -> impl Iterator<Item = P::NodeId> + '_ {
        self.voter_ids.iter().cloned()
    }

    /// Returns an Iterator of all learner node ids. Voters are not included.
    pub(crate) fn learner_ids(&self) -> impl Iterator<Item = P::NodeId> + '_ {
        self.membership().learner_ids()
    }

    /// Get the node (either voter or learner) by node id.
    pub fn get_node(&self, node_id: &P::NodeId) -> Option<&P::Node> {
        self.membership().get_node(node_id)
    }

    /// Returns an Iterator of all nodes (voters and learners).
    pub fn nodes(&self) -> impl Iterator<Item = (&P::NodeId, &P::Node)> {
        self.membership().nodes()
    }

    /// Returns reference to the joint config.
    ///
    /// Membership is defined by a joint of multiple configs.
    /// Each config is a vec of node-id.
    pub fn get_joint_config(&self) -> &Vec<Vec<P::NodeId>> {
        self.quorum_set.children()
    }
}

impl<P> fmt::Display for EffectiveMembership<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EffectiveMembership{{log_id: {}, membership: {}}}",
            self.log_id().display(),
            self.membership()
        )
    }
}

/// Implement node-id joint quorum set.
impl<P> QuorumSet<P::NodeId> for EffectiveMembership<P>
where P: RaftPrimitives
{
    type Iter = std::collections::btree_set::IntoIter<P::NodeId>;

    fn is_quorum<'a, I: Iterator<Item = &'a P::NodeId> + Clone>(&self, ids: I) -> bool {
        self.quorum_set.is_quorum(ids)
    }

    fn ids(&self) -> Self::Iter {
        self.quorum_set.ids()
    }
}
