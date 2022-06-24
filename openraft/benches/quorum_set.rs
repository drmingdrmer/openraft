use criterion::criterion_group;
use criterion::criterion_main;
use criterion::Criterion;
use maplit::btreeset;
use openraft::quorum::AsJoint;
use openraft::quorum::QuorumSet;

pub fn bench_is_quorum(c: &mut Criterion) {
    c.bench_function("quorum_set: slice, ids: slice", |b| {
        let m12345: &[usize] = &[1, 2, 3, 4, 5];
        let x = [1, 2, 3, 6, 7];

        b.iter(|| m12345.is_quorum(x.iter()))
    });

    c.bench_function("quorum_set: vec, ids: slice", |b| {
        let m12345 = vec![1, 2, 3, 4, 5];
        let x = [1, 2, 3, 6, 7];

        b.iter(|| m12345.is_quorum(x.iter()))
    });

    c.bench_function("quorum_set: btreeset, ids: slice", |b| {
        let m12345678 = btreeset! {1,2,3,4,5,6,7,8};
        let x = [1, 2, 3, 6, 7];

        b.iter(|| m12345678.is_quorum(x.iter()))
    });

    c.bench_function("quorum_set: vec of vec, ids: slice", |b| {
        let m12345_678 = vec![vec![1, 2, 3, 4, 5], vec![6, 7, 8]];
        let x = [1, 2, 3, 6, 7];

        b.iter(|| m12345_678.as_joint().is_quorum(x.iter()))
    });

    c.bench_function("quorum_set: vec of btreeset, ids: slice", |b| {
        let m12345_678 = vec![btreeset! {1,2,3,4,5}, btreeset! {6,7,8}];
        let x = [1, 2, 3, 6, 7];

        b.iter(|| m12345_678.as_joint().is_quorum(x.iter()))
    });

    c.bench_function("quorum_set: vec of btreeset, ids: btreeset", |b| {
        let m12345_678 = vec![btreeset! {1,2,3,4,5}, btreeset! {6,7,8}];
        let x = btreeset! {1,2,3,6,7};

        b.iter(|| m12345_678.as_joint().is_quorum(x.iter()))
    });
}

criterion_group!(
    benches, //
    bench_is_quorum
);
criterion_main!(benches);
