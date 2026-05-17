//! Fixture-based replay tests. The TS port spawns the headless binary,
//! pipes `.in.jsonl` through it, and compares stdout to `.out.jsonl`. That
//! full integration belongs in Tier 5 (headless). Here we cover the parts
//! that don't need a live binary:
//!
//! 1. Every fixture line is a well-formed `IpcRequest` (`.in.jsonl`) or
//!    `IpcResponse` (`.out.jsonl`) — schema parity check.
//! 2. The IGNORE_PATHS normalization logic that the TS replay uses to
//!    compare against non-deterministic fields.

use heddle::ipc::types::{IpcRequest, IpcResponse};
use serde_json::Value;
use std::path::PathBuf;

const FIXTURE_NAMES: &[&str] = &["normal", "error", "cancel", "heartbeat", "version-mismatch"];

const IGNORE_PATHS: &[&str] = &[
    "session_id",
    "timestamp",
    "usage.prompt_tokens",
    "usage.completion_tokens",
    "usage.total_tokens",
    "event.result_preview",
    "event.details",
    "event.provider",
    "task_id",
    "worker_id",
    "model_latency_ms",
    "tool_latency_ms",
    "total_latency_ms",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ts-test/ipc/fixtures")
}

fn load_lines(path: &PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {path:?}: {e}"))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect()
}

fn delete_path(obj: &mut Value, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut cur = obj;
    for key in &parts[..parts.len() - 1] {
        match cur.get_mut(*key) {
            Some(v) if v.is_object() => cur = v,
            _ => return,
        }
    }
    if let Some(obj) = cur.as_object_mut() {
        obj.remove(parts[parts.len() - 1]);
    }
}

fn strip_ignored(v: &Value) -> Value {
    let mut clone = v.clone();
    for p in IGNORE_PATHS {
        delete_path(&mut clone, p);
    }
    clone
}

#[test]
fn every_input_fixture_line_parses_as_ipc_request() {
    for name in FIXTURE_NAMES {
        let path = fixtures_dir().join(format!("{name}.in.jsonl"));
        for (i, line) in load_lines(&path).into_iter().enumerate() {
            let r: Result<IpcRequest, _> = serde_json::from_str(&line);
            assert!(
                r.is_ok(),
                "{name}.in.jsonl line {i} did not parse: {line}\nerr={:?}",
                r.err()
            );
        }
    }
}

#[test]
fn every_output_fixture_line_parses_as_ipc_response() {
    for name in FIXTURE_NAMES {
        let path = fixtures_dir().join(format!("{name}.out.jsonl"));
        for (i, line) in load_lines(&path).into_iter().enumerate() {
            let r: Result<IpcResponse, _> = serde_json::from_str(&line);
            assert!(
                r.is_ok(),
                "{name}.out.jsonl line {i} did not parse: {line}\nerr={:?}",
                r.err()
            );
        }
    }
}

#[test]
fn strip_ignored_removes_top_level_session_id() {
    let v = serde_json::json!({
        "type": "init_ok",
        "id": "1",
        "session_id": "sess-anything",
        "protocol_version": "0.2.0"
    });
    let s = strip_ignored(&v);
    assert!(s.get("session_id").is_none());
    assert_eq!(s["type"], "init_ok");
}

#[test]
fn strip_ignored_removes_nested_event_fields() {
    let v = serde_json::json!({
        "type": "event",
        "event": {
            "event": "tool_end",
            "name": "glob",
            "result_preview": "junk"
        },
        "event_seq": 1,
        "send_id": "2"
    });
    let s = strip_ignored(&v);
    assert!(s["event"].get("result_preview").is_none(), "got: {s}");
    assert_eq!(s["event"]["name"], "glob");
}

#[test]
fn strip_ignored_removes_usage_subfields() {
    let v = serde_json::json!({
        "type": "result",
        "id": "2",
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
    });
    let s = strip_ignored(&v);
    assert!(s["usage"].get("prompt_tokens").is_none());
    assert!(s["usage"].get("completion_tokens").is_none());
    assert!(s["usage"].get("total_tokens").is_none());
}

#[test]
fn strip_ignored_is_no_op_when_paths_absent() {
    let v = serde_json::json!({
        "type": "event",
        "event": { "event": "content_delta", "text": "hi" },
        "event_seq": 0,
        "send_id": "2"
    });
    let s = strip_ignored(&v);
    assert_eq!(s, v);
}

#[test]
fn stripped_fixture_lines_still_compare_byte_stable() {
    // Stripping the same value twice must be idempotent (a stable
    // comparison key).
    let path = fixtures_dir().join("normal.out.jsonl");
    for (i, line) in load_lines(&path).into_iter().enumerate() {
        let v: Value = serde_json::from_str(&line).unwrap();
        let once = strip_ignored(&v);
        let twice = strip_ignored(&once);
        assert_eq!(
            once, twice,
            "normal.out.jsonl line {i}: stripping is not idempotent"
        );
    }
}
