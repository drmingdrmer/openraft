use openraft_macros::since;

use crate::RaftTypeConfig;
use crate::StorageError;

/// Unrecoverable error that stops the Raft node, or that leaves a request without a response.
///
/// `RaftCore`, the task that runs the protocol, stops with a `Fatal` error. API calls on the
/// stopped node return that error, and [`RaftMetrics::running_state`] holds it. An API call whose
/// request got no response returns [`NoResponse`](Self::NoResponse) instead when it does not see
/// `RaftCore` stop. An application that gets a `Fatal` error should take the node out of service
/// and shut it down with [`Raft::shutdown`]. Getting the error does not mean that the node's tasks
/// have exited, and a shutdown joins only the `RaftCore` task.
///
/// # Variants
///
/// - `StorageError`: Underlying storage (log or state machine) encountered an error
/// - `Panicked`: Raft core task panicked due to a programming error
/// - `Stopped`: Raft core task, or the state machine worker, stopped for another reason, such as a
///   shutdown or the drop of every `Raft` handle
/// - `NoResponse`: a request got no response, and the Raft core task was not seen to stop
///
/// [`Raft::shutdown`]: crate::Raft::shutdown
/// [`RaftMetrics::running_state`]: crate::RaftMetrics::running_state
#[since(version = "0.10.2", change = "added `NoResponse`")]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub enum Fatal<C>
where C: RaftTypeConfig
{
    /// Storage error that caused the Raft node to stop.
    #[error(transparent)]
    StorageError(#[from] StorageError<C>),

    /// Raft node panicked and stopped.
    #[error("panicked")]
    Panicked,

    /// Raft core task stopped without a storage error or a panic.
    ///
    /// The core task stops this way when its control channel closes, by [`Raft::shutdown`] or by
    /// dropping every `Raft` handle. A node shut down before [`Raft::run`] reports `Stopped` too,
    /// and so does a core task whose runtime join fails without a panic, such as by cancellation.
    ///
    /// So `Stopped` alone does not tell an explicit shutdown from a dropped handle, and it does
    /// not mean that the core task joined successfully: [`Raft::try_shutdown`] returns an error
    /// when that join failed.
    ///
    /// The state machine worker, a task of its own, can stop without an error that the core task
    /// receives, such as after a panic in a function passed to [`Raft::with_state_machine`]. The
    /// core task then stops with `Stopped` when it next sends the worker a command. An API call
    /// that sends its command to the worker directly, such as [`Raft::get_snapshot`], and finds
    /// the worker stopped, returns `Stopped` too, also while the core task runs.
    ///
    /// [`Raft::get_snapshot`]: crate::Raft::get_snapshot
    /// [`Raft::run`]: crate::Raft::run
    /// [`Raft::shutdown`]: crate::Raft::shutdown
    /// [`Raft::try_shutdown`]: crate::Raft::try_shutdown
    /// [`Raft::with_state_machine`]: crate::Raft::with_state_machine
    #[error("raft stopped")]
    Stopped,

    /// A request got no response, and the Raft core task was not seen to stop.
    ///
    /// A task that serves a request can drop it without a reply while the core task runs, such as
    /// the state machine worker after a panic in the state machine or in a function passed to
    /// [`Raft::with_state_machine`]. An API call whose response does not arrive waits up to one
    /// second for the core task to stop. It returns the error that stopped the core task if it
    /// sees the core task stop, and `NoResponse` otherwise. The core task never stops with
    /// `NoResponse`, so [`RaftMetrics::running_state`] never holds it.
    ///
    /// What the request did is unknown: a write that gets `NoResponse` may have been committed
    /// and applied, or not.
    ///
    /// [`Raft::with_state_machine`]: crate::Raft::with_state_machine
    /// [`RaftMetrics::running_state`]: crate::RaftMetrics::running_state
    #[since(version = "0.10.2")]
    #[error("no response arrived")]
    NoResponse,
}
