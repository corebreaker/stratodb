use super::{super::IndexId, IndexEntry};
use crate::{
    codec::{self, Reader},
    constants::META_INDEX_REGISTRY_KEY,
    error::{SdbError, SdbResult},
    index::definitions::IndexDef,
};

use redb::{ReadableTable, Table};

/// The write handle to the metadata table.
pub(super) type MetaTable<'txn> = Table<'txn, &'static str, &'static [u8]>;

/// The whole registry: an id allocator plus every registered entry.
#[derive(Default)]
pub(super) struct RegistryRepository {
    next_id: u32,
    entries: Vec<IndexEntry>,
}

impl RegistryRepository {
    fn decode(data: &[u8]) -> SdbResult<Self> {
        let mut r = Reader::new(data);
        let next_id = r.u32()?;
        let count = r.u32()?;

        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let table = std::str::from_utf8(r.bytes()?)
                .map(str::to_string)
                .map_err(|_| SdbError::Corrupt("invalid utf-8 in index registry".into()))?;

            let id = IndexId(r.u32()?);
            let def = IndexDef::decode(&mut r)?;
            entries.push(IndexEntry::new(table, id, def));
        }

        Ok(Self {
            next_id,
            entries,
        })
    }

    fn load<T: ReadableTable<&'static str, &'static [u8]>>(meta: &T) -> SdbResult<Self> {
        match meta.get(META_INDEX_REGISTRY_KEY)? {
            Some(guard) => Self::decode(guard.value()),
            None => Ok(Self::default()),
        }
    }

    /// Encodes the registry, including its entry count, as a `u32`.
    ///
    /// This caps the registry at `u32::MAX` index entries (across the whole
    /// database, not per table) — far beyond any realistic schema — and is
    /// reported as [`SdbError::Corrupt`] rather than silently truncated.
    fn encode(&self) -> SdbResult<Vec<u8>> {
        let count = u32::try_from(self.entries.len())
            .map_err(|_| SdbError::Corrupt("index registry exceeds u32::MAX entries".into()))?;

        let mut buf = Vec::new();
        codec::put_u32(&mut buf, self.next_id);
        codec::put_u32(&mut buf, count);

        for entry in &self.entries {
            codec::put_bytes(&mut buf, entry.table().as_bytes());
            codec::put_u32(&mut buf, entry.id().0);
            entry.def().encode(&mut buf);
        }

        Ok(buf)
    }

    fn store(&self, meta: &mut MetaTable<'_>) -> SdbResult<()> {
        meta.insert(META_INDEX_REGISTRY_KEY, self.encode()?.as_slice())?;
        Ok(())
    }

    /// Looks up an index by `table` and `name`.
    pub(crate) fn lookup<T: ReadableTable<&'static str, &'static [u8]>>(
        meta: &T,
        table: &str,
        name: &str,
    ) -> SdbResult<Option<IndexEntry>> {
        let registry = Self::load(meta)?;
        let entry = registry
            .entries
            .into_iter()
            .find(|e| e.table() == table && e.def().name() == name);

        Ok(entry)
    }

    pub(super) fn create(meta: &mut MetaTable<'_>, table: &str, def: &IndexDef) -> SdbResult<IndexId> {
        let mut registry = Self::load(meta)?;
        let entry = registry
            .entries
            .iter()
            .find(|e| e.table() == table && e.def().name() == def.name());

        if let Some(entry) = entry {
            if entry.def() == def {
                return Ok(entry.id());
            }

            return Err(SdbError::SchemaMismatch(format!(
                "index '{name}' on table '{table}' already exists with a different definition",
                name = def.name()
            )));
        }

        let id = IndexId(registry.next_id);
        registry.next_id += 1;
        registry
            .entries
            .push(IndexEntry::new(table.to_string(), id, def.clone()));

        registry.store(meta)?;

        Ok(id)
    }
}
