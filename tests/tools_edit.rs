use heddle::tools::edit::create_edit_tool;
use heddle::tools::types::ExecOptions;
use heddle::{checkpoints::diff::snapshot_meta, config::paths::get_file_history_dir};
use serde_json::json;
use tempfile::tempdir;

mod common;
use common::Sandbox;

async fn run_edit(file: &std::path::Path, old: &str, new: &str, replace_all: bool) -> String {
    let tool = create_edit_tool();
    tool.execute(
        json!({
            "file_path": file.to_string_lossy(),
            "old_string": old,
            "new_string": new,
            "replace_all": replace_all,
        }),
        ExecOptions::default(),
    )
    .await
}

#[tokio::test]
async fn exact_match_replacement() {
    let _sb = Sandbox::new("edit-exact");
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\nfoo bar\nbaz").unwrap();

    let result = run_edit(&path, "foo bar", "FOO BAR", false).await;
    assert!(result.contains("Applied edit"), "got: {result}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "hello world\nFOO BAR\nbaz"
    );
}

#[tokio::test]
async fn replace_all_replaces_every_occurrence() {
    let _sb = Sandbox::new("edit-replaceall");
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "aaa bbb aaa ccc aaa").unwrap();

    let result = run_edit(&path, "aaa", "ZZZ", true).await;
    assert!(result.contains("Replaced 3 occurrences"), "got: {result}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "ZZZ bbb ZZZ ccc ZZZ"
    );
}

#[tokio::test]
async fn fails_when_old_string_not_unique() {
    let _sb = Sandbox::new("edit-not-unique");
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "aaa bbb aaa").unwrap();

    let result = run_edit(&path, "aaa", "ZZZ", false).await;
    assert!(result.contains("not unique"), "got: {result}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "aaa bbb aaa");
    assert!(snapshot_meta(None).is_empty());
}

#[tokio::test]
async fn fails_when_old_string_not_found() {
    let _sb = Sandbox::new("edit-not-found");
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world").unwrap();

    let result = run_edit(&path, "nonexistent", "replacement", false).await;
    assert!(result.contains("not found"), "got: {result}");
    assert!(snapshot_meta(None).is_empty());
}

#[tokio::test]
async fn fails_when_file_missing() {
    let _sb = Sandbox::new("edit-missing-file");
    let dir = tempdir().unwrap();
    let path = dir.path().join("nonexistent.txt");

    let result = run_edit(&path, "foo", "bar", false).await;
    assert!(result.contains("not found"), "got: {result}");
}

#[tokio::test]
async fn multiline_replacement() {
    let _sb = Sandbox::new("edit-multiline");
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "line1\nline2\nline3\nline4").unwrap();

    let result = run_edit(&path, "line2\nline3", "REPLACED", false).await;
    assert!(result.contains("Applied edit"), "got: {result}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "line1\nREPLACED\nline4"
    );
}

#[tokio::test]
async fn fuzzy_fallback_whitespace_normalized() {
    let _sb = Sandbox::new("edit-fuzzy-ws");
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello  world\nfoo  bar  baz\nend").unwrap();

    let result = run_edit(&path, "foo bar baz", "REPLACED", false).await;
    assert!(result.contains("Applied edit"), "got: {result}");
    assert!(result.contains("whitespace-normalized"), "got: {result}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "hello  world\nREPLACED\nend"
    );
}

#[tokio::test]
async fn fuzzy_fallback_indent_flexible() {
    let _sb = Sandbox::new("edit-fuzzy-indent");
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "function test() {\n\treturn 1;\n}").unwrap();

    let result = run_edit(
        &path,
        "function test() {\n  return 1;\n}",
        "function test() {\n\treturn 2;\n}",
        false,
    )
    .await;
    assert!(result.contains("Applied edit"), "got: {result}");
    let level_ok = result.contains("indent-flexible")
        || result.contains("whitespace-normalized")
        || result.contains("line-fuzzy");
    assert!(level_ok, "expected a fuzzy match level in: {result}");
}

#[tokio::test]
async fn fuzzy_fallback_all_levels_fail() {
    let _sb = Sandbox::new("edit-fuzzy-all-fail");
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "alpha\nbeta\ngamma\ndelta\nepsilon").unwrap();

    let result = run_edit(&path, "totally_nonexistent_string", "replacement", false).await;
    assert!(result.contains("not found"), "got: {result}");
    assert!(snapshot_meta(None).is_empty());
}

#[tokio::test]
async fn successful_edit_creates_backup_metadata() {
    let sb = Sandbox::new("edit-backup-success");
    let path = sb.project.join("test.txt");
    std::fs::write(&path, "hello world").unwrap();

    let result = run_edit(&path, "world", "there", false).await;
    assert!(result.contains("Applied edit"), "got: {result}");

    let snapshot = snapshot_meta(None);
    assert_eq!(snapshot.len(), 1);
    let history_dir = get_file_history_dir(None);
    assert!(history_dir.join("meta.json").exists());
}
