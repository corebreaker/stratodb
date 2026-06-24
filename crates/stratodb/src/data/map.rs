//! `BTreeMap<String, _>` support: the [`SData`] impl plus the [`Map`]/[`MapMut`]
//! accessors.
//!
//! A map shreds into an object node whose field names are the map's keys and
//! whose child nodes hold the values (each value is addressable as `…/key`). An
//! empty map still materializes its object node, so the accessor always has a
//! key.

use super::{
    SData,
    refs::{SIdentifiable, SMut, SRef},
};

use crate::{
    access::{Reader, Writer},
    error::SdbResult,
    path::{SPath, Segment},
    Skey,
};

use std::{collections::BTreeMap, marker::PhantomData, sync::Arc};

impl<T: SData> SData for BTreeMap<String, T> {
    type Mut<'t> = MapMut<'t, T>;
    type Ref<'t> = Map<'t, T>;

    fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()> {
        writer.ensure_container(at, false)?;

        for (key, value) in self {
            value.store(writer, &at.child_name(key.as_str()))?;
        }

        Ok(())
    }

    fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self> {
        let Some(key) = reader.resolve(at)? else {
            return Ok(BTreeMap::new());
        };

        let mut map = BTreeMap::new();
        for name in reader.object_keys(key)? {
            let value = T::load(reader, &at.child_name(name.as_str()))?;
            map.insert(name, value);
        }

        Ok(map)
    }
}

/// Read accessor for a map (object) node.
pub struct Map<'t, T> {
    reader: Arc<dyn Reader + 't>,
    base:   SPath,
    key:    Skey,
    _type:  PhantomData<T>,
}

impl<'t, T: SData> Map<'t, T> {
    /// The keys present, in sorted order.
    pub fn keys(&self) -> SdbResult<Vec<String>> {
        self.reader.object_keys(self.key)
    }

    /// The number of entries.
    pub fn len(&self) -> SdbResult<usize> {
        Ok(self.keys()?.len())
    }

    /// Whether the map has no entries.
    pub fn is_empty(&self) -> SdbResult<bool> {
        Ok(self.len()? == 0)
    }

    /// Whether `key` is present.
    pub fn contains_key(&self, key: &str) -> SdbResult<bool> {
        let at = self.base.child_name(key);

        Ok(self
            .reader
            .child_cached(self.key, &Segment::Name(key.to_string()), &at)?
            .is_some())
    }

    /// A read accessor over the value for `key`, or `None` if absent.
    pub fn get(&self, key: &str) -> SdbResult<Option<T::Ref<'t>>> {
        let at = self.base.child_name(key);
        let Some(child) = self
            .reader
            .child_cached(self.key, &Segment::Name(key.to_string()), &at)?
        else {
            return Ok(None);
        };

        Ok(Some(<T::Ref<'t> as SRef<'t>>::open(
            Arc::clone(&self.reader),
            at,
            child,
        )))
    }
}

impl<'t, T> SIdentifiable for Map<'t, T> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

impl<'t, T> SRef<'t> for Map<'t, T> {
    fn open(reader: Arc<dyn Reader + 't>, base: SPath, key: Skey) -> Self {
        Self {
            reader,
            base,
            key,
            _type: PhantomData,
        }
    }
}

/// Write accessor for a map (object) node.
pub struct MapMut<'t, T> {
    writer: Arc<dyn Writer + 't>,
    base:   SPath,
    key:    Skey,
    _type:  PhantomData<T>,
}

impl<'t, T: SData> MapMut<'t, T> {
    /// The keys present, in sorted order.
    pub fn keys(&self) -> SdbResult<Vec<String>> {
        self.writer.object_keys(self.key)
    }

    /// The number of entries.
    pub fn len(&self) -> SdbResult<usize> {
        Ok(self.keys()?.len())
    }

    /// Whether the map has no entries.
    pub fn is_empty(&self) -> SdbResult<bool> {
        Ok(self.len()? == 0)
    }

    /// Whether `key` is present.
    pub fn contains_key(&self, key: &str) -> SdbResult<bool> {
        let at = self.base.child_name(key);

        Ok(self
            .writer
            .child_cached(self.key, &Segment::Name(key.to_string()), &at)?
            .is_some())
    }

    /// A write accessor over the value for `key`, or `None` if absent.
    pub fn get(&self, key: &str) -> SdbResult<Option<T::Mut<'t>>> {
        let at = self.base.child_name(key);
        let Some(child) = self
            .writer
            .child_cached(self.key, &Segment::Name(key.to_string()), &at)?
        else {
            return Ok(None);
        };

        Ok(Some(<T::Mut<'t> as SMut<'t>>::open(
            Arc::clone(&self.writer),
            at,
            child,
        )))
    }

    /// Inserts `value` under `key`, replacing any existing entry there.
    pub fn insert(&self, key: &str, value: &T) -> SdbResult<()> {
        let at = self.base.child_name(key);
        self.writer.remove(&at)?;

        value.store(&self.writer, &at)
    }

    /// Removes `key`, returning whether it was present.
    pub fn remove(&self, key: &str) -> SdbResult<bool> {
        self.writer.remove(&self.base.child_name(key))
    }
}

impl<'t, T> SIdentifiable for MapMut<'t, T> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

impl<'t, T> SMut<'t> for MapMut<'t, T> {
    fn open(writer: Arc<dyn Writer + 't>, base: SPath, key: Skey) -> Self {
        Self {
            writer,
            base,
            key,
            _type: PhantomData,
        }
    }
}
