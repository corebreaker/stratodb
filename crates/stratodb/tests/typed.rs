//! Validates the typed `SData` contract (store/fetch/load + eager-key accessors)
//! with hand-written accessors that mirror what `#[derive(SData)]` will emit.

use stratodb::{
    access::{Reader, Writer},
    data::{
        leaf::Leaf,
        refs::{SIdentifiable, SRef, SMut},
        SData,
    },
    path::{SPath, Segment},
    SdbError,
    SdbResult,
    Skey,
    StratoDb,
};

use std::sync::Arc;

#[derive(Debug, PartialEq)]
struct Inner {
    y: i64,
}

#[derive(Debug, PartialEq)]
struct Sample {
    x:     u32,
    inner: Inner,
}

// ---- hand-written derive output for `Inner` ------------------------------

impl SData for Inner {
    type Mut<'t> = StratoInnerMut<'t>;
    type Ref<'t> = StratoInner<'t>;

    fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()> {
        self.y.store(writer, &at.child_name("y"))
    }

    fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self> {
        Ok(Inner {
            y: i64::load(reader, &at.child_name("y"))?,
        })
    }
}

struct StratoInner<'t> {
    reader: Arc<dyn Reader + 't>,
    base:   SPath,
    key:    Skey,
}

impl<'t> StratoInner<'t> {
    fn y(&self) -> SdbResult<Leaf<'t, i64>> {
        let at = self.base.child_name("y");
        let key = self
            .reader
            .child_cached(self.key, &Segment::Name("y".into()), &at)?
            .ok_or_else(|| SdbError::PathNotFound(at.clone()))?;

        Ok(SRef::open(Arc::clone(&self.reader), at, key))
    }
}

impl<'t> SRef<'t> for StratoInner<'t> {
    fn open(reader: Arc<dyn Reader + 't>, base: SPath, key: Skey) -> Self {
        Self {
            reader,
            base,
            key,
        }
    }
}

impl<'t> SIdentifiable for StratoInner<'t> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

struct StratoInnerMut<'t> {
    writer: Arc<dyn Writer + 't>,
    base:   SPath,
    key:    Skey,
}

impl<'t> StratoInnerMut<'t> {
    fn set_y(&self, value: i64) -> SdbResult<()> {
        value.store(&self.writer, &self.base.child_name("y"))
    }
}

impl<'t> SMut<'t> for StratoInnerMut<'t> {
    fn open(writer: Arc<dyn Writer + 't>, base: SPath, key: Skey) -> Self {
        Self {
            writer,
            base,
            key,
        }
    }
}

impl<'t> SIdentifiable for StratoInnerMut<'t> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

// ---- hand-written derive output for `Sample` -----------------------------

impl SData for Sample {
    type Mut<'t> = StratoSampleMut<'t>;
    type Ref<'t> = StratoSample<'t>;

    fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()> {
        self.x.store(writer, &at.child_name("x"))?;
        self.inner.store(writer, &at.child_name("inner"))?;
        Ok(())
    }

    fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self> {
        Ok(Sample {
            x:     u32::load(reader, &at.child_name("x"))?,
            inner: Inner::load(reader, &at.child_name("inner"))?,
        })
    }
}

struct StratoSample<'t> {
    reader: Arc<dyn Reader + 't>,
    base:   SPath,
    key:    Skey,
}

impl<'t> StratoSample<'t> {
    fn x(&self) -> SdbResult<Leaf<'t, u32>> {
        let at = self.base.child_name("x");
        let key = self
            .reader
            .child_cached(self.key, &Segment::Name("x".into()), &at)?
            .ok_or_else(|| SdbError::PathNotFound(at.clone()))?;

        Ok(SRef::open(Arc::clone(&self.reader), at, key))
    }

    fn inner(&self) -> SdbResult<StratoInner<'t>> {
        let at = self.base.child_name("inner");
        let key = self
            .reader
            .child_cached(self.key, &Segment::Name("inner".into()), &at)?
            .ok_or_else(|| SdbError::PathNotFound(at.clone()))?;

        Ok(SRef::open(Arc::clone(&self.reader), at, key))
    }
}

impl<'t> SRef<'t> for StratoSample<'t> {
    fn open(reader: Arc<dyn Reader + 't>, base: SPath, key: Skey) -> Self {
        Self {
            reader,
            base,
            key,
        }
    }
}

impl<'t> SIdentifiable for StratoSample<'t> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

struct StratoSampleMut<'t> {
    writer: Arc<dyn Writer + 't>,
    base:   SPath,
    key:    Skey,
}

impl<'t> StratoSampleMut<'t> {
    fn set_x(&self, value: u32) -> SdbResult<()> {
        value.store(&self.writer, &self.base.child_name("x"))
    }

    fn inner_mut(&self) -> SdbResult<StratoInnerMut<'t>> {
        let at = self.base.child_name("inner");
        let key = self
            .writer
            .child_cached(self.key, &Segment::Name("inner".into()), &at)?
            .ok_or_else(|| SdbError::PathNotFound(at.clone()))?;

        Ok(SMut::open(Arc::clone(&self.writer), at, key))
    }
}

impl<'t> SMut<'t> for StratoSampleMut<'t> {
    fn open(writer: Arc<dyn Writer + 't>, base: SPath, key: Skey) -> Self {
        Self {
            writer,
            base,
            key,
        }
    }
}

impl<'t> SIdentifiable for StratoSampleMut<'t> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

#[test]
fn store_fetch_load_roundtrip() {
    let db = StratoDb::create_in_memory().unwrap();
    let table = db.open_table("data").unwrap();

    let sample = Sample {
        x:     7,
        inner: Inner {
            y: -3
        },
    };

    let w = table.write().unwrap();
    w.store("a/h", &sample).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();

    // Typed accessor: eager key (infallible), scalar read via `.get()`.
    let acc = r.fetch::<StratoSample>("a/h").unwrap();
    let _pk: Skey = acc.key();
    assert_eq!(acc.x().unwrap().get().unwrap(), 7);
    assert_eq!(acc.inner().unwrap().y().unwrap().get().unwrap(), -3);

    // Full recomposition.
    let loaded: Sample = r.load("a/h").unwrap();
    assert_eq!(loaded, sample);

    // Homogeneity: the shredded leaves are also reachable by raw path.
    assert_eq!(r.get::<u32>("a/h/x").unwrap(), Some(7));
    assert_eq!(r.get::<i64>("a/h/inner/y").unwrap(), Some(-3));
}

#[test]
fn fetch_mut_exposes_pk() {
    let db = StratoDb::create_in_memory().unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    {
        w.store(
            "a/h",
            &Sample {
                x:     1,
                inner: Inner {
                    y: 2
                },
            },
        )
        .unwrap();

        let accessor = w.fetch_mut::<StratoSampleMut>("a/h").unwrap();
        let _pk: Skey = accessor.key();

        accessor.set_x(42).unwrap();
        accessor.inner_mut().unwrap().set_y(99).unwrap();

        let loaded: Sample = w.load("a/h").unwrap();
        assert_eq!(
            loaded,
            Sample {
                x:     42,
                inner: Inner {
                    y: 99
                },
            }
        );
    }

    w.commit().unwrap();
}

/// Child navigation is memoized through the shared path cache (redesign step 3),
/// so it must stay coherent: repeated hops return the same key, and a newer
/// generation never serves an older snapshot's cached child — nor the reverse.
#[test]
fn accessor_navigation_is_generation_coherent() {
    let db = StratoDb::create_in_memory().unwrap();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.store(
        "a/h",
        &Sample {
            x:     1,
            inner: Inner {
                y: 2
            },
        },
    )
    .unwrap();
    w.commit().unwrap();

    // On the first snapshot, navigating to `inner` twice is a cache hit the
    // second time and must yield the same stable key.
    let r1 = table.read().unwrap();
    let acc1 = r1.fetch::<StratoSample>("a/h").unwrap();
    let inner_key = acc1.inner().unwrap().key();
    assert_eq!(acc1.inner().unwrap().key(), inner_key);

    // Overwrite the whole subtree (which mints fresh keys) and bump the generation.
    let w = table.write().unwrap();
    w.store(
        "a/h",
        &Sample {
            x:     9,
            inner: Inner {
                y: 8
            },
        },
    )
    .unwrap();
    w.commit().unwrap();

    // A fresh reader navigates to the new node, never the cached old key.
    let r2 = table.read().unwrap();
    let acc2 = r2.fetch::<StratoSample>("a/h").unwrap();
    assert_ne!(acc2.inner().unwrap().key(), inner_key);
    assert_eq!(acc2.inner().unwrap().y().unwrap().get().unwrap(), 8);

    // The older snapshot still navigates to its own key and value.
    assert_eq!(acc1.inner().unwrap().key(), inner_key);
    assert_eq!(acc1.inner().unwrap().y().unwrap().get().unwrap(), 2);
}
