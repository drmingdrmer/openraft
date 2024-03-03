use std::fmt;

use crate::display_ext::DisplayOptionExt;
use crate::LogId;
use crate::RaftTypeConfig;
use crate::Vote;

pub(crate) enum LogMeta<C>
where C: RaftTypeConfig
{
    V3(LogMetaV3<C>),
}

/// Represents a single record containing metadata related to the Raft log.
///
/// This structure stores metadata about the Raft log, such as the last vote,
/// the last purged log id, the last committed log id, and the last stored log id.
#[derive(Debug, Clone)]
#[derive(Default)]
#[derive(PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub(crate) struct LogMetaV3<C>
where C: RaftTypeConfig
{
    /// The most recent stored vote.
    pub vote: Option<Vote<C::NodeId>>,

    /// The last(greatest) purged log id.
    ///
    /// Logs contained in a snapshot are purged according to the specified policy.
    /// The log store does **NOT** store log entries in the range `[0, purged]`.
    /// `purged` represents the first log id that the log store is aware of.
    pub purged: Option<LogId<C::NodeId>>,

    /// The last(greatest) committed log id.
    ///
    /// NOTE: A log store implementation is not required to store the committed log IDs to provide
    /// correctness.
    /// Always returning `None` is acceptable.
    pub committed: Option<LogId<C::NodeId>>,

    /// The last(greatest) stored log id.
    pub last: Option<LogId<C::NodeId>>,
}

impl<C> fmt::Display for LogMetaV3<C>
where C: RaftTypeConfig
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{{vote:{}, purged,committed,last:({}, {}, {}]}}",
            self.vote.display(),
            self.purged.display(),
            self.committed.display(),
            self.last.display()
        )
    }
}

impl<C> LogMetaV3<C>
where C: RaftTypeConfig
{
    //
}
