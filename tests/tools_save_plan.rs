use heddle::plans::storage::load_plan;
use heddle::tools::save_plan::create_save_plan_tool;
use heddle::tools::types::ExecOptions;
use serde_json::json;

mod common;
use common::Sandbox;

#[tokio::test]
async fn has_correct_name_and_description() {
    let _sb = Sandbox::new("saveplan-name");
    let tool = create_save_plan_tool("sess-1".to_string(), Some("test-model".to_string()));
    assert_eq!(tool.name(), "save_plan");
    assert!(!tool.description().is_empty());
}

#[tokio::test]
async fn execute_saves_plan_and_returns_confirmation() {
    let _sb = Sandbox::new("saveplan-exec");
    let tool = create_save_plan_tool("sess-tool".to_string(), Some("tool-model".to_string()));
    let result = tool
        .execute(
            json!({ "name": "tool-plan", "content": "# Tool Plan\n\nThis is a plan from the tool." }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("tool-plan"), "got: {result}");

    let plan = load_plan("tool-plan", None).expect("plan should exist");
    assert!(plan.content.contains("This is a plan from the tool."));
    assert_eq!(
        plan.meta.get("model").map(String::as_str),
        Some("tool-model")
    );
    assert_eq!(
        plan.meta.get("session_id").map(String::as_str),
        Some("sess-tool")
    );
}

#[tokio::test]
async fn execute_without_model_still_saves() {
    let _sb = Sandbox::new("saveplan-no-model");
    let tool = create_save_plan_tool("sess-nomodel".to_string(), None);
    let result = tool
        .execute(
            json!({ "name": "no-model-plan", "content": "Plan without model." }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("no-model-plan"), "got: {result}");

    let plan = load_plan("no-model-plan", None).expect("plan should exist");
    assert!(plan.meta.get("model").is_none());
}

#[tokio::test]
async fn schema_requires_name_and_content() {
    let _sb = Sandbox::new("saveplan-schema");
    let tool = create_save_plan_tool("sess-1".to_string(), None);
    let schema = tool.parameters();
    let props = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .unwrap();
    assert!(props.contains_key("name"));
    assert!(props.contains_key("content"));
    let required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(required.contains(&"name"));
    assert!(required.contains(&"content"));
}
