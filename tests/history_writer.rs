use heddle::history::writer::{append_history_entry, ContentType, HistoryEntry};

mod common;
use common::Sandbox;

#[test]
fn appends_single_entry_as_jsonl() {
    let sb = Sandbox::new("hwriter-single");
    let entry = HistoryEntry {
        timestamp: "2026-03-29T12:00:00.000Z".into(),
        session_id: "test-session-1".into(),
        project: "/tmp/project".into(),
        message_preview: "hello world".into(),
        content_type: ContentType::Text,
    };
    append_history_entry(&entry).unwrap();
    let path = sb.heddle_home.join("history.jsonl");
    let content = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 1);
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(v["timestamp"], "2026-03-29T12:00:00.000Z");
    assert_eq!(v["session_id"], "test-session-1");
    assert_eq!(v["project"], "/tmp/project");
    assert_eq!(v["message_preview"], "hello world");
    assert_eq!(v["content_type"], "text");
}

#[test]
fn appends_multiple_separate_lines() {
    let sb = Sandbox::new("hwriter-multi");
    append_history_entry(&HistoryEntry {
        timestamp: "2026-03-29T12:00:00.000Z".into(),
        session_id: "s1".into(),
        project: "/tmp/project".into(),
        message_preview: "first".into(),
        content_type: ContentType::Text,
    })
    .unwrap();
    append_history_entry(&HistoryEntry {
        timestamp: "2026-03-29T12:01:00.000Z".into(),
        session_id: "s1".into(),
        project: "/tmp/project".into(),
        message_preview: "second".into(),
        content_type: ContentType::Mention,
    })
    .unwrap();
    let path = sb.heddle_home.join("history.jsonl");
    let content = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2);
    let v: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(v["content_type"], "mention");
}

#[test]
fn handles_shell_content_type() {
    let sb = Sandbox::new("hwriter-shell");
    append_history_entry(&HistoryEntry {
        timestamp: "2026-03-29T12:02:00.000Z".into(),
        session_id: "s3".into(),
        project: "/tmp/project".into(),
        message_preview: "run ls".into(),
        content_type: ContentType::Shell,
    })
    .unwrap();
    let path = sb.heddle_home.join("history.jsonl");
    let content = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
    assert_eq!(v["content_type"], "shell");
}
