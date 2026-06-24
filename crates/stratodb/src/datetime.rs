//! Date/time (de)serialization helpers for the temporal scalar variants.

use crate::{
    codec::Reader,
    error::{SdbError, SdbResult},
};

use chrono::NaiveTime;

/// Decodes a [`NaiveTime`] from its seconds-since-midnight and nanoseconds encoding.
pub(super) fn decode_time(r: &mut Reader<'_>) -> SdbResult<NaiveTime> {
    let secs = r.u32()?;
    let nanos = r.u32()?;

    NaiveTime::from_num_seconds_from_midnight_opt(secs, nanos)
        .ok_or_else(|| SdbError::Corrupt("out-of-range time scalar".into()))
}
