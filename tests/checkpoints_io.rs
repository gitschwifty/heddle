use heddle::checkpoints::io::{load_checkpoints, write_checkpoint};
use heddle::checkpoints::record::{CheckpointRecord, FileChange};
use heddle::session::jsonl::{
    append_context_marker, append_message, write_session_meta, SessionMeta,
};
use heddle::types::{Message, UserMessage};

mod common;
use common::Sandbox;

fn make_record(turn: u64, preview: &str, paths: &[(&str, u32, u32)]) -> CheckpointRecord {
    let changes = paths
        .iter()
        .map(|(p, before, after)| FileChange {
            file_path: (*p).into(),
            uuid: format!("uuid-{p}"),
            version_before: *before,
            version_after: *after,
        })
        .collect();
    CheckpointRecord::new(turn, 0, preview.into(), changes)
}

#[test]
fn write_and_load_round_trip() {
    let sb = Sandbox::new("cp-io-rt");
    let session = sb.root.join("session.jsonl");
    let meta = SessionMeta {
        kind: "session_meta".into(),
        id: "s1".into(),
        cwd: sb.root.to_string_lossy().to_string(),
        model: "test".into(),
        created: "2026-05-22T00:00:00Z".into(),
        heddle_version: "0".into(),
        name: None,
        forked_from: None,
        extra: Default::default(),
    };
    write_session_meta(&session, &meta).unwrap();

    let r = make_record(1, "hello", &[("/foo.rs", 0, 1)]);
    write_checkpoint(&session, &r).unwrap();

    let loaded = load_checkpoints(&session);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].turn_index, 1);
    assert_eq!(loaded[0].user_preview, "hello");
    assert_eq!(loaded[0].changes.len(), 1);
    assert_eq!(loaded[0].changes[0].file_path, "/foo.rs");
    assert_eq!(loaded[0].kind, "checkpoint");
}

#[test]
fn load_returns_empty_when_session_missing() {
    let sb = Sandbox::new("cp-io-missing");
    let session = sb.root.join("nope.jsonl");
    let loaded = load_checkpoints(&session);
    assert!(loaded.is_empty());
}

#[test]
fn load_skips_non_checkpoint_lines() {
    let sb = Sandbox::new("cp-io-mixed");
    let session = sb.root.join("session.jsonl");
    // Mix: meta, user msg, an unrelated marker, then a checkpoint.
    let meta = SessionMeta {
        kind: "session_meta".into(),
        id: "s1".into(),
        cwd: sb.root.to_string_lossy().to_string(),
        model: "test".into(),
        created: "2026-05-22T00:00:00Z".into(),
        heddle_version: "0".into(),
        name: None,
        forked_from: None,
        extra: Default::default(),
    };
    write_session_meta(&session, &meta).unwrap();
    append_message(
        &session,
        &Message::User(UserMessage {
            content: "hi".into(),
        }),
    )
    .unwrap();
    append_context_marker(
        &session,
        &serde_json::json!({"type": "context_prune", "tokens_after": 100}),
    )
    .unwrap();
    write_checkpoint(&session, &make_record(2, "edit", &[("/x.rs", 1, 2)])).unwrap();

    let loaded = load_checkpoints(&session);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].turn_index, 2);
}

#[test]
fn load_preserves_order() {
    let sb = Sandbox::new("cp-io-order");
    let session = sb.root.join("session.jsonl");
    let meta = SessionMeta {
        kind: "session_meta".into(),
        id: "s1".into(),
        cwd: sb.root.to_string_lossy().to_string(),
        model: "test".into(),
        created: "2026-05-22T00:00:00Z".into(),
        heddle_version: "0".into(),
        name: None,
        forked_from: None,
        extra: Default::default(),
    };
    write_session_meta(&session, &meta).unwrap();

    for turn in [3, 1, 2] {
        write_checkpoint(
            &session,
            &make_record(turn, &format!("turn-{turn}"), &[("/a.rs", 0, 1)]),
        )
        .unwrap();
    }

    let loaded = load_checkpoints(&session);
    let turns: Vec<u64> = loaded.iter().map(|r| r.turn_index).collect();
    assert_eq!(turns, vec![3, 1, 2]);
}
