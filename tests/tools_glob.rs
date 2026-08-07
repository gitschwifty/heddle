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

#[tokio::test]
async fn skips_generated_trees_during_broad_discovery_but_allows_narrow_requests() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("src.txt"), "x").unwrap();
    std::fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    std::fs::write(dir.path().join("target/debug/generated.txt"), "x").unwrap();
    let tool = create_glob_tool();

    let broad = tool
        .execute(
            json!({ "pattern": "**/*", "path": dir.path().to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(broad.contains("src.txt"), "got: {broad}");
    assert!(!broad.contains("generated.txt"), "got: {broad}");

    let narrow = tool
        .execute(
            json!({ "pattern": "debug/*.txt", "path": dir.path().join("target").to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(narrow.contains("generated.txt"), "got: {narrow}");
}
