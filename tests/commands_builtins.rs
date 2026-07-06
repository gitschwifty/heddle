use heddle::agents::types::AgentDefinition;
use heddle::commands::builtins::create_builtin_commands;
use heddle::commands::types::{CommandContext, SlashCommand};
use heddle::config::loader::HeddleConfig;
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
            session_file,
            agent_definitions: HashMap::new(),
        }
    }
    fn ctx(&mut self) -> CommandContext<'_> {
        CommandContext {
            config: &mut self.config,
            messages: &mut self.messages,
            registry: &self.registry,
            cost_tracker: self.cost_tracker.clone(),
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
        "help", "clear", "exit", "quit", "cost", "status", "context", "model", "tools", "history",
        "compact", "sessions", "fork", "tasks", "agents", "plan", "stats", "paste", "agent",
        "restore", "name", "rewind",
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
