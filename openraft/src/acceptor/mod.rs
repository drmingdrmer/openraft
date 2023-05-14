//! Acceptor is a process passively accept value proposed by a granted proposer(leader).

mod accepted;
mod acceptor;

pub(crate) use accepted::Accepted;
pub(crate) use accepted::AcceptedBorrow;
pub(crate) use acceptor::Acceptor;
