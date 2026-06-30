//! Write benchmarks: storing a whole entity and putting a single scalar leaf,
//! each a transaction of its own (open → write → commit). The working set is a
//! bounded ring of keys, so these measure upsert cost (`store` replaces the
//! subtree) on a stable-sized table.
//!
//! Run with: `cargo bench -p stratodb --features derive --bench writes`.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;

mod common;
use common::{Ring, RING};

fn writes(c: &mut Criterion) {
    let mut group = c.benchmark_group("writes");
    group.throughput(Throughput::Elements(1));

    // Store a full entity (shredded into its leaves), committing each time.
    {
        let fixture = Ring::new(false);
        let mut i = 0usize;
        group.bench_function("store_entity", |b| {
            b.iter(|| {
                let k = i % RING;
                let w = fixture.table.write().expect("begin write");
                w.store(&fixture.paths[k], &fixture.users[k]).expect("store");
                w.commit().expect("commit");
                i += 1;
            })
        });
    }

    // Put a single scalar leaf, committing each time.
    {
        let fixture = Ring::new(false);
        let mut i = 0usize;
        group.bench_function("put_leaf", |b| {
            b.iter(|| {
                let k = i % RING;
                let w = fixture.table.write().expect("begin write");
                w.put(format!("{}/score", &fixture.paths[k]), &black_box(i as i64))
                    .expect("put");
                w.commit().expect("commit");
                i += 1;
            })
        });
    }

    group.finish();
}

criterion_group!(benches, writes);
criterion_main!(benches);
