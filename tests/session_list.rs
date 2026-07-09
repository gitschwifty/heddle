use std::path::Path;

use heddle::session::list::{find_session, list_sessions};
use serde_json::json;
use tempfile::TempDir;

mod common;

fn tmp() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn session_line(overrides: serde_json::Value) -> String {
    let mut base = json!({
        "type": "session_meta",
        "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        "cwd": "/tmp/test",
        "model": "test-model",
        "created": "2026-01-15T10:00:00.000Z",
        "heddle_version": "0.1.0"
    });
    if let Some(obj) = overrides.as_object() {
        for (k, v) in obj {
            base[k] = v.clone();
        }
    }
    base.to_string()
}

fn message_line(role: &str, content: &str) -> String {
    json!({"role": role, "content": content, "timestamp": "2026-01-15T10:00:01.000Z"}).to_string()
}

fn write_jsonl(path: &Path, lines: &[String]) {
    std::fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
}

#[test]
fn empty_dir_returns_empty() {
    let dir = tmp();
    let sessions = list_sessions(Some(dir.path()));
    assert!(sessions.is_empty());
}

#[test]
fn parses_metas_and_counts_messages() {
    let dir = tmp();
    write_jsonl(
        &dir.path().join("id-1.jsonl"),
        &[
            session_line(json!({"id":"id-1","model":"gpt-4"})),
            message_line("system", "You are helpful"),
            message_line("user", "Hello there"),
            message_line("assistant", "Hi!"),
        ],
    );
    let sessions = list_sessions(Some(dir.path()));
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "id-1");
    assert_eq!(sessions[0].model, "gpt-4");
    assert_eq!(sessions[0].message_count, 3);
    assert_eq!(
        sessions[0].first_user_message.as_deref(),
        Some("Hello there")
    );
}

#[test]
fn sorts_by_created_descending() {
    let dir = tmp();
    write_jsonl(
        &dir.path().join("old.jsonl"),
        &[session_line(
            json!({"id":"old","created":"2026-01-01T00:00:00.000Z"}),
        )],
    );
    write_jsonl(
        &dir.path().join("new.jsonl"),
        &[session_line(
            json!({"id":"new","created":"2026-03-01T00:00:00.000Z"}),
        )],
    );
    write_jsonl(
        &dir.path().join("mid.jsonl"),
        &[session_line(
            json!({"id":"mid","created":"2026-02-01T00:00:00.000Z"}),
        )],
    );
    let sessions = list_sessions(Some(dir.path()));
    let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["new", "mid", "old"]);
}

#[test]
fn truncates_first_user_message_to_100() {
    let dir = tmp();
    let long = "x".repeat(200);
    write_jsonl(
        &dir.path().join("trunc.jsonl"),
        &[
            session_line(json!({"id":"trunc"})),
            message_line("user", &long),
        ],
    );
    let sessions = list_sessions(Some(dir.path()));
    assert!(sessions[0].first_user_message.as_ref().unwrap().len() <= 100);
}

#[test]
fn truncates_first_user_message_on_unicode_boundary() {
    let dir = tmp();
    let long = "é".repeat(120);
    write_jsonl(
        &dir.path().join("unicode.jsonl"),
        &[
            session_line(json!({"id":"unicode"})),
            message_line("user", &long),
        ],
    );
    let sessions = list_sessions(Some(dir.path()));
    let preview = sessions[0].first_user_message.as_ref().unwrap();
    assert_eq!(preview.chars().count(), 100);
}

#[test]
fn skips_files_without_valid_session_meta() {
    let dir = tmp();
    std::fs::write(dir.path().join("bad.jsonl"), r#"{"not":"session_meta"}"#).unwrap();
    write_jsonl(
        &dir.path().join("good.jsonl"),
        &[session_line(json!({"id":"good"}))],
    );
    let sessions = list_sessions(Some(dir.path()));
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "good");
}

#[test]
fn reads_forked_from() {
    let dir = tmp();
    write_jsonl(
        &dir.path().join("forked.jsonl"),
        &[session_line(
            json!({"id":"forked","forked_from":"parent-id"}),
        )],
    );
    let sessions = list_sessions(Some(dir.path()));
    assert_eq!(sessions[0].forked_from.as_deref(), Some("parent-id"));
}

#[test]
fn reads_name_from_session_name_marker() {
    let dir = tmp();
    write_jsonl(
        &dir.path().join("named.jsonl"),
        &[
            session_line(json!({"id":"named"})),
            json!({"type":"session_name","name":"my session","timestamp":"2026-01-15T10:00:00.000Z"}).to_string(),
        ],
    );
    let sessions = list_sessions(Some(dir.path()));
    assert_eq!(sessions[0].name.as_deref(), Some("my session"));
}

// ── find_session ──

#[test]
fn find_most_recent_when_empty_target() {
    let dir = tmp();
    write_jsonl(
        &dir.path().join("old.jsonl"),
        &[session_line(
            json!({"id":"old-id","created":"2026-01-01T00:00:00.000Z"}),
        )],
    );
    write_jsonl(
        &dir.path().join("new.jsonl"),
        &[session_line(
            json!({"id":"new-id","created":"2026-03-01T00:00:00.000Z"}),
        )],
    );
    let result = find_session(Some(""), Some(dir.path())).unwrap();
    assert!(result.to_string_lossy().contains("new.jsonl"));
}

#[test]
fn find_returns_none_when_empty_and_no_sessions() {
    let dir = tmp();
    let result = find_session(Some(""), Some(dir.path()));
    assert!(result.is_none());
}

#[test]
fn find_by_uuid() {
    let dir = tmp();
    let id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    write_jsonl(
        &dir.path().join("test.jsonl"),
        &[session_line(json!({"id":id}))],
    );
    let result = find_session(Some(id), Some(dir.path())).unwrap();
    assert!(result.to_string_lossy().contains("test.jsonl"));
}

#[test]
fn find_by_partial_uuid() {
    let dir = tmp();
    let id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    write_jsonl(
        &dir.path().join("test.jsonl"),
        &[session_line(json!({"id":id}))],
    );
    let result = find_session(Some("aaaaaaaa"), Some(dir.path())).unwrap();
    assert!(result.to_string_lossy().contains("test.jsonl"));
}

#[test]
fn find_by_name() {
    let dir = tmp();
    write_jsonl(
        &dir.path().join("named.jsonl"),
        &[session_line(json!({"id":"named-id","name":"my-session"}))],
    );
    let result = find_session(Some("my-session"), Some(dir.path())).unwrap();
    assert!(result.to_string_lossy().contains("named.jsonl"));
}

#[test]
fn find_returns_none_for_unknown() {
    let dir = tmp();
    write_jsonl(&dir.path().join("test.jsonl"), &[session_line(json!({}))]);
    let result = find_session(Some("nonexistent-id"), Some(dir.path()));
    assert!(result.is_none());
}
