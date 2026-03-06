use std::fmt;

use crate::RaftPrimitives;
use crate::SnapshotId;
use crate::StoredMembership;
use crate::display_ext::DisplayOption;
use crate::storage::SnapshotSignature;
use crate::type_config::alias::LogIdOf;

/// The metadata of a snapshot.
///
/// Including the last log id that is included in this snapshot,
/// the last membership included,
/// and a snapshot id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub struct SnapshotMeta<P>
where P: RaftPrimitives
{
    /// Log entries up to which this snapshot includes, inclusive.
    pub last_log_id: Option<LogIdOf<P>>,

    /// The last applied membership config.
    pub last_membership: StoredMembership<P>,

    /// To identify a snapshot when transferring.
    /// Caveat: even when two snapshots are built with the same `last_log_id`, they still could be
    /// different in bytes.
    pub snapshot_id: SnapshotId,
}

impl<P> fmt::Display for SnapshotMeta<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{{snapshot_id: {}, last_log:{}, last_membership: {}}}",
            self.snapshot_id,
            DisplayOption(&self.last_log_id),
            self.last_membership
        )
    }
}

impl<P> SnapshotMeta<P>
where P: RaftPrimitives
{
    /// Get the signature of this snapshot metadata for comparison and identification.
    pub fn signature(&self) -> SnapshotSignature<P> {
        SnapshotSignature {
            last_log_id: self.last_log_id.clone(),
            last_membership_log_id: self.last_membership.log_id().as_ref().map(|x| Box::new(x.clone())),
            snapshot_id: self.snapshot_id.clone(),
        }
    }

    /// Returns a ref to the id of the last log that is included in this snapshot.
    pub fn last_log_id(&self) -> Option<&LogIdOf<P>> {
        self.last_log_id.as_ref()
    }
}
