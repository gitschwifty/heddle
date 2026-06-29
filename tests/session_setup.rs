//! Tests for `create_session`. Mirrors `ts-test/session/setup.test.ts`.

use heddle::session::setup::{create_session, SessionOptions};
use heddle::types::{Message, UserMessage};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::Sandbox;

fn opts() -> SessionOptions {
    SessionOptions::default()
}

fn registered_tool_names(ctx: &heddle::session::setup::SessionContext) -> Vec<String> {
    let mut names: Vec<String> = ctx
        .registry
        .all()
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    names.sort();
    names
}

fn ok_response() -> Value {
    serde_json::json!({
        "id": "chatcmpl-test",
        "choices": [{
            "index": 0,
            "message": { "content": "ok" },
            "finish_reason": "stop"
        }]
    })
}

fn user_msg() -> Vec<Message> {
    vec![Message::User(UserMessage {
        content: "hi".to_string(),
    })]
}

const ALL_TOOLS: &[&str] = &[
    "bash",
    "create_task",
    "edit_file",
    "glob",
    "grep",
    "list_tasks",
    "read_file",
    "save_memory",
    "save_plan",
    "subagent",
    "update_task",
    "web_fetch",
    "write_file",
];

#[tokio::test]
async fn returns_valid_session_context_with_all_fields_populated() {
    let _sb = Sandbox::new("setup-fields");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    let ctx = create_session(opts()).await.expect("create_session");
    assert!(!ctx.messages.is_empty());
    assert!(matches!(ctx.messages[0], heddle::types::Message::System(_)));
    assert!(!ctx.session_id.is_empty());
    assert!(ctx.session_file.exists());
}

#[tokio::test]
async fn default_tools_all_registered() {
    let _sb = Sandbox::new("setup-default-tools");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    std::env::remove_var("HEDDLE_TOOLS");
    let ctx = create_session(opts()).await.unwrap();
    let names = registered_tool_names(&ctx);
    let expected: Vec<String> = ALL_TOOLS.iter().map(|s| s.to_string()).collect();
    assert_eq!(names, expected);
}

#[tokio::test]
async fn session_options_tools_filtering_only_named_tools_registered() {
    let _sb = Sandbox::new("setup-tool-filter");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    std::env::remove_var("HEDDLE_TOOLS");
    let mut o = opts();
    o.tools = Some(vec!["read_file".into(), "glob".into()]);
    let ctx = create_session(o).await.unwrap();
    let names = registered_tool_names(&ctx);
    let mut expected: Vec<String> = vec![
        "create_task",
        "glob",
        "list_tasks",
        "read_file",
        "save_memory",
        "save_plan",
        "subagent",
        "update_task",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    expected.sort();
    assert_eq!(names, expected);
}

#[tokio::test]
async fn session_options_tools_empty_array_keeps_always_on_tools() {
    let _sb = Sandbox::new("setup-tool-empty");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    std::env::remove_var("HEDDLE_TOOLS");
    let mut o = opts();
    o.tools = Some(vec![]);
    let ctx = create_session(o).await.unwrap();
    let names = registered_tool_names(&ctx);
    let mut expected: Vec<String> = vec![
        "create_task",
        "list_tasks",
        "save_memory",
        "save_plan",
        "subagent",
        "update_task",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    expected.sort();
    assert_eq!(names, expected);
}

#[tokio::test]
async fn heddle_tools_env_limits_tools() {
    let _sb = Sandbox::new("setup-heddle-tools");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    std::env::set_var("HEDDLE_TOOLS", "read_file,glob");
    let ctx = create_session(opts()).await.unwrap();
    std::env::remove_var("HEDDLE_TOOLS");
    let names = registered_tool_names(&ctx);
    let mut expected: Vec<String> = vec![
        "create_task",
        "glob",
        "list_tasks",
        "read_file",
        "save_memory",
        "save_plan",
        "subagent",
        "update_task",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    expected.sort();
    assert_eq!(names, expected);
}

#[tokio::test]
async fn session_options_tools_overrides_heddle_tools_env() {
    let _sb = Sandbox::new("setup-override");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    std::env::set_var("HEDDLE_TOOLS", "read_file,glob,bash");
    let mut o = opts();
    o.tools = Some(vec!["write_file".into(), "edit_file".into()]);
    let ctx = create_session(o).await.unwrap();
    std::env::remove_var("HEDDLE_TOOLS");
    let names = registered_tool_names(&ctx);
    let mut expected: Vec<String> = vec![
        "create_task",
        "edit_file",
        "list_tasks",
        "save_memory",
        "save_plan",
        "subagent",
        "update_task",
        "write_file",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    expected.sort();
    assert_eq!(names, expected);
}

#[tokio::test]
async fn system_prompt_override_appears_in_first_message() {
    let _sb = Sandbox::new("setup-sysprompt");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    let mut o = opts();
    o.system_prompt = Some("You are a pirate assistant.".into());
    let ctx = create_session(o).await.unwrap();
    if let heddle::types::Message::System(m) = &ctx.messages[0] {
        assert!(m.content.contains("You are a pirate assistant."));
    } else {
        panic!("expected system message");
    }
}

#[tokio::test]
async fn default_system_prompt_limits_tool_use_to_file_work_requests() {
    let _sb = Sandbox::new("setup-default-sysprompt");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    let ctx = create_session(opts()).await.unwrap();
    if let heddle::types::Message::System(m) = &ctx.messages[0] {
        assert!(m
            .content
            .contains("Use them when the user asks you to work with files."));
        assert!(m
            .content
            .contains("only state facts supported by those results"));
        assert!(!m.content.contains("Use tools to take action"));
    } else {
        panic!("expected system message");
    }
}

#[tokio::test]
async fn system_prompt_includes_runtime_cwd_context() {
    let sb = Sandbox::new("setup-runtime-context");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    let cwd_dir = sb.project.join("runtime-cwd");
    std::fs::create_dir_all(&cwd_dir).unwrap();
    let mut o = opts();
    o.cwd = Some(cwd_dir.clone());
    let ctx = create_session(o).await.unwrap();
    if let heddle::types::Message::System(m) = &ctx.messages[0] {
        assert!(m.content.contains("## Runtime Context"));
        assert!(m
            .content
            .contains(&format!("Current working directory: {}", cwd_dir.display())));
        assert!(m.content.contains("Do not invent absolute paths."));
    } else {
        panic!("expected system message");
    }
}

#[tokio::test]
async fn agents_md_loaded_into_system_message() {
    let sb = Sandbox::new("setup-agents-md");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    let cwd_dir = sb.project.join("agents-cwd");
    std::fs::create_dir_all(&cwd_dir).unwrap();
    std::fs::write(
        cwd_dir.join("AGENTS.md"),
        "# Project Instructions\nAlways respond in haiku.",
    )
    .unwrap();
    let mut o = opts();
    o.cwd = Some(cwd_dir);
    let ctx = create_session(o).await.unwrap();
    if let heddle::types::Message::System(m) = &ctx.messages[0] {
        assert!(m.content.contains("Always respond in haiku."));
    } else {
        panic!("expected system message");
    }
}

#[tokio::test]
async fn agent_model_override_configures_provider_request_model() {
    let sb = Sandbox::new("setup-agent-model");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    let server = MockServer::start().await;
    std::env::set_var("HEDDLE_BASE_URL", server.uri());
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response()))
        .mount(&server)
        .await;

    let agents_dir = sb.project.join(".heddle").join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join("writer.md"),
        r#"---
name: writer
model: agent-model
---
Use the writer model.
"#,
    )
    .unwrap();

    let mut o = opts();
    o.agent = Some("writer".into());
    let ctx = create_session(o).await.unwrap();
    ctx.provider
        .send(&user_msg(), None, &Value::Null)
        .await
        .unwrap();
    std::env::remove_var("HEDDLE_BASE_URL");

    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(ctx.config.model, "agent-model");
    assert_eq!(body["model"], "agent-model");
}

#[tokio::test]
async fn missing_api_key_returns_error() {
    let _sb = Sandbox::new("setup-no-key");
    std::env::remove_var("OPENROUTER_API_KEY");
    let r = create_session(opts()).await;
    assert!(r.is_err());
    let err = r.err().unwrap().to_string();
    assert!(
        err.to_lowercase().contains("api_key") || err.to_lowercase().contains("api key"),
        "got: {err}"
    );
}

#[tokio::test]
async fn session_file_created_with_session_meta_header() {
    let _sb = Sandbox::new("setup-meta");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    let ctx = create_session(opts()).await.unwrap();
    assert!(ctx.session_file.exists());
    let content = std::fs::read_to_string(&ctx.session_file).unwrap();
    let first_line = content.lines().next().unwrap();
    let meta: Value = serde_json::from_str(first_line).unwrap();
    assert_eq!(meta["type"], "session_meta");
    assert_eq!(meta["id"], ctx.session_id);
    assert!(meta["model"].is_string());
    assert!(meta["cwd"].is_string());
    assert!(meta["heddle_version"].is_string());
}

#[tokio::test]
async fn session_id_is_valid_uuid_format() {
    let _sb = Sandbox::new("setup-uuid");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    let ctx = create_session(opts()).await.unwrap();
    let re = regex::Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
        .unwrap();
    assert!(re.is_match(&ctx.session_id), "got: {}", ctx.session_id);
}

#[tokio::test]
async fn cwd_option_chdir_to_provided_directory() {
    let sb = Sandbox::new("setup-cwd");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    let target = sb.project.join("cwd-test");
    std::fs::create_dir_all(&target).unwrap();
    let mut o = opts();
    o.cwd = Some(target.clone());
    create_session(o).await.unwrap();
    assert_eq!(
        std::env::current_dir().unwrap().canonicalize().ok(),
        target.canonicalize().ok()
    );
}

#[tokio::test]
async fn cwd_option_with_nonexistent_dir_returns_error() {
    let sb = Sandbox::new("setup-cwd-bad");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    let mut o = opts();
    o.cwd = Some(sb.project.join("nonexistent-dir"));
    let r = create_session(o).await;
    assert!(r.is_err());
}

#[tokio::test]
async fn weak_model_set_when_heddle_weak_model_env_is_present() {
    let _sb = Sandbox::new("setup-weak");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    std::env::set_var("HEDDLE_WEAK_MODEL", "openrouter/free-weak");
    let ctx = create_session(opts()).await.unwrap();
    std::env::remove_var("HEDDLE_WEAK_MODEL");
    assert!(ctx.weak_provider.is_some());
}

#[tokio::test]
async fn no_weak_model_returns_none() {
    let _sb = Sandbox::new("setup-no-weak");
    std::env::set_var("OPENROUTER_API_KEY", "test-key");
    std::env::remove_var("HEDDLE_WEAK_MODEL");
    let ctx = create_session(opts()).await.unwrap();
    assert!(ctx.weak_provider.is_none());
}
