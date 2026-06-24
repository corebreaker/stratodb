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
        let key = self
            .reader
            .child(self.key, &Segment::Index(index as u64))?
            .ok_or_else(|| SdbError::IndexOutOfRange {
                path:  at.clone(),
                index: index as u64,
                len:   self.len().unwrap_or(0) as u64,
            })?;

        Ok(<T::Ref<'t> as SRef<'t>>::open(Arc::clone(&self.reader), at, key))
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
        let key = self
            .writer
            .child(self.key, &Segment::Index(index as u64))?
            .ok_or_else(|| SdbError::IndexOutOfRange {
                path:  at.clone(),
                index: index as u64,
                len:   self.len().unwrap_or(0) as u64,
            })?;

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
