use std::fmt::Debug;

use crate::vote::RaftTerm;
use crate::AppData;
use crate::AppDataResponse;
use crate::Node;
use crate::NodeId;
use crate::OptionalSend;
use crate::OptionalSync;
use crate::RaftTypeConfig;

/// Basic requirements for a type to be used as a **config type** containing associated types.
///
/// A config type is typically a zero-sized struct that defines the associated types used throughout
/// the Raft implementation. Since it will be used as a type parameter in many other types, it must
/// implement these common traits to enable `#[derive]` to work properly on the types that depend on
/// it.
pub trait TypeConfigBase
where
    Self: Sized + OptionalSend + OptionalSync + 'static,
    Self: Debug + Clone + Copy + Default,
    Self: Eq + PartialEq + Ord + PartialOrd,
{
}

impl<T> TypeConfigBase for T
where
    T: Sized + OptionalSend + OptionalSync + 'static,
    T: Debug + Clone + Copy + Default,
    T: Eq + PartialEq + Ord + PartialOrd,
{
}

/// A type config trait that defines the primitive, non-configurable types used in the Raft
/// implementation.
///
/// This trait specifies the following associated types:
/// - `AppData`: The type of data that can be replicated through the Raft protocol
/// - `AppDataResponse`: The response type returned when applying `AppData` to the state machine
/// - `NodeId`: The type used to uniquely identify nodes in the Raft cluster
/// - `Node`: The type containing node metadata and connection information
/// - `Term`: The type representing election terms in the Raft protocol
pub trait RaftBaseConfig: TypeConfigBase {
    /// Application-specific request data passed to the state machine.
    type D: AppData;

    /// Application-specific response data returned by the state machine.
    type R: AppDataResponse;

    /// A Raft node's ID.
    type NodeId: NodeId;

    /// Raft application level node data
    type Node: Node;

    /// Type representing a Raft term number.
    ///
    /// A term is a logical clock in Raft that is used to detect obsolete information,
    /// such as old leaders. It must be totally ordered and monotonically increasing.
    ///
    /// Common implementations are provided for standard integer types like `u64`, `i64` etc.
    ///
    /// See: [`RaftTerm`] for the required methods.
    type Term: RaftTerm;
}

impl<T> RaftBaseConfig for T
where T: RaftTypeConfig
{
    type D = <Self as RaftTypeConfig>::D;
    type R = <Self as RaftTypeConfig>::R;
    type NodeId = <Self as RaftTypeConfig>::NodeId;
    type Node = <Self as RaftTypeConfig>::Node;
    type Term = <Self as RaftTypeConfig>::Term;
}
