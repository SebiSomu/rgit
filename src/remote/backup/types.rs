//! Types of `rgit backup`.

use std::path::PathBuf;

/// One backup file of this repository: its path, the time stamp in its name
/// (`2026-11-25-174212`, UTC) and the sequence number of backups made in the same second.
// Used by: src/remote/backup/types.rs, src/remote/backup/engine.rs, src/remote/backup.rs
// Used by command/helper: `list_backups` and the `backup` command
#[derive(Debug, Clone)]
pub struct Backup {
    pub path: PathBuf,
    pub stamp: String,
    pub sequence: u32,
}
