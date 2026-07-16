//! JSONL session persistence — meta header, message-per-line, context markers.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::Message;

pub const CONTEXT_RESET_MARKER_TYPE: &str = "context_reset";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub cwd: String,
    pub model: String,
    pub created: String,
    pub heddle_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<String>,
    /// Catch-all for unknown extra fields, preserved on round-trip.
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn append_line(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(value)?;
    writeln!(file, "{line}")?;
    Ok(())
}

pub fn write_session_meta(path: &Path, meta: &SessionMeta) -> Result<()> {
    let v = serde_json::to_value(meta)?;
    append_line(path, &v)
}

pub fn append_message(path: &Path, message: &Message) -> Result<()> {
    let mut v = serde_json::to_value(message)?;
    if let Value::Object(map) = &mut v {
        map.insert("timestamp".into(), Value::String(Utc::now().to_rfc3339()));
    }
    append_line(path, &v)
}

pub fn append_context_marker(path: &Path, marker: &Value) -> Result<()> {
    append_line(path, marker)
}

pub fn load_session(path: &Path) -> Vec<Message> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut messages = Vec::new();
    for value in content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        if value.get("type").and_then(Value::as_str) == Some(CONTEXT_RESET_MARKER_TYPE) {
            messages.clear();
            continue;
        }
        if value.get("role").is_some() {
            if let Ok(message) = serde_json::from_value::<Message>(value) {
                messages.push(message);
            }
        }
    }
    messages
}

pub fn load_session_meta(path: &Path) -> Option<SessionMeta> {
    let content = std::fs::read_to_string(path).ok()?;
    let first_line = content.lines().next()?.trim();
    if first_line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(first_line).ok()?;
    if v.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    serde_json::from_value(v).ok()
}

pub fn load_all_session_metas(session_dir: &Path) -> Vec<(SessionMeta, PathBuf)> {
    let entries = match std::fs::read_dir(session_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if let Some(meta) = load_session_meta(&path) {
            out.push((meta, path));
        }
    }
    out
}
