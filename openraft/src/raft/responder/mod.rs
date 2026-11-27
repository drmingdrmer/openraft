//! API to consume a response when a client write request is completed.

pub(crate) mod core_responder;
pub(crate) mod impls;
pub use impls::OneshotResponder;
pub use impls::ProgressResponder;
use openraft_macros::since;

use crate::OptionalSend;
use crate::RaftTypeConfig;
use crate::type_config::alias::LogIdOf;

/// A trait that lets `RaftCore` send a result back to the client or to somewhere else.
///
/// This is a generic abstraction for sending results of any type `T`.
/// Usually an implementation of [`Responder`] is a oneshot channel Sender.
///
/// ## Lifecycle Callbacks
///
/// - [`on_commit()`](Self::on_commit) is called when the entry is locally committed. Implementing
///   this callback is optional, and receiving it does not guarantee a later completion callback.
/// - [`on_complete()`](Self::on_complete) delivers the result after the state machine applies the
///   entry, or an error if the request is rejected. Rejection can happen without a commit callback.
///
/// The responder can be dropped without a result, before or after the commit callback. This can
/// happen during shutdown or after a fatal error, but work already handed off to the state machine
/// may still complete after the Raft core shuts down. Custom implementations should release or
/// notify any waiter when dropped, for example by owning a channel sender whose drop closes the
/// channel.
///
/// # Type Parameters
///
/// - `T`: The type of value to send through this responder
pub trait Responder<C, T>
where
    Self: OptionalSend + Sized + 'static,
    C: RaftTypeConfig,
{
    /// Called when the log entry is locally committed.
    ///
    /// Invoked when the log has been replicated to a quorum. Future leaders must preserve the
    /// entry, but it may not yet be visible to state-machine queries. Ordinary linearizable reads
    /// still wait for their committed read boundary to be applied.
    ///
    /// Do not acknowledge a successful write from this notification when using relaxed reads
    /// ([`LinearizerOption::with_relaxed`](crate::raft::linearizable_read::LinearizerOption::with_relaxed)). State-machine reads after the write response must include the
    /// entry's updates.
    ///
    /// # Parameters
    ///
    /// - `log_id`: The log ID assigned by the proposing leader.
    ///
    /// Default implementation does nothing.
    #[since(version = "0.10.0")]
    fn on_commit(&mut self, _log_id: LogIdOf<C>) {}

    /// Consumes the responder to deliver the final result.
    ///
    /// This callback is not guaranteed to run. A missing reply means no result arrived; it does
    /// not undo a known commit or prove that the write was not applied.
    #[since(version = "0.10.0")]
    fn on_complete(self, result: T);
}
