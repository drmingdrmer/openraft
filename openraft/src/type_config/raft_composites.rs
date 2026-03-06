use std::fmt::Debug;

use crate::OptionalSend;
use crate::OptionalSync;
use crate::entry::RaftEntry;
use crate::errors::ErrorSource;
use crate::raft::responder::Responder;
use crate::type_config::AsyncRuntime;
use crate::type_config::RaftPrimitives;
use crate::vote::raft_vote::RaftVote;

/// Composite types in the Raft type configuration.
///
/// `RaftComposites` contains types that reference primitive types via `Self::Prim`.
/// These include votes, log entries, snapshot data, async runtime, responders, and error types.
///
/// Uses composition (`Prim` associated type) rather than inheritance to reference primitives,
/// cleanly breaking circular type dependencies.
///
/// Any `C: RaftTypeConfig` automatically implements `RaftComposites` via a blanket impl.
pub trait RaftComposites:
    Sized + OptionalSend + OptionalSync + Debug + Clone + Copy + Default + Eq + PartialEq + Ord + PartialOrd + 'static
{
    /// The primitive types this composite configuration is built on.
    type Prim: RaftPrimitives;

    /// Raft vote type.
    type Vote: RaftVote<Self::Prim>;

    /// Raft log entry type.
    type Entry: RaftEntry<Self::Prim>;

    /// Snapshot data for exposing a snapshot for reading & writing.
    type SnapshotData: OptionalSend + 'static;

    /// Asynchronous runtime type.
    type AsyncRuntime: AsyncRuntime;

    /// Responder type for sending client write responses asynchronously.
    type Responder<T>: Responder<Self::Prim, T>
    where T: OptionalSend + 'static;

    /// Error wrapper type for storage and network errors.
    type ErrorSource: ErrorSource;
}

impl<C: crate::RaftTypeConfig> RaftComposites for C {
    type Prim = C;
    type Vote = C::Vote;
    type Entry = C::Entry;
    type SnapshotData = C::SnapshotData;
    type AsyncRuntime = C::AsyncRuntime;
    type Responder<T>
        = C::Responder<T>
    where T: OptionalSend + 'static;
    type ErrorSource = C::ErrorSource;
}
