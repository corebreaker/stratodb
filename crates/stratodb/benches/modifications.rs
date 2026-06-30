//! Modification benchmarks — the three ways to change a stored entity, so their
//! costs can be compared directly:
//!
//! - `accessor_set_field`: through the typed `StratoUserMut` accessor. This is StratoDB's zero-copy / in-place path —
//!   only the targeted leaf is rewritten, no sibling field is read or re-shredded. (The read half of the same accessor
//!   pair is `SRef`/`StratoUser`; mutation goes through its `SMut` half.)
//! - `put_field`: the same single-leaf update addressed by raw path, without an accessor.
//! - `load_update_store`: the `SData` read / update / write round-trip — recompose the whole entity, change a field in
//!   memory, then store it back (re-shredding every field).
//!
//! All three cycle a bounded ring of existing keys and commit each iteration.
//!
//! Run with: `cargo bench -p stratodb --features derive --bench modifications`.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;

mod common;
use common::{Ring, StratoUserMut, User, RING};

fn modifications(c: &mut Criterion) {
    let mut group = c.benchmark_group("modifications");
    group.throughput(Throughput::Elements(1));

    // In-place, single leaf, via the typed mutable accessor (zero-copy path).
    {
        let fixture = Ring::new(false);
        let mut i = 0usize;
        group.bench_function("accessor_set_field", |b| {
            b.iter(|| {
                let k = i % RING;
                let w = fixture.table.write().expect("begin write");
                // The temporary accessor is dropped at the end of the statement,
                // before `commit` consumes the transaction.
                w.fetch_mut::<StratoUserMut>(&fixture.paths[k])
                    .expect("fetch_mut")
                    .score_mut()
                    .expect("score_mut")
                    .set(&(i as i64))
                    .expect("set");
                w.commit().expect("commit");
                i += 1;
            })
        });
    }

    // The same single-leaf update, addressed by raw path.
    {
        let fixture = Ring::new(false);
        let mut i = 0usize;
        group.bench_function("put_field", |b| {
            b.iter(|| {
                let k = i % RING;
                let w = fixture.table.write().expect("begin write");
                w.put(format!("{}/score", &fixture.paths[k]), &(i as i64)).expect("put");
                w.commit().expect("commit");
                i += 1;
            })
        });
    }

    // Full SData read / update / write: load the entity, change a field, store it.
    {
        let fixture = Ring::new(false);
        let mut i = 0usize;
        group.bench_function("load_update_store", |b| {
            b.iter(|| {
                let k = i % RING;
                let w = fixture.table.write().expect("begin write");
                let mut u: User = w.load(&fixture.paths[k]).expect("load");
                u.score = i as i64;
                w.store(&fixture.paths[k], &black_box(u)).expect("store");
                w.commit().expect("commit");
                i += 1;
            })
        });
    }

    group.finish();
}

criterion_group!(benches, modifications);
criterion_main!(benches);
