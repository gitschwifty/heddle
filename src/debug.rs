//! Channel-based debug logging — mirrors `ts-src/debug.ts`.
//!
//! Channels are configured via the `HEDDLE_DEBUG` env var (comma-separated list,
//! or `1`/`true` for everything). Output goes to a file if `HEDDLE_DEBUG_FILE`
//! is set, otherwise stderr in headless mode and stdout otherwise.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::RwLock;

use chrono::Utc;
use once_cell::sync::Lazy;

#[derive(Default)]
struct DebugState {
    channels: HashSet<String>,
    debug_all: bool,
    headless: bool,
    log_file: Option<String>,
}

static STATE: Lazy<RwLock<DebugState>> = Lazy::new(|| RwLock::new(load_state()));

fn load_state() -> DebugState {
    let mut state = DebugState::default();
    state.log_file = std::env::var("HEDDLE_DEBUG_FILE").ok();
    match std::env::var("HEDDLE_DEBUG") {
        Ok(val) if val == "1" || val == "true" => state.debug_all = true,
        Ok(val) if !val.is_empty() => {
            for ch in val.split(',') {
                state.channels.insert(ch.trim().to_string());
            }
        }
        _ => {}
    }
    state
}

/// Re-read `HEDDLE_DEBUG`/`HEDDLE_DEBUG_FILE` env vars. Used by tests.
pub fn reset_debug() {
    *STATE.write().unwrap() = load_state();
}

pub fn set_headless(value: bool) {
    STATE.write().unwrap().headless = value;
}

/// Emit a debug message on `channel`. No-op unless the channel (or `*`) is
/// enabled.
pub fn debug(channel: &str, msg: &str) {
    let state = STATE.read().unwrap();
    if !state.debug_all && !state.channels.contains(channel) {
        return;
    }
    let prefix = format!("[heddle:{channel}]");
    if let Some(log_path) = &state.log_file {
        let timestamp = Utc::now().to_rfc3339();
        if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(log_path) {
            let _ = writeln!(file, "{timestamp} {prefix} {msg}");
        }
    } else if state.headless {
        eprintln!("{prefix} {msg}");
    } else {
        eprintln!("{prefix} {msg}");
    }
}

/// Truncate the log file if one is configured.
pub fn clear_log_file() {
    let state = STATE.read().unwrap();
    if let Some(path) = &state.log_file {
        let _ = std::fs::write(path, "");
    }
}

/// Build a debug message from a list of values. Convenience for places where
/// the TS code did `debug("ch", "msg:", obj)`.
pub fn debug_fmt(channel: &str, parts: &[&dyn std::fmt::Debug]) {
    let state = STATE.read().unwrap();
    if !state.debug_all && !state.channels.contains(channel) {
        return;
    }
    drop(state);
    let msg = parts
        .iter()
        .map(|p| format!("{p:?}"))
        .collect::<Vec<_>>()
        .join(" ");
    debug(channel, &msg);
}
