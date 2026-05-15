//! List backups and restore a specific version.

use std::path::{Path, PathBuf};

use regex::Regex;

use super::meta::FileHistoryMeta;
use crate::config::paths::get_file_history_dir;

#[derive(Debug, Clone)]
pub struct BackupEntry {
    pub version: u32,
    pub path: PathBuf,
    pub size: u64,
}

pub fn list_backups(file_path: &Path, project_path: Option<&str>) -> Vec<BackupEntry> {
    let base_dir = get_file_history_dir(project_path);
    let mut meta = FileHistoryMeta::new(&base_dir);
    let entry = match meta.find_by_path(file_path) {
        Some(e) => e,
        None => return Vec::new(),
    };
    let uuid_dir = base_dir.join(&entry.uuid);
    let read = match std::fs::read_dir(&uuid_dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let re = Regex::new(r"^v(\d+)\.bak$").unwrap();
    let mut entries: Vec<BackupEntry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let caps = re.captures(&name)?;
            let version: u32 = caps.get(1)?.as_str().parse().ok()?;
            let path = e.path();
            let size = e.metadata().ok()?.len();
            Some(BackupEntry {
                version,
                path,
                size,
            })
        })
        .collect();
    entries.sort_by(|a, b| b.version.cmp(&a.version));
    entries
}

pub fn restore_backup(file_path: &Path, version: u32, project_path: Option<&str>) -> String {
    let base_dir = get_file_history_dir(project_path);
    let mut meta = FileHistoryMeta::new(&base_dir);
    let entry = match meta.find_by_path(file_path) {
        Some(e) => e,
        None => return format!("Error: No backup history found for {}", file_path.display()),
    };
    let backup_path = base_dir.join(&entry.uuid).join(format!("v{version}.bak"));
    let content = match std::fs::read(&backup_path) {
        Ok(c) => c,
        Err(_) => {
            return format!(
                "Error: Backup version {version} not found for {}",
                file_path.display()
            )
        }
    };
    if let Err(e) = std::fs::write(file_path, content) {
        return format!("Error: {e}");
    }
    format!("Restored {} from backup v{version}", file_path.display())
}
