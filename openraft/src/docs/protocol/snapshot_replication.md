# Replication with Snapshot

Snapshot replication is a special type of log replication that replicates all **committed** logs from index 0 up to a specific index.

Similar to append-entry:

- (1) If the logs in the snapshot match the logs already stored on a Follower/Learner, no action is taken.

- (2) If the logs conflict with the local logs, **ALL** non-committed logs will be deleted because it is unclear which logs are in conflict. Additionally, the effective membership must be reverted to a previous non-conflicting state.


## Deleting conflicting logs

If [`snapshot_meta.last_log_id`] conflicts with the local log:

Since the node with conflicting logs cannot become a leader:
If this node could become a leader, according to the Raft specification, it must contain all committed logs. However, the log entry at `last_applied.index` is not committed, so it can never become a leader.

However, it could still become a leader when more logs are received. At this point, the logs after [`snapshot_meta.last_log_id`] will be deleted. The logs before or equal to [`snapshot_meta.last_log_id`] will not be deleted.

Then, there is a chance that this node becomes the leader and uses these logs for replication.


### Deleting all non-committed logs

Here, the system truncates **ALL** non-committed logs because `snapshot_meta.last_log_id` is committed. If the local log ID conflicts with `snapshot_meta.last_log_id`, there must be a quorum that contains `snapshot_meta.last_log_id`. Hence, it is **safe to remove all logs** on this node.


### Cleaning conflicting logs after installing snapshot is not safe

It is not safe to remove the conflicting logs that are less than `snapshot_meta.last_log_id` after installing the snapshot.

If the node crashes, dirty logs may remain there. These logs might be forwarded to other nodes if this node becomes a leader.

[`snapshot_meta.last_log_id`]: `crate::storage::SnapshotMeta::last_log_id`