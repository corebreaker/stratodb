//! Crate-wide constants.

/// Name of the reserved, file-global metadata table.
pub const METADATA_TABLE_NAME: &str = "$metadata";

/// On-disk format version understood by this build.
///
/// v2 stores an object's children as separate `(parent, name)` child-link entries
/// instead of one inline map blob per object node. v3 adds packed entities: a
/// `store` whose subtree no index reaches into is written as a single packed value
/// (a serialized mini node-table) rather than one engine entry per shredded node.
/// v4 makes that packed value an rkyv-archived tree, navigated zero-copy on read
/// (see [`crate::engine`]).
pub const FORMAT_VERSION: u32 = 4;

/// A constant string key representing the metadata key for the format version.
pub(crate) const META_FORMAT_VERSION_KEY: &str = "format_version";

/// Metadata key under which the secondary-index registry blob is stored.
pub(crate) const META_INDEX_REGISTRY_KEY: &str = "index_registry";
