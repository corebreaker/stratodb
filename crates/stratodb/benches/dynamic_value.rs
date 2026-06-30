//! Benchmarks for the dynamic-document and export features:
//!
//! - `Value` round-tripping: `load_value` (tree → `Value`) and `store_value` (`Value` → tree, with index maintenance),
//!   plus the in-memory `get_value` / `set_value` path-addressed accessors.
//! - export: rendering a stored subtree to JSON (compact and pretty) and YAML.
//!
//! Run with: `cargo bench -p stratodb --features derive --bench dynamic_value`.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use stratodb::{
    data::Scalar,
    export::{JsonExporter, YamlExporter},
    Value,
};

mod common;
use common::{DATASET, RING};

fn dynamic_value(c: &mut Criterion) {
    let (_db, table) = common::populated(DATASET, false);
    let r = table.read().expect("read txn");
    let mid = DATASET / 2;
    let entity = format!("users/{mid}");

    let mut group = c.benchmark_group("dynamic_value");
    group.throughput(Throughput::Elements(1));

    // Tree → Value (a faithful, owned copy of the subtree).
    group.bench_function("load_value", |b| {
        b.iter(|| {
            let v = r.load_value(black_box(&entity)).expect("load_value");
            black_box(v)
        })
    });

    // In-memory navigation: clone the subtree at a path out of a Value.
    {
        let value = r.load_value(&entity).expect("load_value").expect("present");
        group.bench_function("get_value", |b| {
            b.iter(|| black_box(value.get_value(black_box("name"))))
        });
    }

    // In-memory mutation: overwrite an existing leaf (bounded, no growth).
    {
        let mut value = r.load_value(&entity).expect("load_value").expect("present");
        let mut n = 0u32;
        group.bench_function("set_value", |b| {
            b.iter(|| {
                let ok = value.set_value("age", Value::Leaf(Scalar::U32(n)));
                n = n.wrapping_add(1);
                black_box(ok);
            })
        });
    }

    // Value → tree: decompose and store, replacing the subtree (with maintenance
    // skipped here — the table is un-indexed). Cycles a bounded ring of keys.
    {
        let (_wdb, wtable) = common::populated(RING, false);
        let value = r.load_value(&entity).expect("load_value").expect("present");
        let mut i = 0usize;
        group.bench_function("store_value", |b| {
            b.iter(|| {
                let w = wtable.write().expect("begin write");
                w.store_value(format!("users/{}", i % RING), &value)
                    .expect("store_value");
                w.commit().expect("commit");
                i += 1;
            })
        });
    }

    group.finish();
}

fn export(c: &mut Criterion) {
    let (_db, table) = common::populated(DATASET, false);
    let r = table.read().expect("read txn");
    let entity = format!("users/{}", DATASET / 2);

    let mut group = c.benchmark_group("export");
    group.throughput(Throughput::Elements(1));

    group.bench_function("json_compact", |b| {
        b.iter(|| {
            let s = r.export_to_json(black_box(&entity), None).expect("json");
            black_box(s)
        })
    });

    group.bench_function("json_pretty", |b| {
        b.iter(|| {
            let s = r.export_to_json(black_box(&entity), Some(2)).expect("json");
            black_box(s)
        })
    });

    group.bench_function("yaml", |b| {
        b.iter(|| {
            let s = r.export_to_yaml(black_box(&entity)).expect("yaml");
            black_box(s)
        })
    });

    group.finish();
}

criterion_group!(benches, dynamic_value, export);
criterion_main!(benches);
