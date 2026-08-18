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

#[tokio::test]
async fn reads_an_inclusive_line_range_with_metadata() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("lines.txt");
    std::fs::write(&path, "one\ntwo\nthree\nfour\n").unwrap();
    let tool = create_read_tool();

    let result = tool
        .execute(
            json!({ "file_path": path.to_string_lossy(), "start_line": 2, "end_line": 3 }),
            ExecOptions::default(),
        )
        .await;

    assert!(result.contains("total_lines=4"), "got: {result}");
    assert!(result.contains("total_bytes=19"), "got: {result}");
    assert!(result.contains("requested_lines=2-3"), "got: {result}");
    assert!(result.contains("returned_lines=2-3"), "got: {result}");
    assert!(result.contains("truncated=false"), "got: {result}");
    assert!(result.ends_with("two\nthree"), "got: {result}");
}

#[tokio::test]
async fn default_large_read_is_capped_with_a_continuation_hint() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("large.txt");
    let content = (1..=205)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, &content).unwrap();
    let tool = create_read_tool();

    let result = tool
        .execute(
            json!({ "file_path": path.to_string_lossy() }),
            ExecOptions::default(),
        )
        .await;

    assert!(result.contains("total_lines=205"), "got: {result}");
    assert!(result.contains("requested_lines=1-205"), "got: {result}");
    assert!(result.contains("returned_lines=1-200"), "got: {result}");
    assert!(result.contains("truncated=true"), "got: {result}");
    assert!(result.contains("next_start_line=201"), "got: {result}");
    assert!(result.contains("line 200"), "got: {result}");
    assert!(!result.contains("line 201"), "got: {result}");
}

#[tokio::test]
async fn full_file_escapes_the_default_line_cap() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("large.txt");
    let content = (1..=205)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, &content).unwrap();
    let tool = create_read_tool();

    let result = tool
        .execute(
            json!({ "file_path": path.to_string_lossy(), "full_file": true }),
            ExecOptions::default(),
        )
        .await;

    assert_eq!(result, content);
}

#[tokio::test]
async fn rejects_invalid_ranges_with_actionable_errors() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("lines.txt");
    std::fs::write(&path, "one\ntwo\nthree").unwrap();
    let tool = create_read_tool();

    for params in [
        json!({ "file_path": path.to_string_lossy(), "start_line": 0 }),
        json!({ "file_path": path.to_string_lossy(), "start_line": 3, "end_line": 2 }),
        json!({ "file_path": path.to_string_lossy(), "start_line": 4 }),
        json!({ "file_path": path.to_string_lossy(), "end_line": 4 }),
        json!({ "file_path": path.to_string_lossy(), "full_file": true, "start_line": 1 }),
    ] {
        let result = tool.execute(params, ExecOptions::default()).await;
        assert!(result.starts_with("Error:"), "got: {result}");
        assert!(
            result.contains("line") || result.contains("full_file"),
            "got: {result}"
        );
    }
}

#[tokio::test]
async fn range_reads_preserve_utf8_line_boundaries() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("unicode.txt");
    std::fs::write(&path, "first\n🧵 second\nthird").unwrap();
    let tool = create_read_tool();

    let result = tool
        .execute(
            json!({ "file_path": path.to_string_lossy(), "start_line": 2, "end_line": 2 }),
            ExecOptions::default(),
        )
        .await;

    assert!(result.ends_with("🧵 second"), "got: {result}");
    assert!(result.contains("total_bytes=23"), "got: {result}");
}
