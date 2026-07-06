//! Discovery: scan `.heddle/` dirs up from cwd, plus `/etc/heddle` and
//! `.agents/skills/` at the repo root.

use std::path::{Path, PathBuf};

use super::paths::{find_repo_root, get_system_heddle_dir, walk_up_heddle_dirs};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoverySource {
    Heddle,
    Agents,
    System,
}

#[derive(Debug, Clone)]
pub struct DiscoveryLevel {
    pub path: PathBuf,
    pub source: DiscoverySource,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub config: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveryResult {
    pub levels: Vec<DiscoveryLevel>,
}

fn list_subdir(base: &Path, subdir: &str) -> Vec<String> {
    let dir = base.join(subdir);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect()
}

fn build_heddle_level(heddle_path: PathBuf) -> DiscoveryLevel {
    let skills = list_subdir(&heddle_path, "skills");
    let agents = list_subdir(&heddle_path, "agents");
    let config_path = heddle_path.join("config.toml");
    let config = if config_path.exists() {
        Some(config_path)
    } else {
        None
    };
    DiscoveryLevel {
        path: heddle_path,
        source: DiscoverySource::Heddle,
        skills,
        agents,
        config,
    }
}

pub fn resolve_discovery(cwd: Option<&Path>, home_dir: Option<&Path>) -> DiscoveryResult {
    let mut levels = Vec::new();

    let heddle_dirs = walk_up_heddle_dirs(cwd, home_dir);
    for dir in heddle_dirs {
        levels.push(build_heddle_level(dir));
    }

    if let Some(repo_root) = find_repo_root(cwd) {
        let agents_skills = repo_root.join(".agents").join("skills");
        if agents_skills.is_dir() {
            let skills = list_subdir(&repo_root.join(".agents"), "skills");
            levels.push(DiscoveryLevel {
                path: agents_skills,
                source: DiscoverySource::Agents,
                skills,
                agents: Vec::new(),
                config: None,
            });
        }
    }

    let system_dir = get_system_heddle_dir();
    if system_dir.is_dir() {
        let skills = list_subdir(&system_dir, "skills");
        let agents = list_subdir(&system_dir, "agents");
        let config_path = system_dir.join("config.toml");
        let config = if config_path.exists() {
            Some(config_path)
        } else {
            None
        };
        levels.push(DiscoveryLevel {
            path: system_dir,
            source: DiscoverySource::System,
            skills,
            agents,
            config,
        });
    }

    DiscoveryResult { levels }
}
