use std::fmt::Debug;

use crate::AppData;
use crate::AppDataResponse;
use crate::Node;
use crate::NodeId;
use crate::OptionalSend;
use crate::OptionalSync;
use crate::errors::ErrorSource;
use crate::vote::RaftLeaderId;
use crate::vote::RaftTerm;

/// Primitive types in the Raft type configuration.
///
/// `RaftPrimitives` contains self-contained types with no cross-references to composite types.
/// These are the foundational building blocks: application data, node identifiers, terms,
/// and leader IDs.
///
/// Any `C: RaftTypeConfig` automatically implements `RaftPrimitives` via a blanket impl.
pub trait RaftPrimitives:
    Sized + OptionalSend + OptionalSync + Debug + Clone + Copy + Default + Eq + PartialEq + Ord + PartialOrd + 'static
{
    /// Application-specific request data passed to the state machine.
    type D: AppData;

    /// Application-specific response data returned by the state machine.
    type R: AppDataResponse;

    /// A Raft node's ID.
    type NodeId: NodeId;

    /// Raft application level node data.
    type Node: Node;

    /// Type representing a Raft term number.
    type Term: RaftTerm;

    /// A Leader identifier in a cluster.
    type LeaderId: RaftLeaderId<Self::Term, Self::NodeId>;

    /// Error wrapper type for storage and network errors.
    type ErrorSource: ErrorSource;
}

impl<C: crate::RaftTypeConfig> RaftPrimitives for C {
    type D = C::D;
    type R = C::R;
    type NodeId = C::NodeId;
    type Node = C::Node;
    type Term = C::Term;
    type LeaderId = C::LeaderId;
    type ErrorSource = C::ErrorSource;
}
