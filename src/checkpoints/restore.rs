//! Apply a checkpoint's `changes` to the working tree.

use std::path::Path;

use crate::file_history::restore::restore_backup;

use super::record::{CheckpointRecord, FileChange};

/// Outcome of restoring a single file from a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// File was rolled back to a prior backup version.
    Restored { file_path: String, version: u32 },
    /// File didn't exist before the turn — removed it.
    Removed { file_path: String },
    /// Restore failed (backup missing, write failed, etc).
    Failed { file_path: String, reason: String },
}

/// Restore every file in `record.changes` to its pre-turn state.
/// `project_path` is forwarded to the file_history layer for project
/// scoping (matches `backup_file` / `restore_backup` signatures).
pub fn restore_code(record: &CheckpointRecord, project_path: Option<&str>) -> Vec<RestoreOutcome> {
    record
        .changes
        .iter()
        .map(|c| restore_one(c, project_path))
        .collect()
}

fn restore_one(change: &FileChange, project_path: Option<&str>) -> RestoreOutcome {
    let path = Path::new(&change.file_path);
    if change.version_before == 0 {
        // File didn't exist before the turn — remove it. If the path is
        // already gone, treat that as success (idempotent rewind).
        if !path.exists() {
            return RestoreOutcome::Removed {
                file_path: change.file_path.clone(),
            };
        }
        return match std::fs::remove_file(path) {
            Ok(()) => RestoreOutcome::Removed {
                file_path: change.file_path.clone(),
            },
            Err(e) => RestoreOutcome::Failed {
                file_path: change.file_path.clone(),
                reason: e.to_string(),
            },
        };
    }
    let msg = restore_backup(path, change.version_before, project_path);
    if msg.starts_with("Error") {
        RestoreOutcome::Failed {
            file_path: change.file_path.clone(),
            reason: msg,
        }
    } else {
        RestoreOutcome::Restored {
            file_path: change.file_path.clone(),
            version: change.version_before,
        }
    }
}
