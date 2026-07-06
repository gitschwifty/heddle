use heddle::tools::read::create_read_tool;
use heddle::tools::types::ExecOptions;
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn returns_error_for_nonexistent_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nope.txt");
    let tool = create_read_tool();
    let result = tool
        .execute(
            json!({ "file_path": path.to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("Error"), "got: {result}");
    assert!(result.contains("nope.txt"), "got: {result}");
}

#[tokio::test]
async fn returns_error_for_directory_path() {
    let dir = tempdir().unwrap();
    let tool = create_read_tool();
    let result = tool
        .execute(
            json!({ "file_path": dir.path().to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("Error"), "got: {result}");
}

#[tokio::test]
async fn returns_error_for_empty_path() {
    let tool = create_read_tool();
    let result = tool
        .execute(json!({ "file_path": "" }), ExecOptions::default())
        .await;
    assert!(result.contains("Error"), "got: {result}");
}

#[tokio::test]
async fn reads_existing_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("hello.txt");
    std::fs::write(&path, "hello world").unwrap();
    let tool = create_read_tool();
    let result = tool
        .execute(
            json!({ "file_path": path.to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;
    assert_eq!(result, "hello world");
}
