use openraft::type_config::base_config::RaftBaseConfig;
use openraft::RaftTypeConfig;
use openraft::SnapshotMeta;

#[derive(Debug, Clone, Copy, Default, PartialOrd, Ord, PartialEq, Eq)]
struct BaseConfig;

impl RaftBaseConfig for BaseConfig {
    type Term = u64;
    type NodeId = u64;
    type D = ();
    type R = ();
    type Node = ();
}

#[derive(Debug, Clone, Copy, Default, PartialOrd, Ord, PartialEq, Eq)]
struct TypeConfig;

impl RaftTypeConfig for TypeConfig {
    type D = ();
    type R = ();
    type NodeId = u64;
    type Node = ();
    type Term = u64;
    type LeaderId = openraft::impls::leader_id_adv::LeaderId<BaseConfig>;
    type Vote = openraft::impls::Vote<Self>;
    type Entry = openraft::impls::Entry<Self>;
    type SnapshotData = SnapshotData;
    type AsyncRuntime = openraft::TokioRuntime;
    type Responder = openraft::impls::OneshotResponder<Self>;
    type BaseConfig = BaseConfig;
}

pub struct SnapshotData {
    meta: SnapshotMeta<BaseConfig>,
}

#[cfg(test)]
mod tests {
    #[test]
    fn foo() {}
}
