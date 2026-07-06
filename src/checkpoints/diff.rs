//! Snapshot + diff over `FileHistoryMeta` so a turn's file changes can be
//! reconstructed without instrumenting the tools.

use std::collections::HashMap;
use std::path::Path;

use crate::config::paths::get_file_history_dir;
use crate::file_history::meta::FileHistoryMeta;

use super::record::FileChange;

/// `uuid -> (path, versions)` at a point in time.
pub type MetaSnapshot = HashMap<String, (String, u32)>;

/// Read the on-disk meta.json and produce a snapshot. Returns an empty
/// snapshot if meta.json is absent (no file history yet).
pub fn snapshot_meta(project_path: Option<&str>) -> MetaSnapshot {
    let base_dir = get_file_history_dir(project_path);
    let meta_path = Path::new(&base_dir).join("meta.json");
    let Ok(content) = std::fs::read_to_string(&meta_path) else {
        return MetaSnapshot::new();
    };
    let Ok(raw): Result<serde_json::Value, _> = serde_json::from_str(&content) else {
        return MetaSnapshot::new();
    };
    let Some(obj) = raw.as_object() else {
        return MetaSnapshot::new();
    };
    let mut out = MetaSnapshot::new();
    for (uuid, entry) in obj {
        let path = entry
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let versions = entry.get("versions").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        out.insert(uuid.clone(), (path, versions));
    }
    out
}

/// Diff two snapshots and return one `FileChange` per uuid whose version
/// count increased.
///
/// For files created from scratch during the turn (e.g. `write_file` on a
/// path that didn't previously exist), `backup_file` is a no-op and meta
/// is never updated. The diff therefore misses them. Callers that want
/// new-file tracking must pass the set of paths touched by edit/write
/// tools as `touched_paths` — any path in that set that doesn't surface
/// in the diff is appended as a synthetic `FileChange` with `uuid` empty
/// and both versions `0`, which `restore_code` interprets as "remove".
///
/// Sorted by `file_path` for deterministic test output.
pub fn compute_changes_with_touched(
    before: &MetaSnapshot,
    after: &MetaSnapshot,
    touched_paths: &std::collections::HashSet<String>,
) -> Vec<FileChange> {
    let mut changes = compute_changes(before, after);
    let already: std::collections::HashSet<String> =
        changes.iter().map(|c| c.file_path.clone()).collect();
    for path in touched_paths {
        if !already.contains(path) {
            changes.push(FileChange {
                file_path: path.clone(),
                uuid: String::new(),
                version_after: 0,
            });
        }
    }
    changes.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    changes
}

/// Diff two snapshots and return one `FileChange` per uuid whose version
/// count increased. Sorted by `file_path` for deterministic test output.
pub fn compute_changes(before: &MetaSnapshot, after: &MetaSnapshot) -> Vec<FileChange> {
    let mut changes: Vec<FileChange> = after
        .iter()
        .filter_map(|(uuid, (path, after_v))| {
            let before_v = before.get(uuid).map(|(_, v)| *v).unwrap_or(0);
            if *after_v > before_v {
                Some(FileChange {
                    file_path: path.clone(),
                    uuid: uuid.clone(),
                    version_after: *after_v,
                })
            } else {
                None
            }
        })
        .collect();
    changes.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    changes
}

/// Convenience: take a fresh snapshot via a borrowed `FileHistoryMeta`.
/// Useful in tests where the meta object is already in scope.
pub fn snapshot_from_meta(meta: &mut FileHistoryMeta) -> MetaSnapshot {
    meta.all_entries()
        .into_iter()
        .map(|e| (e.uuid, (e.path, e.versions)))
        .collect()
}
