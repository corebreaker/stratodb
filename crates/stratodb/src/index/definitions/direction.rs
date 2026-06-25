use crate::error::{SdbError, SdbResult};

/// The sort direction of an index column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

impl Direction {
    pub(super) fn to_byte(self) -> u8 {
        match self {
            Direction::Asc => 0,
            Direction::Desc => 1,
        }
    }

    pub(super) fn from_byte(byte: u8) -> SdbResult<Self> {
        match byte {
            0 => Ok(Direction::Asc),
            1 => Ok(Direction::Desc),
            other => Err(SdbError::Corrupt(format!("unknown index direction {other}"))),
        }
    }
}
