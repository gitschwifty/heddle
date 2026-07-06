use heddle::history::reader::{load_history, LoadHistoryOptions};

mod common;
use common::Sandbox;

fn write_history(sb: &Sandbox) {
    let entries = [
        r#"{"timestamp":"2026-03-29T10:00:00.000Z","session_id":"s1","project":"/p1","message_preview":"first message","content_type":"text"}"#,
        r#"{"timestamp":"2026-03-29T11:00:00.000Z","session_id":"s1","project":"/p1","message_preview":"second message","content_type":"mention"}"#,
        r#"{"timestamp":"2026-03-29T12:00:00.000Z","session_id":"s2","project":"/p2","message_preview":"third message with search term","content_type":"text"}"#,
        r#"{"timestamp":"2026-03-29T13:00:00.000Z","session_id":"s2","project":"/p2","message_preview":"fourth message","content_type":"shell"}"#,
    ];
    std::fs::write(
        sb.heddle_home.join("history.jsonl"),
        format!("{}\n", entries.join("\n")),
    )
    .unwrap();
}

#[test]
fn loads_all_entries_no_options() {
    let sb = Sandbox::new("hreader-all");
    write_history(&sb);
    let result = load_history(&LoadHistoryOptions::default());
    assert_eq!(result.len(), 4);
}

#[test]
fn limits_results_with_limit() {
    let sb = Sandbox::new("hreader-limit");
    write_history(&sb);
    let result = load_history(&LoadHistoryOptions {
        limit: Some(2),
        search: None,
    });
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].message_preview, "third message with search term");
    assert_eq!(result[1].message_preview, "fourth message");
}

#[test]
fn filters_by_search() {
    let sb = Sandbox::new("hreader-search");
    write_history(&sb);
    let result = load_history(&LoadHistoryOptions {
        limit: None,
        search: Some("search term".into()),
    });
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].message_preview, "third message with search term");
}

#[test]
fn search_case_insensitive() {
    let sb = Sandbox::new("hreader-case");
    write_history(&sb);
    let result = load_history(&LoadHistoryOptions {
        limit: None,
        search: Some("SEARCH TERM".into()),
    });
    assert_eq!(result.len(), 1);
}

#[test]
fn combines_limit_and_search() {
    let sb = Sandbox::new("hreader-combo");
    write_history(&sb);
    let result = load_history(&LoadHistoryOptions {
        limit: Some(2),
        search: Some("message".into()),
    });
    assert_eq!(result.len(), 2);
}

#[test]
fn empty_when_file_missing() {
    let _sb = Sandbox::new("hreader-missing");
    let result = load_history(&LoadHistoryOptions::default());
    assert!(result.is_empty());
}
