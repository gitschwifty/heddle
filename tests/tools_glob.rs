use heddle::tools::glob::create_glob_tool;
use heddle::tools::types::ExecOptions;
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn no_match_for_pattern_with_no_hits() {
    let dir = tempdir().unwrap();
    let tool = create_glob_tool();
    let result = tool
        .execute(
            json!({ "pattern": "*.nonexistent_ext", "path": dir.path().to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("No files matched"), "got: {result}");
}

#[tokio::test]
async fn no_match_for_empty_directory() {
    let dir = tempdir().unwrap();
    let tool = create_glob_tool();
    let result = tool
        .execute(
            json!({ "pattern": "*", "path": dir.path().to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("No files matched"), "got: {result}");
}

#[tokio::test]
async fn handles_nonexistent_directory() {
    let dir = tempdir().unwrap();
    let bad_path = dir.path().join("does-not-exist");
    let tool = create_glob_tool();
    let result = tool
        .execute(
            json!({ "pattern": "*.rs", "path": bad_path.to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(
        result.contains("No files matched") || result.contains("Error"),
        "got: {result}"
    );
}

#[tokio::test]
async fn finds_matching_files() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "x").unwrap();
    std::fs::write(dir.path().join("b.txt"), "y").unwrap();
    std::fs::write(dir.path().join("ignored.md"), "z").unwrap();

    let tool = create_glob_tool();
    let result = tool
        .execute(
            json!({ "pattern": "*.txt", "path": dir.path().to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("a.txt"), "got: {result}");
    assert!(result.contains("b.txt"), "got: {result}");
    assert!(!result.contains("ignored.md"), "got: {result}");
}
