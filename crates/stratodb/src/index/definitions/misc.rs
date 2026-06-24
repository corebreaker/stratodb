use crate::{codec::Reader, SdbError, SdbResult};

pub(super) fn read_string(r: &mut Reader<'_>) -> SdbResult<String> {
    std::str::from_utf8(r.bytes()?)
        .map(str::to_string)
        .map_err(|_| SdbError::Corrupt("invalid utf-8 in index definition".into()))
}
