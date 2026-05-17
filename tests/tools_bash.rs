use heddle::tools::bash::create_bash_tool;
use heddle::tools::types::ExecOptions;
use serde_json::json;

#[tokio::test]
async fn non_zero_exit_code_reported() {
    let tool = create_bash_tool();
    let result = tool
        .execute(json!({ "command": "exit 42" }), ExecOptions::default())
        .await;
    assert!(result.contains("Exit code: 42"), "got: {result}");
}

#[tokio::test]
async fn captures_stderr_from_failing_command() {
    let tool = create_bash_tool();
    let result = tool
        .execute(
            json!({ "command": "echo 'bad stuff' >&2 && exit 1" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("STDERR"), "got: {result}");
    assert!(result.contains("bad stuff"), "got: {result}");
    assert!(result.contains("Exit code: 1"), "got: {result}");
}

#[tokio::test]
async fn returns_stderr_even_on_success() {
    let tool = create_bash_tool();
    let result = tool
        .execute(
            json!({ "command": "echo 'warning' >&2" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("STDERR"), "got: {result}");
    assert!(result.contains("warning"), "got: {result}");
}

#[tokio::test]
async fn handles_command_not_found() {
    let tool = create_bash_tool();
    let result = tool
        .execute(
            json!({ "command": "nonexistent_command_xyz_12345" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("not found"), "got: {result}");
}

#[tokio::test]
async fn returns_no_output_marker_for_empty_output() {
    let tool = create_bash_tool();
    let result = tool
        .execute(json!({ "command": "true" }), ExecOptions::default())
        .await;
    assert_eq!(result, "(no output)");
}
