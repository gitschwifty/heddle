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
async fn supports_regex_alternation() {
    let dir = setup_dir();
    let tool = create_grep_tool();
    let result = tool
        .execute(
            json!({ "pattern": "hello|baz", "path": dir.path().to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("hello world"), "got: {result}");
    assert!(result.contains("baz"), "got: {result}");
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

#[tokio::test]
async fn bounds_large_search_output_with_a_refinement_hint() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("many-matches.txt"),
        "needle match with enough context to exercise output bounds\n".repeat(2_000),
    )
    .unwrap();
    let tool = create_grep_tool();

    let result = tool
        .execute(
            json!({ "pattern": "needle", "path": dir.path().to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;

    assert!(result.contains("search output truncated"), "got: {result}");
    assert!(
        result.len() < 22 * 1024,
        "result was {} bytes",
        result.len()
    );
}
