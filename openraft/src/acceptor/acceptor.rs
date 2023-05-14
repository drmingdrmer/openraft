use crate::acceptor::accepted::AcceptedBorrow;
use crate::acceptor::Accepted;
use crate::LeaderId;
use crate::LogId;
use crate::NodeId;

// TODO: rename fields and methods with paxos terms?
/// Defines Acceptor behaviors
pub(crate) trait Acceptor<NID: NodeId> {
    /// Accept a newer(and greater) `value`, replicated by a proposer identified with a `vballot`.
    fn accept(&mut self, vballot: LeaderId<NID>, value: Option<LogId<NID>>);

    /// Accept a newer(and greater) `value`, without changing the `vballot`.
    fn accept_value(&mut self, value: Option<LogId<NID>>);

    /// Get a reference to the accepted value.
    fn accepted_ref(&self) -> &Accepted<NID>;

    /// Returns an [`AcceptedBorrow`] instance that contains references.
    fn accepted_borrow(&self) -> AcceptedBorrow<NID>;
}
