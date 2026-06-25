//! `Option` support: the [`SData`] impl plus the [`OptRef`]/[`OptMut`] accessors.
//!
//! `Some(v)` stores `v` at the option's path; `None` stores a `Null` leaf there.
//! Either way a node exists, so the value is distinguishable from an absent path
//! and the accessor always has a key.

use super::{
    refs::{SIdentifiable, SMut, SRef},
    SData,
    Scalar,
};

use crate::{
    access::{Reader, Writer},
    error::SdbResult,
    node::NodeKind,
    path::SPath,
    Skey,
};

use std::{marker::PhantomData, sync::Arc};

impl<T: SData> SData for Option<T> {
    type Mut<'t> = OptMut<'t, T>;
    type Ref<'t> = OptRef<'t, T>;

    fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()> {
        match self {
            Some(value) => value.store(writer, at),
            None => writer.put_scalar(at, Scalar::Null),
        }
    }

    fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self> {
        match reader.resolve(at)? {
            None => Ok(None),
            Some(key) if is_null(reader, key)? => Ok(None),
            Some(_) => Ok(Some(T::load(reader, at)?)),
        }
    }
}

/// Whether `key` is a `Null` leaf (the encoding of `None`).
fn is_null<R: Reader>(reader: &R, key: Skey) -> SdbResult<bool> {
    match reader.kind(key)? {
        Some(NodeKind::Leaf) => Ok(reader.scalar(key)? == Scalar::Null),
        _ => Ok(false),
    }
}

/// Read accessor for an optional node.
pub struct OptRef<'t, T> {
    reader: Arc<dyn Reader + 't>,
    base:   SPath,
    key:    Skey,
    _type:  PhantomData<T>,
}

impl<'t, T: SData> OptRef<'t, T> {
    /// Whether the option is `None` (a stored `Null`).
    pub fn is_none(&self) -> SdbResult<bool> {
        is_null(&self.reader, self.key)
    }

    /// A read accessor over the contained value, or `None`.
    pub fn get(&self) -> SdbResult<Option<T::Ref<'t>>> {
        Ok((!self.is_none()?)
            .then(|| <T::Ref<'t> as SRef<'t>>::open(Arc::clone(&self.reader), self.base.clone(), self.key)))
    }
}

impl<'t, T> SIdentifiable for OptRef<'t, T> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

impl<'t, T> SRef<'t> for OptRef<'t, T> {
    fn open(reader: Arc<dyn Reader + 't>, base: SPath, key: Skey) -> Self {
        Self {
            reader,
            base,
            key,
            _type: PhantomData,
        }
    }
}

/// Write accessor for an optional node.
pub struct OptMut<'t, T> {
    writer: Arc<dyn Writer + 't>,
    base:   SPath,
    key:    Skey,
    _type:  PhantomData<T>,
}

impl<'t, T: SData> OptMut<'t, T> {
    /// Whether the option is `None` (a stored `Null`).
    pub fn is_none(&self) -> SdbResult<bool> {
        is_null(&self.writer, self.key)
    }

    /// Replaces the whole option with `value`.
    pub fn set(&self, value: &Option<T>) -> SdbResult<()> {
        self.writer.remove(&self.base)?;

        value.store(&self.writer, &self.base)
    }
}

impl<'t, T> SIdentifiable for OptMut<'t, T> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

impl<'t, T> SMut<'t> for OptMut<'t, T> {
    fn open(writer: Arc<dyn Writer + 't>, base: SPath, key: Skey) -> Self {
        Self {
            writer,
            base,
            key,
            _type: PhantomData,
        }
    }
}
