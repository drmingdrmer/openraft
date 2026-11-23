use std::fmt;

use crate::LogId;
use crate::engine::leader_log_ids_iter::LeaderLogIdsIter;
use crate::log_id::ref_log_id::RefLogId;
use crate::vote::RaftCommittedLeaderId;

/// A non-empty range of log IDs belonging to a Leader.
///
/// This struct represents a contiguous range of log IDs that share the same
/// committed leader ID. It provides efficient access to the first and last
/// log IDs, and can be iterated over to produce all log IDs in the range.
///
/// The range is `[first, last]` (both inclusive).
/// This struct always represents a non-empty range; use `Option<LeaderLogIds>`
/// when an empty range is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeaderLogIds<CLID>
where CLID: RaftCommittedLeaderId
{
    committed_leader_id: CLID,

    /// First index (inclusive).
    first: u64,

    /// Last index (inclusive).
    last: u64,
}

impl<CLID> fmt::Display for LeaderLogIds<CLID>
where CLID: RaftCommittedLeaderId
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:[{}, {}]", self.committed_leader_id, self.first, self.last)
    }
}

impl<CLID> LeaderLogIds<CLID>
where CLID: RaftCommittedLeaderId
{
    /// Create a new log ID range `[first, last]`.
    pub(crate) fn new(committed_leader_id: CLID, first: u64, last: u64) -> Self {
        debug_assert!(first <= last, "first {} must be <= last {}", first, last);
        Self {
            committed_leader_id,
            first,
            last,
        }
    }

    /// Returns a reference to the last log ID in the range.
    #[cfg_attr(not(feature = "runtime-stats"), allow(dead_code))]
    pub(crate) fn last_ref(&self) -> RefLogId<'_, CLID> {
        RefLogId::new(&self.committed_leader_id, self.last)
    }

    /// Returns the log ID at the specified index.
    ///
    /// # Panics
    /// Panics if `index` is out of range `[first, last]`.
    #[allow(dead_code)]
    pub(crate) fn get(&self, index: u64) -> LogId<CLID> {
        debug_assert!(
            index >= self.first && index <= self.last,
            "index {} out of range [{}, {}]",
            index,
            self.first,
            self.last
        );
        LogId::<CLID>::new(self.committed_leader_id.clone(), index)
    }

    /// Returns a reference to the log ID at the specified index.
    ///
    /// # Panics
    /// Panics if `index` is out of range `[first, last]`.
    #[allow(dead_code)]
    pub(crate) fn ref_at(&self, index: u64) -> RefLogId<'_, CLID> {
        debug_assert!(
            index >= self.first && index <= self.last,
            "index {} out of range [{}, {}]",
            index,
            self.first,
            self.last
        );
        RefLogId::new(&self.committed_leader_id, index)
    }

    /// Returns the number of log IDs in the range.
    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        if self.first > self.last {
            0
        } else {
            (self.last - self.first + 1) as usize
        }
    }
}

impl<CLID> IntoIterator for LeaderLogIds<CLID>
where CLID: RaftCommittedLeaderId
{
    type Item = LogId<CLID>;
    type IntoIter = LeaderLogIdsIter<CLID>;

    fn into_iter(self) -> Self::IntoIter {
        LeaderLogIdsIter::new(
            self.committed_leader_id,
            self.first,
            self.last + 1, // half-open: [start, end)
        )
    }
}

#[cfg(test)]
mod leader_log_ids_test;
