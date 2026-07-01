//! Index benchmarks: the index-backed variants of the core operations, plus the
//! index-specific paths (lookup, prefix/reverse scan, and back-fill on creation).
//!
//! - lookups: `find` (exact, recomposing every hit) and a reverse prefix `query`.
//! - maintenance cost on mutation: storing an entity, updating an *indexed* column and removing an entity, each against
//!   a table carrying the two indexes — to be read against the un-indexed numbers in the other benches.
//! - `create_index_backfill`: building an index over an already-populated table.
//!
//! Run with: `cargo bench -p stratodb --features derive --bench indexes`.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use std::hint::black_box;
use stratodb::data::Scalar;

mod common;
use common::{Ring, StratoUserMut, User, DATASET, RING};

fn index_reads(c: &mut Criterion) {
    let (_db, table) = common::populated(DATASET, true);
    let r = table.read().expect("read txn");

    let mut group = c.benchmark_group("indexes/reads");
    group.throughput(Throughput::Elements(1));

    // Exact lookup on the non-unique `by_age`: ~DATASET/100 hits, each recomposed.
    group.bench_function("find_by_age", |b| {
        b.iter(|| {
            let hits: Vec<User> = r.find("by_age", &[Scalar::U32(42)]).expect("find");
            black_box(hits)
        })
    });

    // Unique lookup on `by_email`: a single hit.
    group.bench_function("find_by_email_unique", |b| {
        let email = format!("user{}@example.io", DATASET / 2);
        b.iter(|| {
            let hits: Vec<User> = r.find("by_email", &[Scalar::Str(email.clone())]).expect("find");

            black_box(hits)
        })
    });

    // A reverse-ordered prefix scan over the whole `by_age` index.
    group.bench_function("query_reversed_full", |b| {
        b.iter(|| {
            let hits: Vec<User> = r.query("by_age").reversed().run().expect("query");
            black_box(hits)
        })
    });

    group.finish();
}

fn index_mutations(c: &mut Criterion) {
    let mut group = c.benchmark_group("indexes/mutations");
    group.throughput(Throughput::Elements(1));

    // Store a full entity into an indexed table (delete-then-insert maintenance on
    // both indexes around the write).
    {
        let fixture = Ring::new(true);
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

    // Update an *indexed* column (`age`) through the accessor: the entity's
    // `by_age` entry is re-keyed by maintenance.
    {
        let fixture = Ring::new(true);
        let mut i = 0usize;
        group.bench_function("accessor_set_indexed_field", |b| {
            b.iter(|| {
                let k = i % RING;
                let w = fixture.table.write().expect("begin write");
                w.fetch_mut::<StratoUserMut>(&fixture.paths[k])
                    .expect("fetch_mut")
                    .age_mut()
                    .expect("age_mut")
                    .set(&((i % 100) as u32))
                    .expect("set");
                w.commit().expect("commit");
                i += 1;
            })
        });
    }

    group.finish();
}

fn index_deletes(c: &mut Criterion) {
    let (_db, table) = common::populated(0, true);
    let mut i = 0usize;

    let mut group = c.benchmark_group("indexes/deletes");
    group.throughput(Throughput::Elements(1));

    group.bench_function("remove_entity", |b| {
        b.iter_batched(
            || {
                let path = format!("users/{i}");
                i += 1;

                let w = table.write().expect("begin write");
                w.store(&path, &User::sample(i)).expect("store");
                w.commit().expect("commit");

                path
            },
            |path| {
                let w = table.write().expect("begin write");
                let removed = w.remove(&path).expect("remove");
                w.commit().expect("commit");

                black_box(removed);
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn index_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("indexes/build");

    // Building one index over an already-populated table: the back-fill cost. The
    // populated table is rebuilt untimed for each measured creation.
    let build = RING; // a modest table so the untimed setup stays quick
    group.bench_function("create_index_backfill", |b| {
        b.iter_batched(
            || common::populated(build, false),
            |(_db, table)| {
                table.create_indexes::<User>("users/*").expect("create indexes");

                black_box(table);
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, index_reads, index_mutations, index_deletes, index_build);
criterion_main!(benches);
