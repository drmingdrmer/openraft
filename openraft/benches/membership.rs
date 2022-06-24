use criterion::criterion_group;
use criterion::criterion_main;
use criterion::Criterion;
use maplit::btreeset;
use openraft::quorum::QuorumSet;
use openraft::Membership;

pub fn bench_membership_is_quorum(c: &mut Criterion) {
    c.bench_function("membership: 12345, ids: slice", |b| {
        let m = Membership::new(vec![btreeset! {1,2,3,4,5}], None);
        let x = [1, 2, 3, 6, 7];

        b.iter(|| m.is_quorum(x.iter()))
    });

    c.bench_function("membership: 12345_678, ids: slice", |b| {
        let m = Membership::new(vec![btreeset! {1,2,3,4,5}, btreeset! {6,7,8}], None);
        let x = [1, 2, 3, 6, 7];

        b.iter(|| m.is_quorum(x.iter()))
    });
}

criterion_group!(
    benches, //
    bench_membership_is_quorum
);
criterion_main!(benches);
