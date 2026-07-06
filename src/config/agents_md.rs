//! AGENTS.md discovery and concatenation.

use std::path::{Path, PathBuf};

use super::paths::get_heddle_home;

fn find_agents_md_in(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if name.to_lowercase() == "agents.md" {
                return Some(dir.join(name));
            }
        }
    }
    None
}

pub fn find_all_agents_md(start_dir: Option<&Path>) -> Vec<PathBuf> {
    let cwd_storage;
    let start = match start_dir {
        Some(d) => d.to_path_buf(),
        None => {
            cwd_storage = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            cwd_storage.clone()
        }
    };
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

    let mut found = Vec::new();
    let mut current = start;
    loop {
        if let Some(p) = find_agents_md_in(&current) {
            found.push(p);
        }
        if current == home {
            break;
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => break,
        }
    }

    found.reverse();

    let heddle_home = get_heddle_home();
    if let Some(p) = find_agents_md_in(&heddle_home) {
        if !found.contains(&p) {
            found.insert(0, p);
        }
    }

    found
}

pub fn load_agents_context(start_dir: Option<&Path>) -> Option<String> {
    let paths = find_all_agents_md(start_dir);
    if paths.is_empty() {
        return None;
    }
    let parts: Vec<String> = paths
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}
