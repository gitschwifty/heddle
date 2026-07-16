use heddle::session::jsonl::{
    append_context_marker, append_message, load_session, load_session_meta, write_session_meta,
    SessionMeta, CONTEXT_RESET_MARKER_TYPE,
};
use heddle::types::{
    AssistantMessage, FunctionCall, Message, ToolCall, ToolCallKind, ToolMessage, UserMessage,
};
use serde_json::json;
use tempfile::TempDir;

mod common;

fn tmp() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn meta(id: &str) -> SessionMeta {
    SessionMeta {
        kind: "session_meta".into(),
        id: id.into(),
        cwd: "/home/user/repos/heddle".into(),
        model: "test-model".into(),
        created: "2026-02-18T20:01:46Z".into(),
        heddle_version: "0.1.0".into(),
        name: None,
        forked_from: None,
        extra: Default::default(),
    }
}

// ── append_message ──

#[test]
fn append_creates_file_if_missing() {
    let dir = tmp();
    let path = dir.path().join("new-session.jsonl");
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    assert!(path.exists());
}

#[test]
fn append_user_message_as_json_line() {
    let dir = tmp();
    let path = dir.path().join("append.jsonl");
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
    assert_eq!(v["role"], "user");
    assert_eq!(v["content"], "Hello");
}

#[test]
fn append_includes_iso_timestamp() {
    let dir = tmp();
    let path = dir.path().join("timestamp.jsonl");
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
    let ts = v["timestamp"].as_str().unwrap();
    chrono::DateTime::parse_from_rfc3339(ts).expect("valid ISO timestamp");
}

#[test]
fn append_multiple_messages_separate_lines() {
    let dir = tmp();
    let path = dir.path().join("multi.jsonl");
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    append_message(
        &path,
        &Message::Assistant(AssistantMessage {
            content: Some("Hi there!".into()),
            tool_calls: None,
        }),
    )
    .unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2);
    let v0: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let v1: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(v0["role"], "user");
    assert_eq!(v1["role"], "assistant");
}

#[test]
fn append_assistant_with_tool_calls() {
    let dir = tmp();
    let path = dir.path().join("tool-calls.jsonl");
    append_message(
        &path,
        &Message::Assistant(AssistantMessage {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".into(),
                kind: ToolCallKind::Function,
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"/tmp/test.txt"}"#.into(),
                },
            }]),
        }),
    )
    .unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
    assert_eq!(v["role"], "assistant");
    assert_eq!(v["tool_calls"][0]["function"]["name"], "read_file");
    assert!(v.get("timestamp").is_some());
}

#[test]
fn append_tool_result_message() {
    let dir = tmp();
    let path = dir.path().join("tool-result.jsonl");
    append_message(
        &path,
        &Message::Tool(ToolMessage {
            tool_call_id: "call_1".into(),
            content: "file contents here".into(),
        }),
    )
    .unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
    assert_eq!(v["role"], "tool");
    assert_eq!(v["tool_call_id"], "call_1");
    assert!(v.get("timestamp").is_some());
}

#[test]
fn append_creates_parent_dirs() {
    let dir = tmp();
    let path = dir.path().join("nested/deep/session.jsonl");
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    assert!(path.exists());
}

// ── write_session_meta / load_session_meta ──

#[test]
fn write_session_meta_as_first_line() {
    let dir = tmp();
    let path = dir.path().join("meta.jsonl");
    write_session_meta(&path, &meta("test-uuid-1234")).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
    assert_eq!(v["type"], "session_meta");
    assert_eq!(v["id"], "test-uuid-1234");
}

#[test]
fn load_session_meta_reads_header() {
    let dir = tmp();
    let path = dir.path().join("meta-load.jsonl");
    write_session_meta(&path, &meta("test-uuid-5678")).unwrap();
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    let loaded = load_session_meta(&path).unwrap();
    assert_eq!(loaded.id, "test-uuid-5678");
    assert_eq!(loaded.model, "test-model");
}

#[test]
fn load_session_meta_none_for_missing_file() {
    let dir = tmp();
    let path = dir.path().join("nope.jsonl");
    assert!(load_session_meta(&path).is_none());
}

#[test]
fn load_session_meta_none_when_no_meta() {
    let dir = tmp();
    let path = dir.path().join("no-meta.jsonl");
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    assert!(load_session_meta(&path).is_none());
}

// ── load_session ──

#[test]
fn load_session_empty_for_missing_file() {
    let dir = tmp();
    let messages = load_session(&dir.path().join("nonexistent.jsonl"));
    assert!(messages.is_empty());
}

#[test]
fn load_session_empty_for_empty_file() {
    let dir = tmp();
    let path = dir.path().join("empty.jsonl");
    std::fs::write(&path, "").unwrap();
    let messages = load_session(&path);
    assert!(messages.is_empty());
}

#[test]
fn load_single_message() {
    let dir = tmp();
    let path = dir.path().join("single.jsonl");
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    let messages = load_session(&path);
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0], Message::User(_)));
}

#[test]
fn load_multiple_messages_in_order() {
    let dir = tmp();
    let path = dir.path().join("multiple.jsonl");
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    append_message(
        &path,
        &Message::Assistant(AssistantMessage {
            content: Some("Hi!".into()),
            tool_calls: None,
        }),
    )
    .unwrap();
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "How are you?".into(),
        }),
    )
    .unwrap();
    let messages = load_session(&path);
    assert_eq!(messages.len(), 3);
    assert!(matches!(messages[0], Message::User(_)));
    assert!(matches!(messages[1], Message::Assistant(_)));
    assert!(matches!(messages[2], Message::User(_)));
}

#[test]
fn load_skips_session_meta() {
    let dir = tmp();
    let path = dir.path().join("with-meta.jsonl");
    write_session_meta(&path, &meta("test-uuid")).unwrap();
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    append_message(
        &path,
        &Message::Assistant(AssistantMessage {
            content: Some("Hi!".into()),
            tool_calls: None,
        }),
    )
    .unwrap();
    let messages = load_session(&path);
    assert_eq!(messages.len(), 2);
}

#[test]
fn load_skips_blank_lines() {
    let dir = tmp();
    let path = dir.path().join("blanks.jsonl");
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    use std::fs::OpenOptions;
    use std::io::Write;
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"\n\n")
        .unwrap();
    let messages = load_session(&path);
    assert_eq!(messages.len(), 1);
}

// ── append_context_marker ──

#[test]
fn writes_context_marker_line() {
    let dir = tmp();
    let path = dir.path().join("marker.jsonl");
    append_context_marker(
        &path,
        &json!({
            "type": "context_prune",
            "messages_pruned": 3,
            "tokens_before": 50000,
            "tokens_after": 30000,
            "timestamp": "2026-03-26T12:00:00.000Z",
        }),
    )
    .unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
    assert_eq!(v["type"], "context_prune");
    assert_eq!(v["messages_pruned"], 3);
}

#[test]
fn load_session_skips_context_markers() {
    let dir = tmp();
    let path = dir.path().join("with-marker.jsonl");
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "Hello".into(),
        }),
    )
    .unwrap();
    append_context_marker(
        &path,
        &json!({"type":"context_prune","messages_pruned":1,"tokens_before":1000,"tokens_after":500}),
    )
    .unwrap();
    append_message(
        &path,
        &Message::Assistant(AssistantMessage {
            content: Some("Hi!".into()),
            tool_calls: None,
        }),
    )
    .unwrap();
    let messages = load_session(&path);
    assert_eq!(messages.len(), 2);
}

#[test]
fn load_session_resumes_after_last_context_reset_marker() {
    let dir = tmp();
    let path = dir.path().join("with-reset.jsonl");
    append_message(
        &path,
        &Message::System(heddle::types::SystemMessage {
            content: "old system".into(),
        }),
    )
    .unwrap();
    append_message(
        &path,
        &Message::User(UserMessage {
            content: "old prompt".into(),
        }),
    )
    .unwrap();
    append_context_marker(
        &path,
        &json!({"type": CONTEXT_RESET_MARKER_TYPE, "timestamp": "2026-07-16T00:00:00Z"}),
    )
    .unwrap();
    append_message(
        &path,
        &Message::System(heddle::types::SystemMessage {
            content: "new system".into(),
        }),
    )
    .unwrap();

    let messages = load_session(&path);

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content_str(), Some("new system"));
}

// ── round-trip ──

#[test]
fn roundtrip_preserves_messages() {
    let dir = tmp();
    let path = dir.path().join("roundtrip.jsonl");
    write_session_meta(&path, &meta("rt-uuid")).unwrap();
    let msgs: Vec<Message> = vec![
        Message::System(heddle::types::SystemMessage {
            content: "You are a helpful assistant.".into(),
        }),
        Message::User(UserMessage {
            content: "Hello".into(),
        }),
        Message::Assistant(AssistantMessage {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".into(),
                kind: ToolCallKind::Function,
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"/tmp/a.txt"}"#.into(),
                },
            }]),
        }),
        Message::Tool(ToolMessage {
            tool_call_id: "call_1".into(),
            content: "file content".into(),
        }),
        Message::Assistant(AssistantMessage {
            content: Some("Here is the file content.".into()),
            tool_calls: None,
        }),
    ];
    for m in &msgs {
        append_message(&path, m).unwrap();
    }
    let loaded = load_session(&path);
    assert_eq!(loaded.len(), msgs.len());
    for (i, m) in msgs.iter().enumerate() {
        assert_eq!(loaded[i].role(), m.role());
    }
    let m = load_session_meta(&path).unwrap();
    assert_eq!(m.id, "rt-uuid");
}
