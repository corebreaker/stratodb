use super::{misc::read_string, Direction, IndexColumn};
use crate::{
    codec::{self, Reader},
    data::Scalar,
    error::SdbResult,
    index::ordered,
    path::SPath,
};

/// A secondary index definition.
///
/// `pattern` selects which entities to index — a path pattern, e.g. `users/*`
/// (matching is added in a later milestone-3 step). `columns` form the sort key,
/// each a path relative to a matched entity, in priority order. A `unique` index
/// rejects a second entity that produces the same column tuple.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexDef {
    /// The index name (unique per table).
    name:    String,
    /// The path pattern selecting indexed entities.
    pattern: String,
    /// The sort-key columns, in priority order.
    columns: Vec<IndexColumn>,
    /// Whether the column tuple must be unique across entities.
    unique:  bool,
}

impl IndexDef {
    pub fn new(name: String, pattern: String, columns: Vec<IndexColumn>, unique: bool) -> Self {
        Self {
            name,
            pattern,
            columns,
            unique,
        }
    }

    /// Appends the registry encoding of this definition to `buf`.
    pub(crate) fn encode(&self, buf: &mut Vec<u8>) {
        codec::put_bytes(buf, self.name.as_bytes());
        codec::put_bytes(buf, self.pattern.as_bytes());
        buf.push(u8::from(self.unique));
        codec::put_u32(buf, self.columns.len() as u32);

        for column in &self.columns {
            codec::put_bytes(buf, column.path().to_string().as_bytes());
            buf.push(column.direction().to_byte());
        }
    }

    /// Decodes a definition written by [`IndexDef::encode`].
    pub(crate) fn decode(r: &mut Reader<'_>) -> SdbResult<Self> {
        let name = read_string(r)?;
        let pattern = read_string(r)?;
        let unique = r.u8()? != 0;
        let count = r.u32()?;

        let mut columns = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let path = SPath::parse(&read_string(r)?)?;
            let direction = Direction::from_byte(r.u8()?)?;
            columns.push(IndexColumn::new(path, direction));
        }

        Ok(IndexDef {
            name,
            pattern,
            columns,
            unique,
        })
    }

    /// Encodes `values` (one per column, in column order) into the order-preserving
    /// byte prefix an index key starts with, applying each column's direction.
    /// Shared by write-time maintenance (values read from an entity) and queries
    /// (values supplied by the caller).
    pub(crate) fn encode_columns(&self, values: &[Scalar]) -> Vec<u8> {
        let mut cols = Vec::new();
        for (value, column) in values.iter().zip(&self.columns) {
            ordered::encode_scalar(&mut cols, value, column.direction() == Direction::Desc);
        }

        cols
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn columns(&self) -> &Vec<IndexColumn> {
        &self.columns
    }

    pub fn unique(&self) -> bool {
        self.unique
    }
}
