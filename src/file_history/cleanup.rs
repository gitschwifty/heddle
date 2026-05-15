//! Disk-budget cleanup: drop oldest backups when over `max_size`.

use std::path::PathBuf;

use regex::Regex;

use crate::config::paths::get_file_history_dir;

#[derive(Debug, Clone)]
pub struct CleanupConfig {
    /// bytes — defaults to 100 MiB
    pub max_size: u64,
    pub project_path: Option<String>,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            max_size: 100 * 1024 * 1024,
            project_path: None,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct CleanupStats {
    pub files_removed: u64,
    pub bytes_freed: u64,
}

struct BackupInfo {
    path: PathBuf,
    version: u32,
    size: u64,
}

pub fn run_file_history_cleanup(config: CleanupConfig) -> CleanupStats {
    let mut stats = CleanupStats::default();
    let base_dir = get_file_history_dir(config.project_path.as_deref());
    let entries = match std::fs::read_dir(&base_dir) {
        Ok(e) => e,
        Err(_) => return stats,
    };
    let re = Regex::new(r"^v(\d+)\.bak$").unwrap();

    let mut all_backups: Vec<BackupInfo> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == "meta.json" {
            continue;
        }
        let dir_path = entry.path();
        if !dir_path.is_dir() {
            continue;
        }
        let inner = match std::fs::read_dir(&dir_path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for f in inner.flatten() {
            let fname = f.file_name().to_string_lossy().into_owned();
            let caps = match re.captures(&fname) {
                Some(c) => c,
                None => continue,
            };
            let version: u32 = match caps.get(1).and_then(|m| m.as_str().parse().ok()) {
                Some(v) => v,
                None => continue,
            };
            let size = f.metadata().map(|m| m.len()).unwrap_or(0);
            all_backups.push(BackupInfo {
                path: f.path(),
                version,
                size,
            });
        }
    }

    let mut total: u64 = all_backups.iter().map(|b| b.size).sum();
    if total > config.max_size {
        all_backups.sort_by_key(|b| b.version); // oldest first
        for backup in all_backups {
            if total <= config.max_size {
                break;
            }
            if std::fs::remove_file(&backup.path).is_ok() {
                stats.files_removed += 1;
                stats.bytes_freed += backup.size;
                total = total.saturating_sub(backup.size);
            }
        }
    }
    stats
}
