//! List benchmarks: a list-bearing entity, exercising the operations whose cost
//! depends on how a `Vec` field is stored.
//!
//! A `Vec<T>` shreds into a list node with one addressable node per element, so
//! these track the shapes the flat scalar `User` benches miss:
//!
//! - `store` / `load` touch every element (whole-entity cost);
//! - `read_element` reads a single element — a packed entity navigates to it without materializing the rest;
//! - `update_element` updates one element under both storage regimes: **packed** (the default — one blob, so the write
//!   rewrites it) and **shredded** (an index reaching into the elements makes each its own node, so the write touches a
//!   single leaf, independent of the list length).
//!
//! Run with: `cargo bench -p stratodb --features derive --bench lists`.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use stratodb::{
    data::Scalar,
    index::{IndexColumn, IndexDef},
    path::SPath,
    SData,
    StratoDb,
    Table,
};

/// Elements in the list. Modest, because a shredded store appends element by
/// element (each append rewrites the list node, so building it is O(N^2)) and the
/// gate runs this bench once in test mode.
const N: usize = 128;

/// The element the single-element benches target.
const MID: usize = N / 2;

/// A list-bearing entity: one `String` list plus a scalar, representative of a
/// document with an embedded collection.
#[derive(SData, Clone)]
struct Doc {
    title: String,
    items: Vec<String>,
}

fn sample() -> Doc {
    Doc {
        title: "a document with an embedded list".to_string(),
        items: (0..N).map(|i| format!("item-number-{i}")).collect(),
    }
}

/// The path of the element the single-element benches target.
fn mid_path() -> String {
    format!("docs/0/items[{MID}]")
}

/// A table holding one packed `Doc` at `docs/0` (the default: no index reaches in).
fn packed() -> (StratoDb, Table) {
    let db = StratoDb::create_in_memory().expect("db");
    let table = db.open_table("docs").expect("table");

    let w = table.write().expect("write");
    w.store("docs/0", &sample()).expect("store");
    w.commit().expect("commit");

    (db, table)
}

/// A table holding one `Doc` at `docs/0` stored **shredded**: an index reaching
/// into the list elements (`docs/*/items/*`) forces the entity to shred into live
/// nodes rather than one packed blob.
fn shredded() -> (StratoDb, Table) {
    let db = StratoDb::create_in_memory().expect("db");
    let table = db.open_table("docs").expect("table");

    let index = IndexDef::new(
        "by_item".to_string(),
        "docs/*/items/*".to_string(),
        vec![IndexColumn::asc(SPath::root())],
        false,
    );
    table.create_index(&index).expect("create index");

    let w = table.write().expect("write");
    w.store("docs/0", &sample()).expect("store");
    w.commit().expect("commit");

    (db, table)
}

fn lists(c: &mut Criterion) {
    let mut group = c.benchmark_group("lists");
    group.throughput(Throughput::Elements(1));

    // Store the whole entity (packed), committing each time.
    {
        let (_db, table) = packed();
        let doc = sample();
        group.bench_function("store", |b| {
            b.iter(|| {
                let w = table.write().expect("write");
                w.store("docs/0", black_box(&doc)).expect("store");
                w.commit().expect("commit");
            })
        });
    }

    // Load the whole entity (all elements).
    {
        let (_db, table) = packed();
        group.bench_function("load", |b| {
            b.iter(|| {
                let r = table.read().expect("read");
                black_box(r.load::<Doc>(black_box("docs/0")).expect("load"));
            })
        });
    }

    // Read a single element — navigates to it without materializing the rest.
    {
        let (_db, table) = packed();
        let path = mid_path();
        group.bench_function("read_element", |b| {
            b.iter(|| {
                let r = table.read().expect("read");
                black_box(r.get::<String>(black_box(&path)).expect("get"));
            })
        });
    }

    // Update one element, packed: the entity is one blob, so this rewrites it.
    {
        let (_db, table) = packed();
        let path = mid_path();
        let mut i = 0u64;
        group.bench_function("update_element/packed", |b| {
            b.iter(|| {
                let w = table.write().expect("write");
                w.put_scalar(black_box(&path), Scalar::Str(format!("updated-{i}")))
                    .expect("put");
                w.commit().expect("commit");
                i += 1;
            })
        });
    }

    // Update one element, shredded: each element is its own node, so this touches
    // a single leaf — cost independent of the list length.
    {
        let (_db, table) = shredded();
        let path = mid_path();
        let mut i = 0u64;
        group.bench_function("update_element/shredded", |b| {
            b.iter(|| {
                let w = table.write().expect("write");
                w.put_scalar(black_box(&path), Scalar::Str(format!("updated-{i}")))
                    .expect("put");
                w.commit().expect("commit");
                i += 1;
            })
        });
    }

    group.finish();
}

criterion_group!(benches, lists);
criterion_main!(benches);
