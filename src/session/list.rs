//! List recent sessions and look one up by id/name/most-recent.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::jsonl::load_session_meta;
use crate::config::paths::get_project_sessions_dir;

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub name: Option<String>,
    pub created: String,
    pub model: String,
    pub cwd: String,
    pub message_count: usize,
    pub first_user_message: Option<String>,
    pub forked_from: Option<String>,
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

pub fn list_sessions(session_dir: Option<&Path>) -> Vec<SessionInfo> {
    let dir = match session_dir {
        Some(d) => d.to_path_buf(),
        None => get_project_sessions_dir(None),
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let meta = match load_session_meta(&path) {
            Some(m) => m,
            None => continue,
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let parsed_lines: Vec<Value> = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();

        let messages: Vec<&Value> = parsed_lines
            .iter()
            .filter(|v| v.get("role").is_some())
            .collect();
        let first_user = messages
            .iter()
            .find(|m| m.get("role").and_then(Value::as_str) == Some("user"));
        let first_user_message = first_user.and_then(|m| {
            m.get("content").and_then(Value::as_str).map(|s| {
                if s.chars().count() > 100 {
                    truncate_chars(s, 100)
                } else {
                    s.to_string()
                }
            })
        });

        let mut name = meta.name.clone();
        for line in &parsed_lines {
            if line.get("type").and_then(Value::as_str) == Some("session_name") {
                if let Some(n) = line.get("name").and_then(Value::as_str) {
                    name = Some(n.to_string());
                }
            }
        }

        out.push(SessionInfo {
            id: meta.id.clone(),
            name,
            created: meta.created.clone(),
            model: meta.model.clone(),
            cwd: meta.cwd.clone(),
            message_count: messages.len(),
            first_user_message,
            forked_from: meta.forked_from.clone(),
        });
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

pub fn find_session(target: Option<&str>, session_dir: Option<&Path>) -> Option<PathBuf> {
    let dir = match session_dir {
        Some(d) => d.to_path_buf(),
        None => get_project_sessions_dir(None),
    };
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut entries_v: Vec<(SessionInfo, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let meta = match load_session_meta(&path) {
            Some(m) => m,
            None => continue,
        };
        let mut name = meta.name.clone();
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                if let Ok(v) = serde_json::from_str::<Value>(line) {
                    if v.get("type").and_then(Value::as_str) == Some("session_name") {
                        if let Some(n) = v.get("name").and_then(Value::as_str) {
                            name = Some(n.to_string());
                        }
                    }
                }
            }
        }
        entries_v.push((
            SessionInfo {
                id: meta.id.clone(),
                name,
                created: meta.created.clone(),
                model: meta.model.clone(),
                cwd: meta.cwd.clone(),
                message_count: 0,
                first_user_message: None,
                forked_from: meta.forked_from.clone(),
            },
            path,
        ));
    }

    let target = target.map(str::trim).filter(|s| !s.is_empty());
    match target {
        None => {
            entries_v.sort_by(|a, b| b.0.created.cmp(&a.0.created));
            entries_v.into_iter().next().map(|(_, p)| p)
        }
        Some(t) => {
            if let Some((_, p)) = entries_v
                .iter()
                .find(|(i, _)| i.id == t || i.id.starts_with(t))
            {
                return Some(p.clone());
            }
            entries_v
                .into_iter()
                .find(|(i, _)| i.name.as_deref() == Some(t))
                .map(|(_, p)| p)
        }
    }
}
