use super::{
    SValue,
    refs::{SIdentifiable, SRef},
};

use crate::{access::Reader, error::SdbResult, path::SPath, Skey};
use std::{marker::PhantomData, sync::Arc};

/// Read accessor for a scalar leaf.
///
/// Returned by the getter a parent accessor exposes for a scalar field. The
/// value is read on demand by [`get`](Leaf::get); the accessor itself only holds
/// the shared reader, the leaf's path and its primary key.
pub struct Leaf<'t, T> {
    reader: Arc<dyn Reader + 't>,
    base:   SPath,
    key:    Skey,
    _type:  PhantomData<T>,
}

impl<'t, T: SValue> Leaf<'t, T> {
    /// Reads and decodes the leaf value.
    pub fn get(&self) -> SdbResult<T> {
        let scalar = self.reader.scalar(self.key)?;

        T::from_scalar(&scalar)
    }
}

impl<'t, T> SIdentifiable for Leaf<'t, T> {
    fn key(&self) -> Skey {
        self.key
    }

    fn path(&self) -> &SPath {
        &self.base
    }
}

impl<'t, T> SRef<'t> for Leaf<'t, T> {
    fn open(reader: Arc<dyn Reader + 't>, base: SPath, key: Skey) -> Self {
        Self {
            reader,
            base,
            key,
            _type: PhantomData,
        }
    }
}
