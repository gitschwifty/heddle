#![allow(dead_code)]
//! Isolated test sandbox: tempdir + HEDDLE_HOME override + cwd switch.
//!
//! Drop the sandbox to restore env/cwd
//! and remove the tempdir.

use std::path::PathBuf;
use std::sync::Mutex;

use once_cell::sync::Lazy;

/// Serializes sandbox setup/teardown across tests so concurrent runs don't
/// race on `HEDDLE_HOME` + cwd. Rust integration tests in the same file share
/// a process by default.
static GLOBAL_ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub struct Sandbox {
    pub root: PathBuf,
    pub home: PathBuf,
    pub heddle_home: PathBuf,
    pub project: PathBuf,
    orig_cwd: PathBuf,
    orig_heddle_home: Option<String>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Sandbox {
    pub fn new(prefix: &str) -> Self {
        let guard = GLOBAL_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let raw_root = std::env::temp_dir().join("heddle-test").join(format!(
            "{prefix}-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        ));
        std::fs::create_dir_all(&raw_root).expect("create sandbox root");
        let root = std::fs::canonicalize(&raw_root).expect("canonicalize sandbox root");

        let home = root.join("home");
        let heddle_home = home.join(".heddle");
        let project = root.join("project");
        std::fs::create_dir_all(&heddle_home).expect("create heddle_home");
        std::fs::create_dir_all(&project).expect("create project");

        let orig_cwd = std::env::current_dir().expect("orig cwd");
        let orig_heddle_home = std::env::var("HEDDLE_HOME").ok();
        std::env::set_var("HEDDLE_HOME", &heddle_home);
        std::env::set_current_dir(&project).expect("chdir into sandbox project");

        // Reload debug config so HEDDLE_HOME-dependent state is fresh.
        heddle::debug::reset_debug();

        Self {
            root,
            home,
            heddle_home,
            project,
            orig_cwd,
            orig_heddle_home,
            _guard: guard,
        }
    }

    pub fn orig_cwd(&self) -> &PathBuf {
        &self.orig_cwd
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.orig_cwd);
        match &self.orig_heddle_home {
            Some(v) => std::env::set_var("HEDDLE_HOME", v),
            None => std::env::remove_var("HEDDLE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
