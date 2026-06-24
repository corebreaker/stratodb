use super::Direction;
use crate::path::SPath;

/// One column of an index: the value found at `path` (relative to each indexed
/// entity), sorted in `direction`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexColumn {
    /// Path of the column value, relative to a matched entity.
    path:      SPath,
    /// Sort direction for this column.
    direction: Direction,
}

impl IndexColumn {
    pub(super) fn new(path: SPath, direction: Direction) -> Self {
        Self {
            path,
            direction,
        }
    }

    /// An ascending column over `path`.
    pub fn asc(path: SPath) -> Self {
        Self {
            path,
            direction: Direction::Asc,
        }
    }

    /// A descending column over `path`.
    pub fn desc(path: SPath) -> Self {
        Self {
            path,
            direction: Direction::Desc,
        }
    }

    pub fn path(&self) -> &SPath {
        &self.path
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }
}
