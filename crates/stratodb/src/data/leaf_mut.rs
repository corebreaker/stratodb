use super::{
    SValue,
    refs::{SMut, SIdentifiable},
};

use crate::{access::Writer, error::SdbResult, path::SPath, Skey};
use std::{marker::PhantomData, sync::Arc};

/// Write accessor for a scalar leaf.
///
/// Returned by the getter a parent write accessor exposes for a scalar field.
/// [`get`](LeafMut::get) reads the value and [`set`](LeafMut::set) overwrites it;
/// the accessor holds the shared writer, the leaf's path and its primary key.
pub struct LeafMut<'t, T> {
    writer: Arc<dyn Writer + 't>,
    base:   SPath,
    key:    Skey,
    _type:  PhantomData<T>,
}

impl<'t, T: SValue> LeafMut<'t, T> {
    /// Reads and decodes the leaf value.
    pub fn get(&self) -> SdbResult<T> {
        T::from_scalar(&self.writer.scalar(self.key)?)
    }

    /// Overwrites the leaf value.
    pub fn set(&self, value: &T) -> SdbResult<()> {
        self.writer.put_scalar(&self.base, value.to_scalar())
    }
}

impl<'t, T> SIdentifiable for LeafMut<'t, T> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

impl<'t, T> SMut<'t> for LeafMut<'t, T> {
    fn open(writer: Arc<dyn Writer + 't>, base: SPath, key: Skey) -> Self {
        Self {
            writer,
            base,
            key,
            _type: PhantomData,
        }
    }
}
