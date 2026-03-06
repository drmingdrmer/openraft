use crate::RaftPrimitives;
use crate::SnapshotId;
use crate::type_config::alias::LogIdOf;

/// A small piece of information for identifying a snapshot and error tracing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub struct SnapshotSignature<P>
where P: RaftPrimitives
{
    /// Log entries up to which this snapshot includes, inclusive.
    pub last_log_id: Option<LogIdOf<P>>,

    /// The last applied membership log id.
    pub last_membership_log_id: Option<Box<LogIdOf<P>>>,

    /// To identify a snapshot when transferring.
    pub snapshot_id: SnapshotId,
}

#[cfg(test)]
mod tests {

    #[cfg(feature = "serde")]
    #[test]
    fn test_snapshot_signature_serde() {
        use super::SnapshotSignature;
        use crate::engine::testing::UTConfig;
        use crate::engine::testing::log_id;

        let sig = SnapshotSignature {
            last_log_id: Some(log_id(1, 2, 3)),
            last_membership_log_id: Some(Box::new(log_id(4, 5, 6))),
            snapshot_id: "test".to_string(),
        };
        let s = serde_json::to_string(&sig).unwrap();
        assert_eq!(
            s,
            r#"{"last_log_id":{"leader_id":{"term":1,"node_id":2},"index":3},"last_membership_log_id":{"leader_id":{"term":4,"node_id":5},"index":6},"snapshot_id":"test"}"#
        );
        let sig2: SnapshotSignature<UTConfig> = serde_json::from_str(&s).unwrap();
        assert_eq!(sig, sig2);
    }
}
