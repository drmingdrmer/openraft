use maplit::btreemap;
use maplit::btreeset;

use super::ChangeMembers;
use crate::Membership;
use crate::errors::EmptyMembership;
use crate::errors::MembershipError;
use crate::membership::RaftMembershipEdit;
use crate::membership::RaftMembershipEditOps;

#[test]
fn learner_edit_preserves_membership_configurations() -> anyhow::Result<()> {
    let configs = [btreeset! {1, 2}, btreeset! {2, 3}, btreeset! {3, 4}];
    let nodes = btreemap! {1 => 10, 2 => 20, 3 => 30, 4 => 40, 5 => 50};
    for config_count in 1..=configs.len() {
        let configs = configs[configs.len() - config_count..].to_vec();
        let original = Membership::new(configs.clone(), nodes.clone())?;
        let membership = original.clone();

        for retain in [false, true] {
            tracing::info!(config_count, retain, "--- add a new learner without changing voters");
            {
                let mut expected_nodes = nodes.clone();
                expected_nodes.insert(6, 60);
                let expected = Membership::new(configs.clone(), expected_nodes)?;
                let actual = ChangeMembers::add_learner(6, 60).apply(&membership, retain)?;
                assert_eq!(expected, actual);
            }

            tracing::info!(config_count, retain, "--- re-add voters and learners");
            {
                for node_id in [1, 2, 4, 5] {
                    let edit = ChangeMembers::add_learner(node_id, 99);
                    assert_eq!(original, edit.apply(&membership, retain)?, "re-adding {node_id}");
                }
            }
        }
        assert_eq!(original, membership);
    }
    Ok(())
}

#[test]
fn learner_edit_in_batch_follows_order_and_finalizes_joint_config() -> anyhow::Result<()> {
    let joint = Membership::new(
        vec![btreeset! {1, 2}, btreeset! {2, 3}],
        btreemap! {1 => 10, 2 => 20, 3 => 30, 4 => 40},
    )?;

    for retain in [false, true] {
        tracing::info!(retain, "--- apply an ordered learner batch on the joint config");
        {
            let nested = ChangeMembers::Batch(vec![
                ChangeMembers::add_learner(4, 41),
                ChangeMembers::add_learner(5, 50),
            ]);
            let edit = ChangeMembers::Batch(vec![
                ChangeMembers::RemoveNodes(btreeset! {4}),
                nested,
                ChangeMembers::SetNodes(btreemap! {5 => 51}),
            ]);
            let mut expected_nodes = btreemap! {2 => 20, 3 => 30, 4 => 41, 5 => 51};
            if retain {
                expected_nodes.insert(1, 10);
            }
            let expected = Membership::new(vec![btreeset! {2, 3}], expected_nodes)?;
            assert_eq!(expected, edit.apply(&joint, retain)?);
        }
    }
    Ok(())
}

#[test]
fn nested_batch_applies_edits_in_order_before_finishing_target() {
    let add = ChangeMembers::AddNodes(btreemap! {1 => 10, 2 => 20});
    let voters = ChangeMembers::ReplaceAllVoters(btreeset! {1, 2});
    let update = ChangeMembers::SetNodes(btreemap! {2 => 21});
    let nested = ChangeMembers::Batch(vec![voters, update]);
    let remove = ChangeMembers::RemoveVoters(btreeset! {1});
    let edit = ChangeMembers::Batch(vec![add, nested, remove]);
    let membership = Membership::<u64, u64>::default();
    let actual = edit.apply(&membership, true);
    let expected = Membership::new(vec![btreeset! {2}], btreemap! {1 => 10, 2 => 21});
    let expected = expected.unwrap();
    let expected = Ok(expected);
    assert_eq!(expected, actual);
}

#[test]
fn node_edit_without_config_returns_empty_membership() {
    let membership = Membership::<u64, u64>::default();
    let expected = Err(MembershipError::EmptyMembership(EmptyMembership {}));
    for edit in [
        ChangeMembers::AddNodes(btreemap! {1 => 10}),
        ChangeMembers::add_learner(1, 10),
    ] {
        assert_eq!(expected, edit.apply(&membership, true));
    }
}

#[test]
fn batch_retention_uses_the_original_and_final_voters() -> anyhow::Result<()> {
    let nodes = btreemap! {1 => 10, 2 => 20, 3 => 30, 4 => 40};
    let membership = Membership::new(vec![btreeset! {1, 2}], nodes)?;
    let remove = ChangeMembers::RemoveVoters(btreeset! {1, 2});
    let promote = ChangeMembers::AddVoterIds(btreeset! {2, 3});
    let demote = ChangeMembers::RemoveVoters(btreeset! {3});
    let nested = ChangeMembers::Batch(vec![promote, demote]);
    let change = ChangeMembers::Batch(vec![remove, nested]);

    let result = change.apply(&membership, false)?;

    let expected = Membership::new(vec![btreeset! {2}], btreemap! {2 => 20, 3 => 30, 4 => 40})?;
    assert_eq!(expected, result);
    Ok(())
}

#[test]
fn test_membership_change() -> anyhow::Result<()> {
    let m = || Membership::<u64, ()> {
        configs: vec![btreeset! {1,2}],
        nodes: btreemap! {1=>(),2=>(),3=>()},
    };

    // Add: no such learner; `next_membership()` rejects this result
    {
        let membership = m();
        let change = ChangeMembers::AddVoterIds(btreeset! {4});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {1,2,4}],
                nodes: btreemap! {1=>(),2=>(),3=>()}
            }),
            res
        );
    }

    // Add: ok
    {
        let membership = m();
        let change = ChangeMembers::AddVoterIds(btreeset! {3});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {1,2,3}],
                nodes: btreemap! {1=>(),2=>(),3=>()}
            }),
            res
        );
    }

    // AddVoters
    {
        let membership = m();
        let change = ChangeMembers::AddVoters(btreemap! {5=>()});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {1,2,5}],
                nodes: btreemap! {1=>(),2=>(),3=>(),5=>()}
            }),
            res
        );
    }

    // Remove: no such voter
    {
        let membership = m();
        let change = ChangeMembers::RemoveVoters(btreeset! {5});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {1,2}],
                nodes: btreemap! {1=>(),2=>(),3=>()}
            }),
            res
        );
    }

    // Remove: become empty; `next_membership()` rejects this result
    {
        let membership = m();
        let change = ChangeMembers::RemoveVoters(btreeset! {1,2});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {}],
                nodes: btreemap! {1=>(),2=>(),3=>()}
            }),
            res
        );
    }

    // Remove: OK retain
    {
        let membership = m();
        let change = ChangeMembers::RemoveVoters(btreeset! {1});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {2}],
                nodes: btreemap! {1=>(),2=>(),3=>()}
            }),
            res
        );
    }

    // Remove: OK, not retain; learner not removed
    {
        let membership = m();
        let change = ChangeMembers::RemoveVoters(btreeset! {1});
        let res = change.apply(&membership, false)?;
        assert_eq!(
            Membership::<u64, ()> {
                configs: vec![btreeset! {2}],
                nodes: btreemap! {2=>(),3=>()}
            },
            res
        );
    }

    // Remove: OK, not retain; learner removed
    {
        let mem = Membership::<u64, ()> {
            configs: vec![btreeset! {1,2}, btreeset! {2}],
            nodes: btreemap! {1=>(),2=>(),3=>()},
        };
        let change = ChangeMembers::RemoveVoters(btreeset! {1});
        let res = change.apply(&mem, false)?;
        assert_eq!(
            Membership::<u64, ()> {
                configs: vec![btreeset! {2}],
                nodes: btreemap! {2=>(),3=>()}
            },
            res
        );
    }

    // Replace:
    {
        let membership = m();
        let change = ChangeMembers::ReplaceAllVoters(btreeset! {2});
        let res = change.apply(&membership, false)?;
        assert_eq!(
            Membership::<u64, ()> {
                configs: vec![btreeset! {2}],
                nodes: btreemap! {2=>(),3=>()}
            },
            res
        );
    }

    // AddNodes: existent voter
    {
        let membership = m();
        let change = ChangeMembers::AddNodes(btreemap! {2=>()});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {1,2}],
                nodes: btreemap! {1=>(),2=>(),3=>()}
            }),
            res
        );
    }

    // AddNodes: existent learner
    {
        let membership = m();
        let change = ChangeMembers::AddNodes(btreemap! {3=>()});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {1,2}],
                nodes: btreemap! {1=>(),2=>(),3=>()}
            }),
            res
        );
    }

    // AddNodes: Ok
    {
        let membership = m();
        let change = ChangeMembers::AddNodes(btreemap! {4=>()});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {1,2}],
                nodes: btreemap! {1=>(),2=>(),3=>(), 4=>()}
            }),
            res
        );
    }

    // SetNodes: Ok
    {
        let m = || Membership::<u64, u64> {
            configs: vec![btreeset! {1,2}],
            nodes: btreemap! {1=>1,2=>2,3=>3},
        };

        let membership = m();
        let change = ChangeMembers::SetNodes(btreemap! {3=>30, 4=>40});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, u64> {
                configs: vec![btreeset! {1,2}],
                nodes: btreemap! {1=>1,2=>2,3=>30, 4=>40}
            }),
            res
        );
    }

    // RemoveNodes: cannot remove node for voter; `next_membership()` rejects this result
    {
        let membership = m();
        let change = ChangeMembers::RemoveNodes(btreeset! {2});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {1,2}],
                nodes: btreemap! {1=>(),3=>()}
            }),
            res
        );
    }

    // RemoveNodes: Ok
    {
        let membership = m();
        let change = ChangeMembers::RemoveNodes(btreeset! {3});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {1,2}],
                nodes: btreemap! {1=>(),2=>()}
            }),
            res
        );
    }

    // ReplaceAllNodes: Ok
    {
        let membership = m();
        let change = ChangeMembers::ReplaceAllNodes(btreemap! {1=>(),2=>(),4=>()});
        let res = change.apply(&membership, true);
        assert_eq!(
            Ok(Membership::<u64, ()> {
                configs: vec![btreeset! {1,2}],
                nodes: btreemap! {1=>(),2=>(),4=>()}
            }),
            res
        );
    }

    Ok(())
}

#[test]
fn edit_of_joint_respects_retention_and_preserves_learner_metadata() -> anyhow::Result<()> {
    let nodes = btreemap! {1 => 10, 2 => 20, 3 => 30, 4 => 40, 5 => 50, 6 => 60};
    let joint = Membership::<u64, u64>::new(vec![btreeset! {1, 2, 3}, btreeset! {3, 4, 5}], nodes.clone())?;
    for retain in [true, false] {
        let change = ChangeMembers::<u64, u64>::RemoveVoters(btreeset! {3});
        let result = change.apply(&joint, retain)?;
        let target_nodes = if retain {
            nodes.clone()
        } else {
            btreemap! {4 => 40, 5 => 50, 6 => 60}
        };
        let expected = Membership::new(vec![btreeset! {4, 5}], target_nodes)?;
        assert_eq!(expected, result);
    }
    Ok(())
}

/// Test membership change described by a batch operation.
///
/// The batch operations add one voter and remove another.
#[test]
fn test_membership_change_batch() -> anyhow::Result<()> {
    let m = || Membership::<u64, ()> {
        configs: vec![btreeset! {1,2,3}],
        nodes: btreemap! {1=>(),2=>(),3=>()},
    };

    let rm_2_add_5 = || {
        ChangeMembers::Batch(vec![
            ChangeMembers::RemoveVoters(btreeset! {2}),
            ChangeMembers::AddVoters(btreemap! {5=>()}),
        ])
    };

    let membership = m();
    let change = rm_2_add_5();
    let goal = change.apply(&membership, true)?;

    assert_eq!(goal, Membership::<u64, ()> {
        configs: vec![btreeset! {1,3,5}],
        nodes: btreemap! {1=>(),2=>(),3=>(),5=>()}
    });

    Ok(())
}
