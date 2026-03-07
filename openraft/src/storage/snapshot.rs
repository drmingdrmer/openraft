use std::fmt;

use openraft_macros::since;

use crate::RaftTypes;
use crate::storage::SnapshotMeta;

/// The data associated with the current snapshot.
#[since(version = "0.10.0", change = "SnapshotData without Box")]
#[derive(Debug, Clone)]
pub struct Snapshot<C>
where C: RaftTypes
{
    /// metadata of a snapshot
    pub meta: SnapshotMeta<C::Prim>,

    /// A read handle to the associated snapshot.
    pub snapshot: C::SnapshotData,
}

impl<C> Snapshot<C>
where C: RaftTypes
{
    #[allow(dead_code)]
    pub(crate) fn new(meta: SnapshotMeta<C::Prim>, snapshot: C::SnapshotData) -> Self {
        Self { meta, snapshot }
    }
}

impl<C> fmt::Display for Snapshot<C>
where C: RaftTypes
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Snapshot{{meta: {}}}", self.meta)
    }
}
