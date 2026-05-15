//! Fork a session — new UUID, forked_from pointing back, optionally truncated.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use super::jsonl::load_session_meta;

#[derive(Debug, Clone)]
pub struct ForkResult {
    pub session_file: PathBuf,
    pub session_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct ForkOptions {
    pub up_to_message: Option<usize>,
}

pub fn fork_session(source_file: &Path, options: ForkOptions) -> Result<ForkResult> {
    let mut meta = load_session_meta(source_file).ok_or_else(|| {
        anyhow!(
            "Cannot fork: no session_meta found in {}",
            source_file.display()
        )
    })?;

    let content = std::fs::read_to_string(source_file)?;
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    let mut message_lines: Vec<&str> = Vec::new();
    for line in &lines {
        let parsed: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if parsed.get("type").and_then(Value::as_str) == Some("session_meta") {
            continue;
        }
        if parsed.get("role").is_some() {
            message_lines.push(line);
        }
    }

    let new_id = Uuid::new_v4().to_string();
    let session_dir = source_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let new_file = session_dir.join(format!("{new_id}.jsonl"));

    let old_id = meta.id.clone();
    meta.id = new_id.clone();
    meta.created = Utc::now().to_rfc3339();
    meta.forked_from = Some(old_id);

    let to_copy = match options.up_to_message {
        Some(n) => &message_lines[..n.min(message_lines.len())],
        None => &message_lines[..],
    };

    let mut output = serde_json::to_string(&meta)?;
    output.push('\n');
    for line in to_copy {
        output.push_str(line);
        output.push('\n');
    }
    std::fs::write(&new_file, output)?;
    Ok(ForkResult {
        session_file: new_file,
        session_id: new_id,
    })
}
