//! Persistent UUID-keyed meta.json mapping paths to backup versions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MetaEntry {
    pub uuid: String,
    pub path: String,
    pub versions: u32,
    pub previous_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MetaStoreEntry {
    path: String,
    versions: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_paths: Option<Vec<String>>,
}

type MetaStore = HashMap<String, MetaStoreEntry>;

pub struct FileHistoryMeta {
    base_dir: PathBuf,
    store: MetaStore,
    loaded: bool,
}

impl FileHistoryMeta {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            store: MetaStore::new(),
            loaded: false,
        }
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        let path = self.base_dir.join("meta.json");
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                self.store = serde_json::from_str(&content).unwrap_or_default();
            }
        }
        self.loaded = true;
    }

    fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.base_dir)?;
        let path = self.base_dir.join("meta.json");
        let serialized = serde_json::to_string_pretty(&self.store)?;
        std::fs::write(&path, serialized)?;
        Ok(())
    }

    pub fn find_by_path(&mut self, file_path: &Path) -> Option<MetaEntry> {
        self.load();
        let target = file_path.to_string_lossy().to_string();
        for (uuid, entry) in &self.store {
            if entry.path == target {
                return Some(MetaEntry {
                    uuid: uuid.clone(),
                    path: entry.path.clone(),
                    versions: entry.versions,
                    previous_paths: entry.previous_paths.clone(),
                });
            }
        }
        None
    }

    pub fn get_or_create(
        &mut self,
        file_path: &Path,
        moved_from_uuid: Option<&str>,
    ) -> Result<MetaEntry> {
        if let Some(existing) = self.find_by_path(file_path) {
            return Ok(existing);
        }
        let uuid = Uuid::new_v4().to_string();
        let target = file_path.to_string_lossy().to_string();
        let previous_paths = moved_from_uuid
            .and_then(|u| self.store.get(u))
            .map(|old| vec![old.path.clone()]);
        let entry = MetaStoreEntry {
            path: target.clone(),
            versions: 0,
            previous_paths: previous_paths.clone(),
        };
        self.store.insert(uuid.clone(), entry);
        self.save()?;
        Ok(MetaEntry {
            uuid,
            path: target,
            versions: 0,
            previous_paths,
        })
    }

    /// Return every entry in the store as a flat Vec, sorted by uuid for
    /// determinism. Used by the checkpoints module to snapshot meta state
    /// at turn boundaries.
    pub fn all_entries(&mut self) -> Vec<MetaEntry> {
        self.load();
        let mut entries: Vec<MetaEntry> = self
            .store
            .iter()
            .map(|(uuid, e)| MetaEntry {
                uuid: uuid.clone(),
                path: e.path.clone(),
                versions: e.versions,
                previous_paths: e.previous_paths.clone(),
            })
            .collect();
        entries.sort_by(|a, b| a.uuid.cmp(&b.uuid));
        entries
    }

    pub fn increment_version(&mut self, uuid: &str) -> Result<()> {
        self.load();
        if let Some(entry) = self.store.get_mut(uuid) {
            entry.versions += 1;
            self.save()?;
        }
        Ok(())
    }
}
