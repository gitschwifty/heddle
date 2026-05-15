//! Back up a file's contents before it's modified.

use std::path::Path;

use anyhow::Result;
use sha2::{Digest, Sha256};

use super::meta::FileHistoryMeta;
use crate::config::paths::get_file_history_dir;

fn hash_content(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    hex::encode(hasher.finalize())
}

pub fn backup_file(file_path: &Path, project_path: Option<&str>) -> Result<()> {
    if !file_path.exists() {
        return Ok(());
    }
    let content = std::fs::read(file_path)?;
    let hash = hash_content(&content);

    let base_dir = get_file_history_dir(project_path);
    let mut meta = FileHistoryMeta::new(&base_dir);
    let entry = meta.get_or_create(file_path, None)?;
    let uuid_dir = base_dir.join(&entry.uuid);

    if entry.versions > 0 {
        let latest_path = uuid_dir.join(format!("v{}.bak", entry.versions));
        if let Ok(latest) = std::fs::read(&latest_path) {
            if hash_content(&latest) == hash {
                return Ok(());
            }
        }
    }

    std::fs::create_dir_all(&uuid_dir)?;
    let next_version = entry.versions + 1;
    std::fs::write(uuid_dir.join(format!("v{next_version}.bak")), content)?;
    meta.increment_version(&entry.uuid)?;
    Ok(())
}
