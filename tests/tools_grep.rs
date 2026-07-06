use heddle::tools::grep::create_grep_tool;
use heddle::tools::types::ExecOptions;
use serde_json::json;
use tempfile::tempdir;

fn setup_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("match.txt"), "hello world\nfoo bar\nbaz\n").unwrap();
    std::fs::write(dir.path().join("code.ts"), "const x = 1;\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "const y = 2;\n").unwrap();
    std::fs::write(dir.path().join("nomatch.txt"), "nothing interesting here\n").unwrap();
    dir
}

#[tokio::test]
async fn returns_matching_lines_with_file_paths() {
    let dir = setup_dir();
    let tool = create_grep_tool();
    let result = tool
        .execute(
            json!({ "pattern": "foo", "path": dir.path().to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("foo bar"), "got: {result}");
    assert!(result.contains("match.txt"), "got: {result}");
}

#[tokio::test]
async fn respects_glob_filter() {
    let dir = setup_dir();
    let tool = create_grep_tool();
    let result = tool
        .execute(
            json!({ "pattern": "const", "path": dir.path().to_string_lossy(), "glob": "*.ts" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("code.ts"), "got: {result}");
    assert!(!result.contains("notes.txt"), "got: {result}");
}

#[tokio::test]
async fn no_match_message_when_pattern_not_found() {
    let dir = setup_dir();
    let tool = create_grep_tool();
    let result = tool
        .execute(
            json!({ "pattern": "zzz_not_here", "path": dir.path().to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("No matches found"), "got: {result}");
}

#[tokio::test]
async fn errors_on_invalid_regex() {
    let dir = setup_dir();
    let tool = create_grep_tool();
    let result = tool
        .execute(
            json!({ "pattern": "[invalid", "path": dir.path().to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("Error"), "got: {result}");
}

#[tokio::test]
async fn no_match_when_glob_excludes_everything() {
    let dir = setup_dir();
    let tool = create_grep_tool();
    let result = tool
        .execute(
            json!({ "pattern": "hello", "path": dir.path().to_string_lossy(), "glob": "*.py" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("No matches found"), "got: {result}");
}

#[tokio::test]
async fn errors_when_path_does_not_exist() {
    let tool = create_grep_tool();
    let result = tool
        .execute(
            json!({ "pattern": "test", "path": "/tmp/heddle-nonexistent-path-xyz-99999" }),
            ExecOptions::default(),
        )
        .await;
    assert!(
        result.contains("Error") || result.contains("No matches"),
        "got: {result}"
    );
}
