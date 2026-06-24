//! Engine table definitions.

use crate::constants::METADATA_TABLE_NAME;
use redb::TableDefinition;

/// Definition of the reserved, file-global metadata table.
pub(super) const META_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new(METADATA_TABLE_NAME);
