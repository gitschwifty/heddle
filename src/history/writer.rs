//! Append a single history line to the global history.jsonl.

use std::fs::OpenOptions;
use std::io::Write;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::paths::get_history_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub timestamp: String,
    pub session_id: String,
    pub project: String,
    pub message_preview: String,
    pub content_type: ContentType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Text,
    Mention,
    Shell,
}

pub fn append_history_entry(entry: &HistoryEntry) -> Result<()> {
    let path = get_history_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(entry)?;
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{line}")?;
    Ok(())
}
