//! Crate-wide constants.

/// Name of the reserved, file-global metadata table.
pub const METADATA_TABLE_NAME: &str = "$metadata";

/// On-disk format version understood by this build.
pub const FORMAT_VERSION: u32 = 1;

/// A constant string key representing the metadata key for the format version.
pub(crate) const META_FORMAT_VERSION_KEY: &str = "format_version";
