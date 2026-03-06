use std::fmt;
use std::fmt::Formatter;

use crate::RaftPrimitives;
use crate::StorageError;
use crate::core::ApplyResult;
use crate::display_ext::DisplayOptionExt;
use crate::display_ext::display_result::DisplayResultExt;
use crate::raft_state::io_state::log_io_id::LogIOId;
use crate::storage::SnapshotMeta;

/// The Ok part of a state machine command result.
#[derive(Debug)]
pub(crate) enum Response<P>
where P: RaftPrimitives
{
    /// Snapshot building completed or was deferred.
    ///
    /// - `Some(meta)`: Snapshot was successfully built with the given metadata.
    /// - `None`: State machine deferred snapshot creation via `try_create_snapshot_builder()`.
    BuildSnapshotDone(Option<SnapshotMeta<P>>),

    /// When finishing installing a snapshot.
    ///
    /// It does not return any value to RaftCore.
    InstallSnapshot((LogIOId<P>, Option<SnapshotMeta<P>>)),

    /// Send back applied result to RaftCore.
    Apply(ApplyResult<P>),
}

impl<P> fmt::Display for Response<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuildSnapshotDone(meta) => {
                write!(f, "BuildSnapshotDone({})", meta.display())
            }
            Self::InstallSnapshot((io_id, meta)) => {
                write!(f, "InstallSnapshot(io_id:{}, meta:{})", io_id, meta.display())
            }
            Self::Apply(result) => {
                write!(f, "{}", result)
            }
        }
    }
}

/// Container of result of a command.
#[derive(Debug)]
pub(crate) struct CommandResult<P>
where P: RaftPrimitives
{
    pub(crate) result: Result<Response<P>, StorageError<P>>,
}

impl<P> fmt::Display for CommandResult<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "sm::Result({})", self.result.display())
    }
}

impl<P> CommandResult<P>
where P: RaftPrimitives
{
    pub(crate) fn new(result: Result<Response<P>, StorageError<P>>) -> Self {
        Self { result }
    }
}
