//! Read/write checkpoint records in the session JSONL.

use std::path::Path;

use anyhow::Result;

use crate::session::jsonl::append_context_marker;

use super::record::CheckpointRecord;

/// Append a checkpoint record as a JSON line in the session file.
pub fn write_checkpoint(session_file: &Path, record: &CheckpointRecord) -> Result<()> {
    let value = serde_json::to_value(record)?;
    append_context_marker(session_file, &value)
}

/// Scan a session JSONL file and return all `type: "checkpoint"` lines as
/// `CheckpointRecord`s in file order. Skips other line types silently.
/// Returns an empty Vec if the file doesn't exist.
pub fn load_checkpoints(session_file: &Path) -> Vec<CheckpointRecord> {
    let Ok(content) = std::fs::read_to_string(session_file) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("checkpoint"))
        .filter_map(|v| serde_json::from_value::<CheckpointRecord>(v).ok())
        .collect()
}
