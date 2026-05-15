//! Skill loading from discovery levels. Mirrors `ts-src/config/skills.ts`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::discovery::{DiscoveryLevel, DiscoveryResult, DiscoverySource};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub frontmatter: HashMap<String, String>,
    pub source: PathBuf,
    pub level_source: DiscoverySource,
}

/// Parse YAML-style frontmatter from a string. Lightweight, key:value lines
/// only — matches the TS regex-based parser.
pub fn parse_frontmatter(content: &str) -> (HashMap<String, String>, String) {
    let mut fm = HashMap::new();
    let trimmed = content.trim_start_matches('\u{feff}');

    if !trimmed.starts_with("---") {
        return (fm, content.to_string());
    }

    // Find `\n---` after the opening
    let after_open = &trimmed[3..];
    // Match `---\n...\n---\n?`
    if let Some(close_idx) = after_open.find("\n---") {
        let raw_fm = &after_open[..close_idx];
        let body_start = close_idx + 4; // skip "\n---"
        let mut body = &after_open[body_start.min(after_open.len())..];
        // Skip optional trailing newline after the closing fence
        if let Some(s) = body.strip_prefix('\n') {
            body = s;
        }
        for line in raw_fm.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(colon_idx) = line.find(':') {
                let key = line[..colon_idx].trim().to_string();
                let value = line[colon_idx + 1..].trim().to_string();
                if !key.is_empty() {
                    fm.insert(key, value);
                }
            }
        }
        return (fm, body.trim().to_string());
    }

    (fm, content.to_string())
}

pub fn parse_skill_file(
    file_path: &Path,
    namespace: &str,
    level: &DiscoveryLevel,
) -> Option<Skill> {
    let content = std::fs::read_to_string(file_path).ok()?;
    let (frontmatter, body) = parse_frontmatter(&content);

    let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name = if namespace.is_empty() {
        stem.to_string()
    } else {
        let mut parts: Vec<&str> = namespace.split('/').collect();
        parts.push(stem);
        parts.join(":")
    };

    let description = frontmatter
        .get("description")
        .cloned()
        .unwrap_or_else(|| format!("Custom skill: {name}"));

    Some(Skill {
        name,
        description,
        content: body,
        frontmatter,
        source: level.path.clone(),
        level_source: level.source.clone(),
    })
}

fn scan_skills_dir(dir: &Path, level: &DiscoveryLevel, base: &Path, out: &mut Vec<Skill>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let full = entry.path();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            scan_skills_dir(&full, level, base, out);
            continue;
        }
        if full.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel_dir = full
            .parent()
            .and_then(|p| p.strip_prefix(base).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if let Some(skill) = parse_skill_file(&full, &rel_dir, level) {
            out.push(skill);
        }
    }
}

pub fn load_skills_from_discovery(discovery: &DiscoveryResult) -> Vec<Skill> {
    let mut map: HashMap<String, Skill> = HashMap::new();
    for level in &discovery.levels {
        let skills_dir = match level.source {
            DiscoverySource::Agents => level.path.clone(),
            _ => level.path.join("skills"),
        };
        let mut levelskills = Vec::new();
        scan_skills_dir(&skills_dir, level, &skills_dir, &mut levelskills);
        for skill in levelskills {
            map.entry(skill.name.clone()).or_insert(skill);
        }
    }
    map.into_values().collect()
}
