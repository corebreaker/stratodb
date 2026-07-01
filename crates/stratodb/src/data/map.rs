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
    error::{SdbError, SdbResult},
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
            .child_cached(self.key, &Segment::Name(key.into()), &at)?
            .is_some())
    }

    /// A read accessor over the value for `key`, or `None` if absent.
    pub fn get(&self, key: &str) -> SdbResult<Option<T::Ref<'t>>> {
        let at = self.base.child_name(key);
        let Some(child) = self.reader.child_cached(self.key, &Segment::Name(key.into()), &at)? else {
            return Ok(None);
        };

        Ok(Some(<T::Ref<'t> as SRef<'t>>::open(
            Arc::clone(&self.reader),
            at,
            child,
        )))
    }

    /// An iterator over `(key, read accessor)` pairs, in ascending key order.
    ///
    /// Each item is fallible (`SdbResult<(String, T::Ref)>`). The iterator is
    /// double-ended, so the standard adapters apply: `map.iter()?.rev()`,
    /// `.fold(..)`, `.filter(..)`, `.map(..)`, etc.
    pub fn iter(&self) -> SdbResult<impl DoubleEndedIterator<Item = SdbResult<(String, T::Ref<'t>)>> + 't> {
        let names = self.keys()?;
        let reader = Arc::clone(&self.reader);
        let base = self.base.clone();
        let key = self.key;

        Ok(names.into_iter().map(move |name| {
            let at = base.child_name(name.as_str());
            let child = reader
                .child_cached(key, &Segment::Name(name.as_str().into()), &at)?
                .ok_or_else(|| SdbError::PathNotFound(at.clone()))?;

            Ok((name, <T::Ref<'t> as SRef<'t>>::open(Arc::clone(&reader), at, child)))
        }))
    }

    /// An iterator over read accessors to the values, in ascending key order.
    pub fn values(&self) -> SdbResult<impl DoubleEndedIterator<Item = SdbResult<T::Ref<'t>>> + 't> {
        Ok(self.iter()?.map(|item| item.map(|(_, value)| value)))
    }

    /// The first `(key, read accessor)` entry (smallest key), or `None` if empty.
    pub fn first(&self) -> SdbResult<Option<(String, T::Ref<'t>)>> {
        self.iter()?.next().transpose()
    }

    /// The last `(key, read accessor)` entry (largest key), or `None` if empty.
    pub fn last(&self) -> SdbResult<Option<(String, T::Ref<'t>)>> {
        self.iter()?.next_back().transpose()
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
            .child_cached(self.key, &Segment::Name(key.into()), &at)?
            .is_some())
    }

    /// A write accessor over the value for `key`, or `None` if absent.
    pub fn get(&self, key: &str) -> SdbResult<Option<T::Mut<'t>>> {
        let at = self.base.child_name(key);
        let Some(child) = self.writer.child_cached(self.key, &Segment::Name(key.into()), &at)? else {
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

    /// An iterator over `(key, write accessor)` pairs, in ascending key order.
    ///
    /// Like [`Map::iter`](super::Map::iter) but yields mutable value accessors;
    /// double-ended, so `rev`/`fold`/`filter`/… apply.
    pub fn iter_mut(&self) -> SdbResult<impl DoubleEndedIterator<Item = SdbResult<(String, T::Mut<'t>)>> + 't> {
        let names = self.keys()?;
        let writer = Arc::clone(&self.writer);
        let base = self.base.clone();
        let key = self.key;

        Ok(names.into_iter().map(move |name| {
            let at = base.child_name(name.as_str());
            let child = writer
                .child_cached(key, &Segment::Name(name.as_str().into()), &at)?
                .ok_or_else(|| SdbError::PathNotFound(at.clone()))?;

            Ok((name, <T::Mut<'t> as SMut<'t>>::open(Arc::clone(&writer), at, child)))
        }))
    }

    /// An iterator over write accessors to the values, in ascending key order.
    pub fn values_mut(&self) -> SdbResult<impl DoubleEndedIterator<Item = SdbResult<T::Mut<'t>>> + 't> {
        Ok(self.iter_mut()?.map(|item| item.map(|(_, value)| value)))
    }

    /// The first `(key, write accessor)` entry (smallest key), or `None` if empty.
    pub fn first_mut(&self) -> SdbResult<Option<(String, T::Mut<'t>)>> {
        self.iter_mut()?.next().transpose()
    }

    /// The last `(key, write accessor)` entry (largest key), or `None` if empty.
    pub fn last_mut(&self) -> SdbResult<Option<(String, T::Mut<'t>)>> {
        self.iter_mut()?.next_back().transpose()
    }

    /// Removes and returns the first entry (smallest key), or `None` if empty.
    pub fn pop_first(&self) -> SdbResult<Option<(String, T)>> {
        let Some(name) = self.keys()?.into_iter().next() else {
            return Ok(None);
        };

        let value = T::load(&self.writer, &self.base.child_name(name.as_str()))?;
        self.writer.remove(&self.base.child_name(name.as_str()))?;

        Ok(Some((name, value)))
    }

    /// Removes and returns the last entry (largest key), or `None` if empty.
    pub fn pop_last(&self) -> SdbResult<Option<(String, T)>> {
        let Some(name) = self.keys()?.into_iter().next_back() else {
            return Ok(None);
        };

        let value = T::load(&self.writer, &self.base.child_name(name.as_str()))?;
        self.writer.remove(&self.base.child_name(name.as_str()))?;

        Ok(Some((name, value)))
    }

    /// Removes every entry, leaving an empty map.
    pub fn clear(&self) -> SdbResult<()> {
        self.writer.clear_children(&self.base, self.key)
    }

    /// Keeps only the entries for which `keep` returns `true`. `keep` is called
    /// once per entry, in ascending key order, on the recomposed value.
    pub fn retain(&self, mut keep: impl FnMut(&str, &T) -> bool) -> SdbResult<()> {
        for name in self.keys()? {
            let value = T::load(&self.writer, &self.base.child_name(name.as_str()))?;
            if !keep(name.as_str(), &value) {
                self.writer.remove(&self.base.child_name(name.as_str()))?;
            }
        }

        Ok(())
    }

    /// Removes every entry and returns them, in ascending key order.
    pub fn drain(&self) -> SdbResult<BTreeMap<String, T>> {
        let mut drained = BTreeMap::new();
        for name in self.keys()? {
            let value = T::load(&self.writer, &self.base.child_name(name.as_str()))?;
            drained.insert(name, value);
        }

        self.writer.clear_children(&self.base, self.key)?;

        Ok(drained)
    }

    /// Inserts every `(key, value)` yielded by `iter`, replacing existing entries.
    pub fn extend<I: IntoIterator<Item = (String, T)>>(&self, iter: I) -> SdbResult<()> {
        for (name, value) in iter {
            self.insert(name.as_str(), &value)?;
        }

        Ok(())
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
