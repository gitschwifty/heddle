//! Path-keyed paste cache for @ mentions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngCore;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct CachedPaste {
    pub path: PathBuf,
    pub content: String,
    pub hash: String,
    pub timestamp: u128,
    pub lines: usize,
    pub paste_id: Option<String>,
}

const DEFAULT_THRESHOLD: usize = 10_240;

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn generate_paste_id() -> String {
    let mut bytes = [0u8; 3];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

pub struct PasteCache {
    cache: HashMap<PathBuf, CachedPaste>,
    paste_ids: HashMap<String, PathBuf>,
    threshold: usize,
}

impl Default for PasteCache {
    fn default() -> Self {
        Self::new(None)
    }
}

impl PasteCache {
    pub fn new(threshold: Option<usize>) -> Self {
        Self {
            cache: HashMap::new(),
            paste_ids: HashMap::new(),
            threshold: threshold.unwrap_or(DEFAULT_THRESHOLD),
        }
    }

    pub fn resolve(&mut self, absolute_path: &PathBuf) -> std::io::Result<CachedPaste> {
        let content = std::fs::read_to_string(absolute_path)?;
        let hash = content_hash(&content);

        if let Some(existing) = self.cache.get(absolute_path) {
            if existing.hash == hash {
                return Ok(existing.clone());
            }
            if let Some(pid) = &existing.paste_id {
                self.paste_ids.remove(pid);
            }
        }

        let lines = content.split('\n').count();
        let byte_length = content.len();
        let paste_id = if byte_length > self.threshold {
            let id = generate_paste_id();
            self.paste_ids.insert(id.clone(), absolute_path.clone());
            Some(id)
        } else {
            None
        };

        let entry = CachedPaste {
            path: absolute_path.clone(),
            content,
            hash,
            timestamp: now_ms(),
            lines,
            paste_id,
        };
        self.cache.insert(absolute_path.clone(), entry.clone());
        Ok(entry)
    }

    pub fn get_by_paste_id(&self, id: &str) -> Option<&CachedPaste> {
        let path = self.paste_ids.get(id)?;
        self.cache.get(path)
    }

    pub fn list(&self) -> Vec<CachedPaste> {
        self.cache.values().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.paste_ids.clear();
    }

    pub fn size(&self) -> usize {
        self.cache.len()
    }
}
