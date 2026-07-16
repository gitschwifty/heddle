use heddle::agents::types::AgentDefinition;
use heddle::commands::builtins::create_builtin_commands;
use heddle::commands::types::{CommandContext, SlashCommand};
use heddle::config::loader::HeddleConfig;
use heddle::cost::pricing::ModelPricing;
use heddle::cost::tracker::CostTracker;
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::{
    ChatCompletionResponse, Message, SystemMessage, ToolDefinition, Usage, UserMessage,
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::Sandbox;

// ─── Minimal stubs ────────────────────────────────────────────────────────
struct NoopProvider;
#[async_trait::async_trait]
impl Provider for NoopProvider {
    async fn send(
        &self,
        _m: &[Message],
        _t: Option<&[ToolDefinition]>,
        _o: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        unimplemented!()
    }
    fn stream(&self, _m: Vec<Message>, _t: Option<Vec<ToolDefinition>>, _o: Value) -> ChunkStream {
        unimplemented!()
    }
    fn with(&self, _o: Value) -> Arc<dyn Provider> {
        Arc::new(NoopProvider)
    }
}

struct DummyTool {
    name: String,
    desc: String,
}
#[async_trait::async_trait]
impl HeddleTool for DummyTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.desc
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _p: Value, _o: ExecOptions) -> String {
        "ok".to_string()
    }
}

fn find<'a>(cmds: &'a [SlashCommand], name: &str) -> &'a SlashCommand {
    cmds.iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("command {name} not found"))
}

struct CtxState {
    config: HeddleConfig,
    messages: Vec<Message>,
    registry: ToolRegistry,
    cost_tracker: Arc<Mutex<CostTracker>>,
    model_pricing: ModelPricing,
    session_file: PathBuf,
    agent_definitions: HashMap<String, AgentDefinition>,
}

impl CtxState {
    fn new(session_file: PathBuf) -> Self {
        Self {
            config: HeddleConfig::default(),
            messages: vec![Message::System(SystemMessage {
                content: "system".into(),
            })],
            registry: ToolRegistry::new(),
            cost_tracker: Arc::new(Mutex::new(CostTracker::default())),
            model_pricing: ModelPricing::new("test-key", None),
            session_file,
            agent_definitions: HashMap::new(),
        }
    }
    fn with_model_pricing(mut self, model_pricing: ModelPricing) -> Self {
        self.model_pricing = model_pricing;
        self
    }
    fn ctx(&mut self) -> CommandContext<'_> {
        CommandContext {
            config: &mut self.config,
            messages: &mut self.messages,
            registry: &self.registry,
            cost_tracker: self.cost_tracker.clone(),
            model_pricing: self.model_pricing.clone(),
            session_file: self.session_file.clone(),
            session_id: "test-session".to_string(),
            provider: Arc::new(NoopProvider),
            weak_provider: None,
            editor_provider: None,
            discovery: None,
            agent_definitions: &self.agent_definitions,
            paste_cache: None,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────

fn mock_models_response() -> serde_json::Value {
    json!({
        "data": [
            {
                "id": "anthropic/claude-3-sonnet",
                "name": "Claude 3 Sonnet",
                "pricing": { "prompt": "0.000003", "completion": "0.000015" },
                "context_length": 200000,
                "top_provider": { "max_completion_tokens": 4096 },
                "architecture": { "modality": "text->text" },
                "supported_parameters": ["temperature", "top_p"]
            },
            {
                "id": "openrouter/free",
                "name": "OpenRouter Free",
                "pricing": { "prompt": "0", "completion": "0" },
                "context_length": 8192,
                "top_provider": { "max_completion_tokens": 1024 },
                "architecture": { "modality": "text->text" },
                "supported_parameters": []
            }
        ]
    })
}

async fn mount_models(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_models_response()))
        .mount(server)
        .await;
}

#[tokio::test]
async fn help_command_returns_none_without_panic() {
    let sb = Sandbox::new("blt-help");
    let mut st = CtxState::new(sb.project.join("s.jsonl"));
    let cmds = create_builtin_commands();
    let help = find(&cmds, "help");
    let r = (help.execute)("", &mut st.ctx()).await;
    assert!(r.is_none());
}

#[tokio::test]
async fn clear_truncates_messages_to_one() {
    let sb = Sandbox::new("blt-clear");
    let mut st = CtxState::new(sb.project.join("s.jsonl"));
    st.messages.push(Message::User(UserMessage {
        content: "hello".into(),
    }));
    st.messages.push(Message::User(UserMessage {
        content: "world".into(),
    }));
    let cmds = create_builtin_commands();
    let clear = find(&cmds, "clear");
    let r = (clear.execute)("", &mut st.ctx()).await;
    assert!(r.is_none());
    assert_eq!(st.messages.len(), 1);
    assert!(matches!(st.messages[0], Message::System(_)));
}

#[tokio::test]
async fn cost_runs_with_populated_tracker() {
    let sb = Sandbox::new("blt-cost-pop");
    let mut st = CtxState::new(sb.project.join("s.jsonl"));
    st.cost_tracker.lock().add_usage(&Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        cost: Some(0.0025),
        prompt_tokens_details: None,
        completion_tokens_details: None,
    });
    let cmds = create_builtin_commands();
    let cost = find(&cmds, "cost");
    let r = (cost.execute)("", &mut st.ctx()).await;
    assert!(r.is_none());
    assert_eq!(st.cost_tracker.lock().total_input_tokens(), 100);
    assert_eq!(st.cost_tracker.lock().total_output_tokens(), 50);
    assert_eq!(st.cost_tracker.lock().total_cost(), Some(0.0025));
}

#[tokio::test]
async fn cost_runs_with_empty_tracker() {
    let sb = Sandbox::new("blt-cost-empty");
    let mut st = CtxState::new(sb.project.join("s.jsonl"));
    let cmds = create_builtin_commands();
    let cost = find(&cmds, "cost");
    let r = (cost.execute)("", &mut st.ctx()).await;
    assert!(r.is_none());
    assert!(st.cost_tracker.lock().total_cost().is_none());
}

#[tokio::test]
async fn status_runs_without_panic() {
    let sb = Sandbox::new("blt-status");
    let mut st = CtxState::new(sb.project.join("my-session.jsonl"));
    st.config.model = "gpt-4".into();
    st.messages.push(Message::User(UserMessage {
        content: "hi".into(),
    }));
    let cmds = create_builtin_commands();
    let status = find(&cmds, "status");
    let r = (status.execute)("", &mut st.ctx()).await;
    assert!(r.is_none());
}

#[tokio::test]
async fn context_command_runs_without_panic() {
    let sb = Sandbox::new("blt-ctx");
    let mut st = CtxState::new(sb.project.join("s.jsonl"));
    st.messages = vec![
        Message::System(SystemMessage {
            content: "a".repeat(400),
        }),
        Message::User(UserMessage {
            content: "b".repeat(400),
        }),
    ];
    let cmds = create_builtin_commands();
    let ctx_cmd = find(&cmds, "context");
    let r = (ctx_cmd.execute)("", &mut st.ctx()).await;
    assert!(r.is_none());
}

#[tokio::test]
async fn model_with_args_returns_new_model_name() {
    let sb = Sandbox::new("blt-model-set");
    let mut st = CtxState::new(sb.project.join("s.jsonl"));
    let cmds = create_builtin_commands();
    let model = find(&cmds, "model");
    let r = (model.execute)("openrouter/free", &mut st.ctx()).await;
    assert_eq!(r.as_deref(), Some("openrouter/free"));
}

#[tokio::test]
async fn model_with_known_id_loads_registry_before_switching() {
    let sb = Sandbox::new("blt-model-known");
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let mut st = CtxState::new(sb.project.join("s.jsonl")).with_model_pricing(pricing.clone());
    let cmds = create_builtin_commands();
    let model = find(&cmds, "model");

    let r = (model.execute)("anthropic/claude-3-sonnet", &mut st.ctx()).await;

    assert_eq!(r.as_deref(), Some("anthropic/claude-3-sonnet"));
    assert!(pricing.is_loaded().await);
}

#[tokio::test]
async fn model_with_unknown_id_warns_but_keeps_fallback_switch_behavior() {
    let sb = Sandbox::new("blt-model-unknown");
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let mut st = CtxState::new(sb.project.join("s.jsonl")).with_model_pricing(pricing.clone());
    let cmds = create_builtin_commands();
    let model = find(&cmds, "model");

    let r = (model.execute)("provider/native-alias", &mut st.ctx()).await;

    assert_eq!(r.as_deref(), Some("provider/native-alias"));
    assert!(pricing.is_loaded().await);
}

#[tokio::test]
async fn model_registry_fetch_failure_is_non_fatal_for_switching() {
    let sb = Sandbox::new("blt-model-fetch-failure");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let mut st = CtxState::new(sb.project.join("s.jsonl")).with_model_pricing(pricing.clone());
    let cmds = create_builtin_commands();
    let model = find(&cmds, "model");

    let r = (model.execute)("openrouter/free", &mut st.ctx()).await;

    assert_eq!(r.as_deref(), Some("openrouter/free"));
    assert!(!pricing.is_loaded().await);
}

#[tokio::test]
async fn model_with_no_args_returns_none() {
    let sb = Sandbox::new("blt-model-noop");
    let mut st = CtxState::new(sb.project.join("s.jsonl"));
    st.config.model = "current-model".into();
    let cmds = create_builtin_commands();
    let model = find(&cmds, "model");
    let r = (model.execute)("", &mut st.ctx()).await;
    assert!(r.is_none());
}

#[tokio::test]
async fn model_with_no_args_loads_current_model_details_when_available() {
    let sb = Sandbox::new("blt-model-current-details");
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let mut st = CtxState::new(sb.project.join("s.jsonl")).with_model_pricing(pricing.clone());
    st.config.model = "anthropic/claude-3-sonnet".into();
    let cmds = create_builtin_commands();
    let model = find(&cmds, "model");

    let r = (model.execute)("", &mut st.ctx()).await;

    assert!(r.is_none());
    assert!(pricing.is_loaded().await);
}

#[tokio::test]
async fn models_command_lists_matching_registry_entries() {
    let sb = Sandbox::new("blt-models-list");
    let server = MockServer::start().await;
    mount_models(&server).await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let mut st = CtxState::new(sb.project.join("s.jsonl")).with_model_pricing(pricing.clone());
    let cmds = create_builtin_commands();
    let models = find(&cmds, "models");

    let r = (models.execute)("sonnet", &mut st.ctx()).await;

    assert!(r.is_none());
    assert!(pricing.is_loaded().await);
}

#[tokio::test]
async fn models_command_registry_failure_is_non_fatal() {
    let sb = Sandbox::new("blt-models-failure");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let pricing = ModelPricing::new("test-key", Some(&server.uri()));
    let mut st = CtxState::new(sb.project.join("s.jsonl")).with_model_pricing(pricing.clone());
    let cmds = create_builtin_commands();
    let models = find(&cmds, "models");

    let r = (models.execute)("sonnet", &mut st.ctx()).await;

    assert!(r.is_none());
    assert!(!pricing.is_loaded().await);
}

#[tokio::test]
async fn tools_command_runs_with_registered_tools() {
    let sb = Sandbox::new("blt-tools");
    let mut st = CtxState::new(sb.project.join("s.jsonl"));
    st.registry
        .register(Arc::new(DummyTool {
            name: "read_file".into(),
            desc: "Read a file".into(),
        }))
        .unwrap();
    st.registry
        .register(Arc::new(DummyTool {
            name: "write_file".into(),
            desc: "Write a file".into(),
        }))
        .unwrap();
    let cmds = create_builtin_commands();
    let tools = find(&cmds, "tools");
    let r = (tools.execute)("", &mut st.ctx()).await;
    assert!(r.is_none());
}

#[tokio::test]
async fn builtins_contains_expected_command_names() {
    let cmds = create_builtin_commands();
    let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
    for required in &[
        "help", "clear", "exit", "quit", "cost", "status", "context", "models", "model", "tools",
        "history", "compact", "sessions", "fork", "tasks", "agents", "plan", "stats", "paste",
        "agent", "restore", "name", "rewind",
    ] {
        assert!(names.contains(required), "missing builtin: {required}");
    }
}

#[tokio::test]
async fn rewind_list_empty_when_no_checkpoints() {
    let sb = Sandbox::new("blt-rewind-empty");
    let session = sb.project.join("s.jsonl");
    // Seed a bare session file so load_checkpoints has something to read.
    heddle::session::jsonl::write_session_meta(
        &session,
        &heddle::session::jsonl::SessionMeta {
            kind: "session_meta".into(),
            id: "test".into(),
            cwd: sb.project.to_string_lossy().to_string(),
            model: "m".into(),
            created: "2026-05-22T00:00:00Z".into(),
            heddle_version: "0".into(),
            name: None,
            forked_from: None,
            extra: Default::default(),
        },
    )
    .unwrap();
    let mut st = CtxState::new(session);
    let cmds = create_builtin_commands();
    let rewind = find(&cmds, "rewind");
    let r = (rewind.execute)("", &mut st.ctx()).await;
    assert!(r.is_none());
}

#[tokio::test]
async fn rewind_to_invalid_index_does_not_panic() {
    let sb = Sandbox::new("blt-rewind-bad-idx");
    let session = sb.project.join("s.jsonl");
    heddle::session::jsonl::write_session_meta(
        &session,
        &heddle::session::jsonl::SessionMeta {
            kind: "session_meta".into(),
            id: "test".into(),
            cwd: sb.project.to_string_lossy().to_string(),
            model: "m".into(),
            created: "2026-05-22T00:00:00Z".into(),
            heddle_version: "0".into(),
            name: None,
            forked_from: None,
            extra: Default::default(),
        },
    )
    .unwrap();
    let mut st = CtxState::new(session);
    let cmds = create_builtin_commands();
    let rewind = find(&cmds, "rewind");
    // Index out of range, malformed scope, missing index — all should be no-ops.
    assert!((rewind.execute)("to 99", &mut st.ctx()).await.is_none());
    assert!((rewind.execute)("to notanumber", &mut st.ctx())
        .await
        .is_none());
    assert!((rewind.execute)("to", &mut st.ctx()).await.is_none());
    assert!((rewind.execute)("to 1 garbage", &mut st.ctx())
        .await
        .is_none());
    assert!((rewind.execute)("bogus", &mut st.ctx()).await.is_none());
}
