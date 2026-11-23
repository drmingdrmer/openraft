use super::*;
use crate::engine::testing::UtClid;
use crate::engine::testing::log_id;

fn committed_leader_id(term: u64, node_id: u64) -> UtClid {
    *log_id(term, node_id, 0).committed_leader_id()
}

#[test]
fn test_empty_range() {
    // start == end means empty
    let iter = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 1), 5, 5);
    assert_eq!(iter.len(), 0);
    let ids: Vec<_> = iter.collect();
    assert!(ids.is_empty());
}

#[test]
fn test_single_element() {
    // [0, 1) is a single element at index 0
    let iter = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 2), 0, 1);
    assert_eq!(iter.len(), 1);
    let ids: Vec<_> = iter.collect();
    assert_eq!(ids, vec![log_id(1, 2, 0)]);
}

#[test]
fn test_forward_iteration() {
    let iter = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 2), 5, 8);
    let ids: Vec<_> = iter.collect();
    assert_eq!(ids, vec![log_id(1, 2, 5), log_id(1, 2, 6), log_id(1, 2, 7)]);
}

#[test]
fn test_backward_iteration() {
    let mut iter = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 2), 5, 8);

    assert_eq!(iter.next_back(), Some(log_id(1, 2, 7)));
    assert_eq!(iter.next_back(), Some(log_id(1, 2, 6)));
    assert_eq!(iter.next_back(), Some(log_id(1, 2, 5)));
    assert!(iter.next_back().is_none());
}

#[test]
fn test_mixed_iteration() {
    let mut iter = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 2), 5, 8);

    assert_eq!(iter.next(), Some(log_id(1, 2, 5)));
    assert_eq!(iter.next_back(), Some(log_id(1, 2, 7)));
    assert_eq!(iter.next(), Some(log_id(1, 2, 6)));
    assert!(iter.next().is_none());
    assert!(iter.next_back().is_none());
}

#[test]
fn test_start_at_zero_forward() {
    let iter = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 2), 0, 3);
    let ids: Vec<_> = iter.collect();
    assert_eq!(ids, vec![log_id(1, 2, 0), log_id(1, 2, 1), log_id(1, 2, 2)]);
}

#[test]
fn test_start_at_zero_backward() {
    let mut iter = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 2), 0, 3);

    assert_eq!(iter.next_back(), Some(log_id(1, 2, 2)));
    assert_eq!(iter.next_back(), Some(log_id(1, 2, 1)));
    assert_eq!(iter.next_back(), Some(log_id(1, 2, 0)));
    assert!(iter.next_back().is_none());
    assert!(iter.next().is_none());
}

#[test]
fn test_single_element_at_zero() {
    let mut iter = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 2), 0, 1);

    assert_eq!(iter.next_back(), Some(log_id(1, 2, 0)));
    assert!(iter.next_back().is_none());
    assert!(iter.next().is_none());
}

#[test]
fn test_exact_size() {
    let iter = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 1), 10, 15);
    assert_eq!(iter.len(), 5);

    let mut iter = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 1), 10, 15);
    assert_eq!(iter.len(), 5);
    iter.next();
    assert_eq!(iter.len(), 4);
    iter.next_back();
    assert_eq!(iter.len(), 3);
}

#[test]
fn test_clone() {
    let iter1 = LeaderLogIdsIter::<UtClid>::new(committed_leader_id(1, 1), 5, 8);
    let iter2 = iter1.clone();

    let ids1: Vec<_> = iter1.collect();
    let ids2: Vec<_> = iter2.collect();
    assert_eq!(ids1, ids2);
}
