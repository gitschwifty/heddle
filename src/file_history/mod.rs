//! File backup, restore, and disk-budget cleanup.

pub mod backup;
pub mod cleanup;
pub mod meta;
pub mod restore;

pub use backup::backup_file;
pub use cleanup::{run_file_history_cleanup, CleanupConfig, CleanupStats};
pub use meta::{FileHistoryMeta, MetaEntry};
pub use restore::{list_backups, restore_backup, BackupEntry};
