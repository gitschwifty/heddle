use heddle::session::jsonl::load_session;
use tempfile::tempdir;

// Note: the Rust port's `load_session` is lenient — it silently filters
// unparseable lines rather than throwing (TS used to throw). The tests here
// codify Rust's actual behavior.

#[test]
fn load_session_with_malformed_line_returns_only_valid_messages() {
    let d = tempdir().unwrap();
    let p = d.path().join("bad.jsonl");
    std::fs::write(
        &p,
        "{\"role\":\"user\",\"content\":\"ok\"}\nnot valid json\n",
    )
    .unwrap();
    let messages = load_session(&p);
    assert_eq!(messages.len(), 1);
}

#[test]
fn load_session_returns_empty_for_whitespace_only_file() {
    let d = tempdir().unwrap();
    let p = d.path().join("whitespace.jsonl");
    std::fs::write(&p, "   \n  \n\n  ").unwrap();
    assert!(load_session(&p).is_empty());
}

#[test]
fn load_session_returns_empty_for_newlines_only_file() {
    let d = tempdir().unwrap();
    let p = d.path().join("newlines.jsonl");
    std::fs::write(&p, "\n\n\n\n").unwrap();
    assert!(load_session(&p).is_empty());
}

#[test]
fn load_session_returns_empty_for_nonexistent_file() {
    let d = tempdir().unwrap();
    let p = d.path().join("missing.jsonl");
    assert!(load_session(&p).is_empty());
}
