//! Debug channel filtering + file-output tests. Uses `HEDDLE_DEBUG_FILE` for
//! deterministic output capture so each test reads from a per-tempdir log.

use heddle::debug::{clear_log_file, debug, reset_debug};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use tempfile::tempdir;

// Serializes env-mutating tests (HEDDLE_DEBUG / HEDDLE_DEBUG_FILE are process-wide).
static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

struct EnvGuard {
    orig_debug: Option<String>,
    orig_file: Option<String>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn new() -> Self {
        let g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        Self {
            orig_debug: std::env::var("HEDDLE_DEBUG").ok(),
            orig_file: std::env::var("HEDDLE_DEBUG_FILE").ok(),
            _guard: g,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.orig_debug {
            Some(v) => std::env::set_var("HEDDLE_DEBUG", v),
            None => std::env::remove_var("HEDDLE_DEBUG"),
        }
        match &self.orig_file {
            Some(v) => std::env::set_var("HEDDLE_DEBUG_FILE", v),
            None => std::env::remove_var("HEDDLE_DEBUG_FILE"),
        }
        reset_debug();
    }
}

fn read_log(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn silent_when_heddle_debug_is_not_set() {
    let _g = EnvGuard::new();
    let d = tempdir().unwrap();
    let log = d.path().join("debug.log");
    std::env::remove_var("HEDDLE_DEBUG");
    std::env::set_var("HEDDLE_DEBUG_FILE", &log);
    reset_debug();
    debug("provider", "test message");
    assert!(!log.exists() || read_log(&log).is_empty());
}

#[test]
fn heddle_debug_1_enables_all_channels() {
    let _g = EnvGuard::new();
    let d = tempdir().unwrap();
    let log = d.path().join("debug.log");
    std::env::set_var("HEDDLE_DEBUG", "1");
    std::env::set_var("HEDDLE_DEBUG_FILE", &log);
    reset_debug();
    debug("provider", "hello");
    debug("config", "world");
    let content = read_log(&log);
    assert!(content.contains("[heddle:provider]"));
    assert!(content.contains("[heddle:config]"));
    assert!(content.contains("hello"));
    assert!(content.contains("world"));
}

#[test]
fn heddle_debug_true_enables_all_channels() {
    let _g = EnvGuard::new();
    let d = tempdir().unwrap();
    let log = d.path().join("debug.log");
    std::env::set_var("HEDDLE_DEBUG", "true");
    std::env::set_var("HEDDLE_DEBUG_FILE", &log);
    reset_debug();
    debug("anything", "test");
    assert!(read_log(&log).contains("[heddle:anything]"));
}

#[test]
fn heddle_debug_provider_only_enables_provider_channel() {
    let _g = EnvGuard::new();
    let d = tempdir().unwrap();
    let log = d.path().join("debug.log");
    std::env::set_var("HEDDLE_DEBUG", "provider");
    std::env::set_var("HEDDLE_DEBUG_FILE", &log);
    reset_debug();
    debug("provider", "yes");
    debug("config", "no");
    let content = read_log(&log);
    assert!(content.contains("yes"));
    assert!(!content.contains("[heddle:config]"));
}

#[test]
fn heddle_debug_csv_enables_listed_channels() {
    let _g = EnvGuard::new();
    let d = tempdir().unwrap();
    let log = d.path().join("debug.log");
    std::env::set_var("HEDDLE_DEBUG", "provider,config");
    std::env::set_var("HEDDLE_DEBUG_FILE", &log);
    reset_debug();
    debug("provider", "p");
    debug("config", "c");
    debug("other", "o");
    let content = read_log(&log);
    assert!(content.contains("[heddle:provider]"));
    assert!(content.contains("[heddle:config]"));
    assert!(!content.contains("[heddle:other]"));
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn heddle_debug_file_writes_to_log_file() {
    let _g = EnvGuard::new();
    let d = tempdir().unwrap();
    let log = d.path().join("debug.log");
    std::env::set_var("HEDDLE_DEBUG", "1");
    std::env::set_var("HEDDLE_DEBUG_FILE", &log);
    reset_debug();
    debug("provider", "file log test");
    debug("config", "second line");
    assert!(log.exists());
    let content = read_log(&log);
    let lines: Vec<&str> = content.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("[heddle:provider]"));
    assert!(lines[0].contains("file log test"));
    assert!(lines[1].contains("[heddle:config]"));
    assert!(lines[1].contains("second line"));
    // ISO 8601 timestamp prefix
    assert!(lines[0].starts_with(char::is_numeric));
    let first_4: String = lines[0].chars().take(4).collect();
    assert!(first_4.parse::<u32>().is_ok(), "got {first_4}");
}

#[test]
fn clear_log_file_empties_the_log() {
    let _g = EnvGuard::new();
    let d = tempdir().unwrap();
    let log = d.path().join("debug.log");
    std::env::set_var("HEDDLE_DEBUG", "1");
    std::env::set_var("HEDDLE_DEBUG_FILE", &log);
    reset_debug();
    debug("provider", "before clear");
    assert!(!read_log(&log).is_empty());
    clear_log_file();
    assert_eq!(read_log(&log), "");
    debug("provider", "after clear");
    let content = read_log(&log).trim().to_string();
    assert_eq!(content.lines().count(), 1);
    assert!(content.contains("after clear"));
}
