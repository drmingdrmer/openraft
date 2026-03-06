use std::fmt;

use crate::EntryPayload;
use crate::Membership;
use crate::RaftPrimitives;
use crate::entry::RaftEntry;
use crate::entry::RaftPayload;
use crate::type_config::alias::CommittedLeaderIdOf;
use crate::type_config::alias::LogIdOf;

/// A Raft log entry.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub struct Entry<P>
where P: RaftPrimitives
{
    /// The log ID uniquely identifying this entry.
    pub log_id: LogIdOf<P>,

    /// This entry's payload.
    pub payload: EntryPayload<P>,
}

impl<P> Clone for Entry<P>
where
    P: RaftPrimitives,
    P::D: Clone,
{
    fn clone(&self) -> Self {
        Self {
            log_id: self.log_id.clone(),
            payload: self.payload.clone(),
        }
    }
}

impl<P> fmt::Debug for Entry<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry").field("log_id", &self.log_id).field("payload", &self.payload).finish()
    }
}

impl<P> PartialEq for Entry<P>
where
    P::D: PartialEq,
    P: RaftPrimitives,
{
    fn eq(&self, other: &Self) -> bool {
        self.log_id == other.log_id && self.payload == other.payload
    }
}

impl<P> AsRef<Entry<P>> for Entry<P>
where P: RaftPrimitives
{
    fn as_ref(&self) -> &Entry<P> {
        self
    }
}

impl<P> fmt::Display for Entry<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.log_id, self.payload)
    }
}

impl<P> RaftPayload<P> for Entry<P>
where P: RaftPrimitives
{
    fn get_membership(&self) -> Option<Membership<P>> {
        self.payload.get_membership()
    }
}

impl<P> RaftEntry<P> for Entry<P>
where P: RaftPrimitives
{
    fn new(log_id: LogIdOf<P>, payload: EntryPayload<P>) -> Self {
        Self { log_id, payload }
    }

    fn log_id_parts(&self) -> (&CommittedLeaderIdOf<P>, u64) {
        (&self.log_id.leader_id, self.log_id.index)
    }

    fn set_log_id(&mut self, new: LogIdOf<P>) {
        self.log_id = new;
    }
}
