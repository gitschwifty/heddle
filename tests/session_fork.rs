use std::path::Path;

use heddle::session::fork::{fork_session, ForkOptions};
use serde_json::{json, Value};
use tempfile::TempDir;

mod common;

fn tmp() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn session_line() -> String {
    json!({
        "type": "session_meta",
        "id": "original-id",
        "cwd": "/tmp/test",
        "model": "test-model",
        "created": "2026-01-15T10:00:00.000Z",
        "heddle_version": "0.1.0"
    })
    .to_string()
}

fn message_line(role: &str, content: &str) -> String {
    json!({"role": role, "content": content, "timestamp": "2026-01-15T10:00:01.000Z"}).to_string()
}

fn parse_lines(content: &str) -> Vec<Value> {
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn write_jsonl(path: &Path, lines: &[String]) {
    std::fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
}

#[test]
fn creates_new_file_with_forked_from() {
    let dir = tmp();
    let source = dir.path().join("original.jsonl");
    write_jsonl(&source, &[session_line(), message_line("user", "hello")]);
    let result = fork_session(&source, ForkOptions::default()).unwrap();
    assert!(result.session_file.exists());
    assert_ne!(result.session_id, "original-id");
    let content = std::fs::read_to_string(&result.session_file).unwrap();
    let parsed = parse_lines(&content);
    let meta = &parsed[0];
    assert_eq!(meta["type"], "session_meta");
    assert_eq!(meta["forked_from"], "original-id");
    assert_eq!(meta["id"], result.session_id);
}

#[test]
fn copies_all_messages() {
    let dir = tmp();
    let source = dir.path().join("source.jsonl");
    write_jsonl(
        &source,
        &[
            session_line(),
            message_line("system", "system prompt"),
            message_line("user", "hello"),
            message_line("assistant", "hi there"),
        ],
    );
    let result = fork_session(&source, ForkOptions::default()).unwrap();
    let content = std::fs::read_to_string(&result.session_file).unwrap();
    let parsed = parse_lines(&content);
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[1]["role"], "system");
    assert_eq!(parsed[2]["role"], "user");
    assert_eq!(parsed[3]["role"], "assistant");
}

#[test]
fn truncates_with_up_to_message() {
    let dir = tmp();
    let source = dir.path().join("trunc.jsonl");
    write_jsonl(
        &source,
        &[
            session_line(),
            message_line("system", "system prompt"),
            message_line("user", "first"),
            message_line("assistant", "response 1"),
            message_line("user", "second"),
            message_line("assistant", "response 2"),
        ],
    );
    let result = fork_session(
        &source,
        ForkOptions {
            up_to_message: Some(2),
        },
    )
    .unwrap();
    let content = std::fs::read_to_string(&result.session_file).unwrap();
    let parsed = parse_lines(&content);
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[1]["role"], "system");
    assert_eq!(parsed[2]["role"], "user");
}

#[test]
fn preserves_original_unchanged() {
    let dir = tmp();
    let source = dir.path().join("preserve.jsonl");
    let original = format!(
        "{}\n",
        [session_line(), message_line("user", "hello")].join("\n")
    );
    std::fs::write(&source, &original).unwrap();
    fork_session(&source, ForkOptions::default()).unwrap();
    let after = std::fs::read_to_string(&source).unwrap();
    assert_eq!(after, original);
}

#[test]
fn forked_file_in_same_dir() {
    let dir = tmp();
    let source = dir.path().join("source.jsonl");
    std::fs::write(&source, format!("{}\n", session_line())).unwrap();
    let result = fork_session(&source, ForkOptions::default()).unwrap();
    let result_dir = result.session_file.parent().unwrap();
    let expected = std::fs::canonicalize(dir.path()).unwrap();
    let actual = std::fs::canonicalize(result_dir).unwrap();
    assert_eq!(actual, expected);
}
