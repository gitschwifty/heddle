//! Path resolution for global/local heddle dirs and project subdirs.
//!

use std::path::{Path, PathBuf};

/// Global heddle config directory. Respects `HEDDLE_HOME`; falls back to
/// `~/.heddle`. Relative `HEDDLE_HOME` resolves against the current dir.
pub fn get_heddle_home() -> PathBuf {
    if let Ok(env) = std::env::var("HEDDLE_HOME") {
        let p = PathBuf::from(&env);
        if p.is_absolute() {
            return p;
        }
        if let Ok(cwd) = std::env::current_dir() {
            return cwd.join(p);
        }
        return p;
    }
    dirs::home_dir()
        .map(|h| h.join(".heddle"))
        .unwrap_or_else(|| PathBuf::from(".heddle"))
}

/// Local `.heddle/` directory in the current working directory.
pub fn get_local_heddle_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".heddle")
}

/// Resolved config.toml path. Prefers local over global.
pub fn get_config_path() -> PathBuf {
    let local = get_local_heddle_dir().join("config.toml");
    if local.exists() {
        return local;
    }
    get_heddle_home().join("config.toml")
}

/// Encode an absolute path as a dash-separated directory name (`/a/b/c` →
/// `-a-b-c`).
pub fn encode_path(absolute_path: &str) -> String {
    let trimmed = absolute_path.trim_end_matches('/');
    trimmed.replace('/', "-")
}

pub fn get_project_dir(project_path: Option<&str>) -> PathBuf {
    let cwd_storage;
    let path = match project_path {
        Some(p) => p,
        None => {
            cwd_storage = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .to_string_lossy()
                .into_owned();
            &cwd_storage
        }
    };
    let encoded = encode_path(path);
    get_heddle_home().join("projects").join(encoded)
}

pub fn get_project_sessions_dir(project_path: Option<&str>) -> PathBuf {
    get_project_dir(project_path).join("sessions")
}

pub fn get_agents_dir() -> PathBuf {
    get_heddle_home().join("agents")
}

pub fn get_skills_dir() -> PathBuf {
    get_heddle_home().join("skills")
}

pub fn get_history_path() -> PathBuf {
    get_heddle_home().join("history.jsonl")
}

pub fn get_file_history_dir(project_path: Option<&str>) -> PathBuf {
    get_project_dir(project_path).join("file-history")
}

pub fn get_project_memory_dir(project_path: Option<&str>) -> PathBuf {
    get_project_dir(project_path).join("memory")
}

pub fn get_global_memory_dir() -> PathBuf {
    get_heddle_home().join("memory")
}

/// Walk up from `start_dir` toward `home_dir` collecting `.heddle/` directories,
/// deepest-first. Includes `HEDDLE_HOME` if not already in the walk.
pub fn walk_up_heddle_dirs(start_dir: Option<&Path>, home_dir: Option<&Path>) -> Vec<PathBuf> {
    let cwd_storage;
    let start = match start_dir {
        Some(d) => d.to_path_buf(),
        None => {
            cwd_storage = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            cwd_storage.clone()
        }
    };
    let home = match home_dir {
        Some(d) => d.to_path_buf(),
        None => dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
    };

    let mut found = Vec::new();
    let mut current = start;
    loop {
        let candidate = current.join(".heddle");
        if candidate.is_dir() {
            found.push(candidate);
        }
        if current == home {
            break;
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => break,
        }
    }

    let heddle_home = get_heddle_home();
    if !found.iter().any(|p| p == &heddle_home) && heddle_home.is_dir() {
        found.push(heddle_home);
    }

    found
}

/// Walk up from `start_dir` to find a `.git` entry (file or directory, so
/// worktrees are supported). Returns the containing directory.
pub fn find_repo_root(start_dir: Option<&Path>) -> Option<PathBuf> {
    let cwd_storage;
    let mut current = match start_dir {
        Some(d) => d.to_path_buf(),
        None => {
            cwd_storage = std::env::current_dir().ok()?;
            cwd_storage.clone()
        }
    };
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => return None,
        }
    }
}

pub fn get_system_heddle_dir() -> PathBuf {
    PathBuf::from("/etc/heddle")
}

/// Create the global heddle directory structure plus current project dirs and
/// write a default permissions config if none exists.
pub fn ensure_heddle_dirs() {
    let home = get_heddle_home();
    let _ = std::fs::create_dir_all(home.join("agents"));
    let _ = std::fs::create_dir_all(home.join("skills"));
    let _ = std::fs::create_dir_all(home.join("memory"));
    let _ = std::fs::create_dir_all(get_project_sessions_dir(None));

    let config_path = home.join("config.toml");
    if !config_path.exists() {
        let toml = crate::permissions::defaults::generate_default_permissions_toml();
        let _ = std::fs::write(&config_path, toml);
    }
}
