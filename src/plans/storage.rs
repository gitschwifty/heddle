//! Read/write plan markdown files with YAML frontmatter.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;

use crate::config::paths::get_project_dir;

pub fn get_plans_dir(project_path: Option<&str>) -> PathBuf {
    get_project_dir(project_path).join("plans")
}

fn sanitize_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect();
    s.replace("..", "").trim_start_matches('.').to_string()
}

fn parse_frontmatter(raw: &str) -> (BTreeMap<String, String>, String) {
    let mut meta = BTreeMap::new();
    if !raw.starts_with("---\n") {
        return (meta, raw.to_string());
    }
    let after_open = &raw[4..];
    let end_idx = match after_open.find("\n---\n") {
        Some(i) => i,
        None => return (meta, raw.to_string()),
    };
    let block = &after_open[..end_idx];
    for line in block.lines() {
        if let Some(colon_idx) = line.find(':') {
            let key = line[..colon_idx].trim().to_string();
            let value = line[colon_idx + 1..].trim().to_string();
            if !key.is_empty() {
                meta.insert(key, value);
            }
        }
    }
    let body_start = 4 + end_idx + 5; // "---\n" + idx + "\n---\n"
    let body = &raw[body_start..];
    (meta, body.to_string())
}

fn build_frontmatter(meta: &BTreeMap<String, String>) -> String {
    let mut lines = vec!["---".to_string()];
    for (k, v) in meta {
        lines.push(format!("{k}: {v}"));
    }
    lines.push("---".to_string());
    let mut joined = lines.join("\n");
    joined.push('\n');
    joined
}

pub struct PlanMeta<'a> {
    pub model: Option<&'a str>,
    pub session_id: Option<&'a str>,
}

pub fn save_plan(
    name: &str,
    content: &str,
    meta: PlanMeta<'_>,
    project_path: Option<&str>,
) -> Result<PathBuf> {
    let plans_dir = get_plans_dir(project_path);
    std::fs::create_dir_all(&plans_dir)?;

    let safe_name = sanitize_name(name);
    let file_path = plans_dir.join(format!("{safe_name}.md"));

    let mut fm = BTreeMap::new();
    fm.insert("created".to_string(), Utc::now().to_rfc3339());
    if let Some(m) = meta.model {
        fm.insert("model".to_string(), m.to_string());
    }
    if let Some(s) = meta.session_id {
        fm.insert("session_id".to_string(), s.to_string());
    }

    let file_content = format!("{}{content}\n", build_frontmatter(&fm));
    std::fs::write(&file_path, file_content)?;
    Ok(file_path)
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub name: String,
    pub content: String,
    pub meta: BTreeMap<String, String>,
}

pub fn load_plan(name: &str, project_path: Option<&str>) -> Option<Plan> {
    let plans_dir = get_plans_dir(project_path);
    let safe_name = sanitize_name(name);
    let file_path = plans_dir.join(format!("{safe_name}.md"));
    let raw = std::fs::read_to_string(&file_path).ok()?;
    let (meta, body) = parse_frontmatter(&raw);
    Some(Plan {
        name: safe_name,
        content: body.trim().to_string(),
        meta,
    })
}

#[derive(Debug, Clone)]
pub struct PlanSummary {
    pub name: String,
    pub created: String,
    pub preview: String,
}

pub fn list_plans(project_path: Option<&str>) -> Vec<PlanSummary> {
    let plans_dir = get_plans_dir(project_path);
    let entries = match std::fs::read_dir(&plans_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let (meta, body) = parse_frontmatter(&raw);
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let first_line = body.trim().lines().next().unwrap_or("").to_string();
        out.push(PlanSummary {
            name,
            created: meta.get("created").cloned().unwrap_or_default(),
            preview: first_line,
        });
    }
    out
}
