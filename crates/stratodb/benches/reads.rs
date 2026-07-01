//! Read benchmarks: scalar reads with and without a presence test, full-entity
//! recomposition, and a single-field read through the zero-copy typed accessor.
//!
//! Run with: `cargo bench -p stratodb --features derive --bench reads`.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;

mod common;
use common::{StratoUser, User, DATASET};

fn reads(c: &mut Criterion) {
    let (_db, table) = common::populated(DATASET, false);
    let r = table.read().expect("read txn");

    let mid = DATASET / 2;
    let entity = format!("users/{mid}");
    let leaf = format!("users/{mid}/age");
    let missing = format!("users/{}/age", DATASET + 1);

    let mut group = c.benchmark_group("reads");
    group.throughput(Throughput::Elements(1));

    // A scalar read straight to the leaf, no presence test.
    group.bench_function("get_scalar", |b| {
        b.iter(|| {
            let v: Option<u32> = r.get(black_box(&leaf)).expect("get");
            black_box(v)
        })
    });

    // The same read guarded by an explicit `exists` first (the "with presence
    // test" variant) — two resolutions instead of one.
    group.bench_function("get_scalar_checked", |b| {
        b.iter(|| {
            let v = if r.exists(black_box(&leaf)).expect("exists") {
                r.get::<u32>(black_box(&leaf)).expect("get")
            } else {
                None
            };

            black_box(v)
        })
    });

    // A presence test that resolves nothing (the absent-path fast exit).
    group.bench_function("exists_missing", |b| {
        b.iter(|| black_box(r.exists(black_box(&missing)).expect("exists")))
    });

    // Full entity recomposition: every field is resolved and decoded.
    group.bench_function("load_entity", |b| {
        b.iter(|| {
            let u: User = r.load(black_box(&entity)).expect("load");
            black_box(u)
        })
    });

    // One field through the typed read accessor: only that leaf is resolved and
    // decoded — no sibling field is touched.
    group.bench_function("accessor_one_field", |b| {
        b.iter(|| {
            let acc = r.fetch::<StratoUser>(black_box(&entity)).expect("fetch");

            black_box(acc.age().expect("age").get().expect("get"))
        })
    });

    group.finish();
}

criterion_group!(benches, reads);
criterion_main!(benches);
