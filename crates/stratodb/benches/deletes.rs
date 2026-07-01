//! Deletion benchmarks: removing a whole entity (a cascading delete of its
//! subtree). `iter_batched` inserts the victim entity in the untimed setup step,
//! so only the `remove` + `commit` is measured. A variant runs against a table
//! with the two secondary indexes present, so the delete also pays index
//! maintenance.
//!
//! Run with: `cargo bench -p stratodb --features derive --bench deletes`.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use std::hint::black_box;

mod common;
use common::User;

fn deletes(c: &mut Criterion) {
    let mut group = c.benchmark_group("deletes");
    group.throughput(Throughput::Elements(1));

    for indexed in [false, true] {
        let label = if indexed {
            "remove_entity_indexed"
        } else {
            "remove_entity"
        };
        let (_db, table) = common::populated(0, indexed);
        let mut i = 0usize;

        group.bench_function(label, |b| {
            b.iter_batched(
                || {
                    // Untimed: insert a fresh, uniquely-keyed entity to delete.
                    let path = format!("users/{i}");
                    i += 1;

                    let w = table.write().expect("begin write");
                    w.store(&path, &User::sample(i)).expect("store");
                    w.commit().expect("commit");

                    path
                },
                |path| {
                    // Timed: the cascading remove and its commit.
                    let w = table.write().expect("begin write");
                    let removed = w.remove(&path).expect("remove");
                    w.commit().expect("commit");

                    black_box(removed);
                },
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

criterion_group!(benches, deletes);
criterion_main!(benches);
