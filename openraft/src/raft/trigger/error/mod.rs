//! Errors that occur during user triggering raft actions.

mod allow_next_revert_error;
mod send_snapshot_error;

pub use self::allow_next_revert_error::AllowNextRevertError;
pub use self::send_snapshot_error::SendSnapshotError;
