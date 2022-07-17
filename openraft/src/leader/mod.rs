#[allow(clippy::module_inception)] mod leader;
mod voting;

pub(crate) use leader::Leader;
pub(crate) use voting::Rejection;
pub(crate) use voting::Voting;
pub(crate) use voting::VotingResult;
