use super::*;
use crate::engine::testing::UtClid;
use crate::engine::testing::log_id;

fn committed_leader_id(term: u64, node_id: u64) -> UtClid {
    *log_id(term, node_id, 0).committed_leader_id()
}

#[test]
fn test_single_element() {
    // [5, 5] is a single element range
    let range = LeaderLogIds::<UtClid>::new(committed_leader_id(1, 2), 5, 5);
    assert_eq!(range.len(), 1);
    assert_eq!(range.get(5), log_id(1, 2, 5));
}

#[test]
fn test_multiple_elements() {
    // [10, 14] is 5 elements
    let range = LeaderLogIds::<UtClid>::new(committed_leader_id(2, 3), 10, 14);
    assert_eq!(range.len(), 5);
    assert_eq!(range.get(10), log_id(2, 3, 10));
    assert_eq!(range.get(14), log_id(2, 3, 14));
}

#[test]
fn test_iterator() {
    // [5, 7] is 3 elements: 5, 6, 7
    let range = LeaderLogIds::<UtClid>::new(committed_leader_id(1, 2), 5, 7);
    let ids: Vec<_> = range.into_iter().collect();
    assert_eq!(ids, vec![log_id(1, 2, 5), log_id(1, 2, 6), log_id(1, 2, 7)]);
}

#[test]
fn test_double_ended_iterator() {
    // [5, 7] is 3 elements: 5, 6, 7
    let range = LeaderLogIds::<UtClid>::new(committed_leader_id(1, 2), 5, 7);
    let mut iter = range.into_iter();

    assert_eq!(iter.next_back(), Some(log_id(1, 2, 7)));
    assert_eq!(iter.next(), Some(log_id(1, 2, 5)));
    assert_eq!(iter.next(), Some(log_id(1, 2, 6)));
    assert!(iter.next().is_none());
    assert!(iter.next_back().is_none());
}

#[test]
fn test_clone_iter() {
    let range = LeaderLogIds::<UtClid>::new(committed_leader_id(1, 2), 5, 7);
    let iter1 = range.into_iter();
    let iter2 = iter1.clone();

    let ids1: Vec<_> = iter1.collect();
    let ids2: Vec<_> = iter2.collect();
    assert_eq!(ids1, vec![log_id(1, 2, 5), log_id(1, 2, 6), log_id(1, 2, 7)]);
    assert_eq!(ids1, ids2);
}

#[test]
fn test_iterator_starting_at_zero() {
    // Edge case: range starting at index 0
    let range = LeaderLogIds::<UtClid>::new(committed_leader_id(1, 2), 0, 2);
    let mut iter = range.into_iter();

    // Consume from back first to test the edge case
    assert_eq!(iter.next_back(), Some(log_id(1, 2, 2)));
    assert_eq!(iter.next_back(), Some(log_id(1, 2, 1)));
    assert_eq!(iter.next_back(), Some(log_id(1, 2, 0)));

    // Iterator should be exhausted
    assert!(iter.next_back().is_none());
    assert!(iter.next().is_none());
}

#[test]
fn test_single_element_at_zero() {
    // Edge case: single element at index 0
    let range = LeaderLogIds::<UtClid>::new(committed_leader_id(1, 2), 0, 0);
    let mut iter = range.into_iter();

    assert_eq!(iter.next_back(), Some(log_id(1, 2, 0)));
    assert!(iter.next_back().is_none());
    assert!(iter.next().is_none());
}

#[test]
fn test_display() {
    let range = LeaderLogIds::<UtClid>::new(committed_leader_id(1, 1), 5, 7);
    assert_eq!(format!("{}", range), "T1-N1:[5, 7]");
}

#[test]
fn test_last_ref() {
    let range = LeaderLogIds::<UtClid>::new(committed_leader_id(1, 2), 5, 10);
    assert_eq!(range.last_ref().into_log_id(), log_id(1, 2, 10));
}

#[test]
fn test_ref_at() {
    let range = LeaderLogIds::<UtClid>::new(committed_leader_id(1, 2), 5, 10);

    assert_eq!(range.ref_at(5).into_log_id(), log_id(1, 2, 5));
    assert_eq!(range.ref_at(7).into_log_id(), log_id(1, 2, 7));
    assert_eq!(range.ref_at(10).into_log_id(), log_id(1, 2, 10));
}
