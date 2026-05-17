use async_trait::async_trait;
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use serde_json::{json, Value};
use std::sync::Arc;

struct EchoTool {
    name_: String,
}

#[async_trait]
impl HeddleTool for EchoTool {
    fn name(&self) -> &str {
        &self.name_
    }
    fn description(&self) -> &str {
        "Test tool"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "input": { "type": "string" } },
            "required": ["input"]
        })
    }
    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        format!("executed {} with {}", self.name_, params)
    }
}

fn echo(name: &str) -> Arc<dyn HeddleTool> {
    Arc::new(EchoTool {
        name_: name.to_string(),
    })
}

#[tokio::test]
async fn register_and_get_by_name() {
    let mut r = ToolRegistry::new();
    r.register(echo("echo")).unwrap();
    assert!(r.get("echo").is_some());
}

#[tokio::test]
async fn get_returns_none_for_unknown_tool() {
    let r = ToolRegistry::new();
    assert!(r.get("nonexistent").is_none());
}

#[tokio::test]
async fn lists_all_registered_tools() {
    let mut r = ToolRegistry::new();
    r.register(echo("alpha")).unwrap();
    r.register(echo("beta")).unwrap();
    let all = r.all();
    assert_eq!(all.len(), 2);
    let mut names: Vec<&str> = all.iter().map(|t| t.name()).collect();
    names.sort();
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[tokio::test]
async fn generates_openai_format_tool_definitions() {
    let mut r = ToolRegistry::new();
    r.register(echo("read_file")).unwrap();
    let defs = r.definitions();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].function.name, "read_file");
    assert!(!defs[0].function.description.is_empty());
}

#[tokio::test]
async fn execute_by_name_with_json_args() {
    let mut r = ToolRegistry::new();
    r.register(echo("greet")).unwrap();
    let result = r
        .execute("greet", r#"{"input":"world"}"#, ExecOptions::default())
        .await;
    assert!(result.contains("executed greet"), "got: {result}");
    assert!(result.contains("world"), "got: {result}");
}

#[tokio::test]
async fn execute_returns_error_for_unknown_tool() {
    let r = ToolRegistry::new();
    let result = r.execute("missing", "{}", ExecOptions::default()).await;
    assert!(result.contains("Unknown tool: missing"), "got: {result}");
}

#[tokio::test]
async fn unknown_tool_with_close_match_suggests_correct_one() {
    let mut r = ToolRegistry::new();
    r.register(echo("read_file")).unwrap();
    r.register(echo("write_file")).unwrap();
    let result = r.execute("reed_file", "{}", ExecOptions::default()).await;
    assert!(
        result.contains(r#"Did you mean "read_file""#),
        "got: {result}"
    );
    assert!(result.contains("Available tools"), "got: {result}");
}

#[tokio::test]
async fn unknown_tool_far_match_just_lists_tools() {
    let mut r = ToolRegistry::new();
    r.register(echo("read_file")).unwrap();
    r.register(echo("write_file")).unwrap();
    let result = r
        .execute("completely_unknown_xyz", "{}", ExecOptions::default())
        .await;
    assert!(!result.contains("Did you mean"), "got: {result}");
    assert!(result.contains("Available tools"), "got: {result}");
    assert!(result.contains("read_file"), "got: {result}");
    assert!(result.contains("write_file"), "got: {result}");
}

#[tokio::test]
async fn execute_returns_error_for_invalid_json() {
    let mut r = ToolRegistry::new();
    r.register(echo("test_tool")).unwrap();
    let result = r
        .execute("test_tool", "not-json", ExecOptions::default())
        .await;
    assert!(result.contains("Invalid JSON"), "got: {result}");
}

#[tokio::test]
async fn duplicate_registration_returns_error() {
    let mut r = ToolRegistry::new();
    r.register(echo("dupe")).unwrap();
    let err = r.register(echo("dupe")).err().expect("expected error");
    assert!(err.to_string().contains("already registered"), "got: {err}");
}
