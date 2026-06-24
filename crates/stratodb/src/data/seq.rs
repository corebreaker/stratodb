//! `Vec` support: the [`SData`] impl plus the [`Seq`]/[`SeqMut`] accessors.
//!
//! A `Vec<T>` shreds into a list node with one child node per element (each
//! element is addressable as `…[i]`). An empty vector still materializes its
//! list node, so the accessor always has a key.

use super::{
    refs::{SIdentifiable, SMut, SRef},
    SData,
};

use crate::{
    access::{Reader, Writer},
    error::{SdbError, SdbResult},
    path::{SPath, Segment},
    Skey,
};

use std::{marker::PhantomData, ops::Range, sync::Arc};

impl<T: SData> SData for Vec<T> {
    type Mut<'t> = SeqMut<'t, T>;
    type Ref<'t> = Seq<'t, T>;

    fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()> {
        writer.ensure_container(at, true)?;

        for (index, item) in self.iter().enumerate() {
            item.store(writer, &at.child_index(index as u64))?;
        }

        Ok(())
    }

    fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self> {
        let Some(key) = reader.resolve(at)? else {
            return Ok(Vec::new());
        };

        let len = reader.len(key)?;
        let mut items = Vec::with_capacity(len);
        for index in 0..len {
            items.push(T::load(reader, &at.child_index(index as u64))?);
        }

        Ok(items)
    }
}

/// Read accessor for a list (`Vec`) node.
pub struct Seq<'t, T> {
    reader: Arc<dyn Reader + 't>,
    base:   SPath,
    key:    Skey,
    _type:  PhantomData<T>,
}

impl<'t, T: SData> Seq<'t, T> {
    /// The number of elements.
    pub fn len(&self) -> SdbResult<usize> {
        self.reader.len(self.key)
    }

    /// Whether the list has no elements.
    pub fn is_empty(&self) -> SdbResult<bool> {
        Ok(self.len()? == 0)
    }

    /// A read accessor over the element at `index`.
    pub fn get(&self, index: usize) -> SdbResult<T::Ref<'t>> {
        let at = self.base.child_index(index as u64);
        let found = self.reader.child_cached(self.key, &Segment::Index(index as u64), &at)?;

        let Some(key) = found else {
            // Fetched lazily, on the error path only: if it fails too, propagate
            // that error rather than reporting a misleading `len: 0`.
            let len = self.len()?;

            return Err(SdbError::IndexOutOfRange {
                path:  at,
                index: index as u64,
                len:   len as u64,
            });
        };

        Ok(<T::Ref<'t> as SRef<'t>>::open(Arc::clone(&self.reader), at, key))
    }

    /// An iterator over read accessors to the elements, front-to-back.
    ///
    /// Each item is fallible (`SdbResult<T::Ref>`, since a per-element read can
    /// fail). The iterator is double-ended, so the standard adapters work on it:
    /// `seq.iter()?.rev()`, `.fold(..)`, `.rfold(..)`, `.reduce(..)`,
    /// `.filter(..)`, `.map(..)`, etc.
    pub fn iter(&self) -> SdbResult<impl DoubleEndedIterator<Item = SdbResult<T::Ref<'t>>> + 't> {
        let len = self.len()?;
        let reader = Arc::clone(&self.reader);
        let base = self.base.clone();
        let key = self.key;

        Ok((0..len).map(move |index| {
            let at = base.child_index(index as u64);
            let child = reader
                .child_cached(key, &Segment::Index(index as u64), &at)?
                .ok_or_else(|| SdbError::IndexOutOfRange {
                    path:  at.clone(),
                    index: index as u64,
                    len:   len as u64,
                })?;

            Ok(<T::Ref<'t> as SRef<'t>>::open(Arc::clone(&reader), at, child))
        }))
    }

    /// A read accessor over the first element, or `None` if the list is empty.
    pub fn first(&self) -> SdbResult<Option<T::Ref<'t>>> {
        self.iter()?.next().transpose()
    }

    /// A read accessor over the last element, or `None` if the list is empty.
    pub fn last(&self) -> SdbResult<Option<T::Ref<'t>>> {
        self.iter()?.next_back().transpose()
    }

    /// Whether any element equals `value`.
    pub fn contains(&self, value: &T) -> SdbResult<bool>
    where
        T: PartialEq, {
        for index in 0..self.len()? {
            if &T::load(&self.reader, &self.base.child_index(index as u64))? == value {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

impl<'t, T> SIdentifiable for Seq<'t, T> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

impl<'t, T> SRef<'t> for Seq<'t, T> {
    fn open(reader: Arc<dyn Reader + 't>, base: SPath, key: Skey) -> Self {
        Self {
            reader,
            base,
            key,
            _type: PhantomData,
        }
    }
}

/// Write accessor for a list (`Vec`) node.
pub struct SeqMut<'t, T> {
    writer: Arc<dyn Writer + 't>,
    base:   SPath,
    key:    Skey,
    _type:  PhantomData<T>,
}

impl<'t, T: SData> SeqMut<'t, T> {
    /// The number of elements.
    pub fn len(&self) -> SdbResult<usize> {
        self.writer.len(self.key)
    }

    /// Whether the list has no elements.
    pub fn is_empty(&self) -> SdbResult<bool> {
        Ok(self.len()? == 0)
    }

    /// A write accessor over the element at `index`.
    pub fn get(&self, index: usize) -> SdbResult<T::Mut<'t>> {
        let at = self.base.child_index(index as u64);
        let found = self.writer.child_cached(self.key, &Segment::Index(index as u64), &at)?;

        let Some(key) = found else {
            // Fetched lazily, on the error path only: if it fails too, propagate
            // that error rather than reporting a misleading `len: 0`.
            let len = self.len()?;

            return Err(SdbError::IndexOutOfRange {
                path:  at,
                index: index as u64,
                len:   len as u64,
            });
        };

        Ok(<T::Mut<'t> as SMut<'t>>::open(Arc::clone(&self.writer), at, key))
    }

    /// Appends `value` to the end of the list.
    pub fn push(&self, value: &T) -> SdbResult<()> {
        let index = self.len()? as u64;

        value.store(&self.writer, &self.base.child_index(index))
    }

    /// Inserts `value` at `index`, shifting later elements to the right.
    ///
    /// Implemented as append-then-reorder: the new element is materialized at the
    /// end, then moved into place (a single list-node rewrite, no shifting of the
    /// other elements' subtrees).
    pub fn insert_at(&self, index: usize, value: &T) -> SdbResult<()> {
        let end = self.len()?;
        value.store(&self.writer, &self.base.child_index(end as u64))?;

        self.writer.list_move(self.key, end, index)
    }

    /// Removes the element at `index`, returning whether it existed.
    pub fn remove_at(&self, index: usize) -> SdbResult<bool> {
        self.writer.remove(&self.base.child_index(index as u64))
    }

    /// Removes the elements in `range`, shifting later elements to the left.
    pub fn remove_range(&self, range: Range<usize>) -> SdbResult<()> {
        for _ in range.clone() {
            if !self.writer.remove(&self.base.child_index(range.start as u64))? {
                break;
            }
        }

        Ok(())
    }

    /// An iterator over write accessors to the elements, front-to-back.
    ///
    /// Like [`Seq::iter`](super::Seq::iter) but yields mutable accessors
    /// (`SdbResult<T::Mut>`); double-ended, so `rev`/`fold`/`rfold`/… apply.
    pub fn iter_mut(&self) -> SdbResult<impl DoubleEndedIterator<Item = SdbResult<T::Mut<'t>>> + 't> {
        let len = self.len()?;
        let writer = Arc::clone(&self.writer);
        let base = self.base.clone();
        let key = self.key;

        Ok((0..len).map(move |index| {
            let at = base.child_index(index as u64);
            let child = writer
                .child_cached(key, &Segment::Index(index as u64), &at)?
                .ok_or_else(|| SdbError::IndexOutOfRange {
                    path:  at.clone(),
                    index: index as u64,
                    len:   len as u64,
                })?;

            Ok(<T::Mut<'t> as SMut<'t>>::open(Arc::clone(&writer), at, child))
        }))
    }

    /// A write accessor over the first element, or `None` if the list is empty.
    pub fn first_mut(&self) -> SdbResult<Option<T::Mut<'t>>> {
        self.iter_mut()?.next().transpose()
    }

    /// A write accessor over the last element, or `None` if the list is empty.
    pub fn last_mut(&self) -> SdbResult<Option<T::Mut<'t>>> {
        self.iter_mut()?.next_back().transpose()
    }

    /// Whether any element equals `value`.
    pub fn contains(&self, value: &T) -> SdbResult<bool>
    where
        T: PartialEq, {
        for index in 0..self.len()? {
            if &T::load(&self.writer, &self.base.child_index(index as u64))? == value {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Swaps the elements at `i` and `j`.
    pub fn swap(&self, i: usize, j: usize) -> SdbResult<()> {
        self.writer.list_swap(self.key, i, j)
    }

    /// Removes the element at `index` by swapping it with the last element and
    /// truncating; returns the removed value, or `None` if `index` is out of range.
    /// Does not preserve element order (the former last element takes `index`).
    pub fn swap_remove(&self, index: usize) -> SdbResult<Option<T>> {
        let len = self.len()?;
        if index >= len {
            return Ok(None);
        }

        let value = T::load(&self.writer, &self.base.child_index(index as u64))?;
        let last = len - 1;
        if index != last {
            self.writer.list_swap(self.key, index, last)?;
        }
        self.writer.remove(&self.base.child_index(last as u64))?;

        Ok(Some(value))
    }

    /// Removes and returns the first element, or `None` if the list is empty.
    pub fn pop_first(&self) -> SdbResult<Option<T>> {
        if self.is_empty()? {
            return Ok(None);
        }

        let value = T::load(&self.writer, &self.base.child_index(0))?;
        self.writer.remove(&self.base.child_index(0))?;

        Ok(Some(value))
    }

    /// Removes and returns the last element, or `None` if the list is empty.
    pub fn pop_last(&self) -> SdbResult<Option<T>> {
        let len = self.len()?;
        if len == 0 {
            return Ok(None);
        }

        let last = len - 1;
        let value = T::load(&self.writer, &self.base.child_index(last as u64))?;
        self.writer.remove(&self.base.child_index(last as u64))?;

        Ok(Some(value))
    }

    /// Removes every element, leaving an empty list.
    pub fn clear(&self) -> SdbResult<()> {
        self.writer.clear_children(&self.base, self.key)
    }

    /// Keeps only the elements for which `keep` returns `true`. `keep` is called
    /// once per element, in order, on the recomposed value.
    pub fn retain(&self, mut keep: impl FnMut(&T) -> bool) -> SdbResult<()> {
        let len = self.len()?;
        let mut drop_indices = Vec::new();
        for index in 0..len {
            let value = T::load(&self.writer, &self.base.child_index(index as u64))?;
            if !keep(&value) {
                drop_indices.push(index);
            }
        }

        // Remove back-to-front so the surviving indices stay valid (and keep their keys).
        for &index in drop_indices.iter().rev() {
            self.writer.remove(&self.base.child_index(index as u64))?;
        }

        Ok(())
    }

    /// Removes the elements in `range` and returns them, in order.
    pub fn drain(&self, range: Range<usize>) -> SdbResult<Vec<T>> {
        let mut drained = Vec::with_capacity(range.len());
        for index in range.clone() {
            drained.push(T::load(&self.writer, &self.base.child_index(index as u64))?);
        }

        self.remove_range(range)?;

        Ok(drained)
    }

    /// Appends every item yielded by `iter` to the end of the list.
    pub fn extend<I: IntoIterator<Item = T>>(&self, iter: I) -> SdbResult<()> {
        for value in iter {
            self.push(&value)?;
        }

        Ok(())
    }
}

impl<'t, T> SIdentifiable for SeqMut<'t, T> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

impl<'t, T> SMut<'t> for SeqMut<'t, T> {
    fn open(writer: Arc<dyn Writer + 't>, base: SPath, key: Skey) -> Self {
        Self {
            writer,
            base,
            key,
            _type: PhantomData,
        }
    }
}
