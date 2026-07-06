use async_trait::async_trait;
use futures::StreamExt;
use heddle::agent::architect::{run_architect_pipeline, ArchitectOptions, OnPlanReady};
use heddle::agent::loop_::AgentLoopOptions;
use heddle::agent::types::AgentEvent;
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::{ChatCompletionResponse, Message, ToolDefinition, UserMessage};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

mod common;
use common::mocks::text_response;

// ─── Providers ───────────────────────────────────────────────────────────

struct ScriptProvider {
    responses: Mutex<Vec<ChatCompletionResponse>>,
}
impl ScriptProvider {
    fn new(rs: Vec<ChatCompletionResponse>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(rs),
        })
    }
}
#[async_trait]
impl Provider for ScriptProvider {
    async fn send(
        &self,
        _messages: &[Message],
        _tools: Option<&[ToolDefinition]>,
        _overrides: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        let mut v = self.responses.lock().unwrap();
        if v.is_empty() {
            return Err(anyhow::anyhow!("No more mock responses"));
        }
        Ok(v.remove(0))
    }
    fn stream(
        &self,
        _messages: Vec<Message>,
        _tools: Option<Vec<ToolDefinition>>,
        _overrides: Value,
    ) -> ChunkStream {
        unimplemented!()
    }
    fn with(&self, _o: Value) -> Arc<dyn Provider> {
        unimplemented!()
    }
}

struct ToolCaptureProvider {
    tools_seen: Mutex<Option<Vec<ToolDefinition>>>,
    messages_seen: Mutex<Option<Vec<Message>>>,
    response: ChatCompletionResponse,
}
impl ToolCaptureProvider {
    fn new(response: ChatCompletionResponse) -> Arc<Self> {
        Arc::new(Self {
            tools_seen: Mutex::new(None),
            messages_seen: Mutex::new(None),
            response,
        })
    }
}
#[async_trait]
impl Provider for ToolCaptureProvider {
    async fn send(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        _overrides: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        *self.tools_seen.lock().unwrap() = tools.map(|t| t.to_vec());
        *self.messages_seen.lock().unwrap() = Some(messages.to_vec());
        Ok(self.response.clone())
    }
    fn stream(
        &self,
        _messages: Vec<Message>,
        _tools: Option<Vec<ToolDefinition>>,
        _overrides: Value,
    ) -> ChunkStream {
        unimplemented!()
    }
    fn with(&self, _o: Value) -> Arc<dyn Provider> {
        unimplemented!()
    }
}

struct FailingProvider;
#[async_trait]
impl Provider for FailingProvider {
    async fn send(
        &self,
        _messages: &[Message],
        _tools: Option<&[ToolDefinition]>,
        _overrides: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        Err(anyhow::anyhow!("Architect model failed"))
    }
    fn stream(
        &self,
        _messages: Vec<Message>,
        _tools: Option<Vec<ToolDefinition>>,
        _overrides: Value,
    ) -> ChunkStream {
        unimplemented!()
    }
    fn with(&self, _o: Value) -> Arc<dyn Provider> {
        unimplemented!()
    }
}

// ─── Stub tools ──────────────────────────────────────────────────────────

struct ReadFileTool;
#[async_trait]
impl HeddleTool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a file"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        })
    }
    async fn execute(&self, params: Value, _o: ExecOptions) -> String {
        let p = params.get("path").and_then(Value::as_str).unwrap_or("?");
        format!("contents of {p}")
    }
}

struct WriteFileTool;
#[async_trait]
impl HeddleTool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Write a file"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        })
    }
    async fn execute(&self, params: Value, _o: ExecOptions) -> String {
        let p = params.get("path").and_then(Value::as_str).unwrap_or("?");
        format!("wrote to {p}")
    }
}

struct BashTool;
#[async_trait]
impl HeddleTool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "Execute a bash command"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "command": { "type": "string" } },
            "required": ["command"]
        })
    }
    async fn execute(&self, params: Value, _o: ExecOptions) -> String {
        let c = params.get("command").and_then(Value::as_str).unwrap_or("?");
        format!("ran: {c}")
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────

fn full_registry() -> ToolRegistry {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(ReadFileTool)).unwrap();
    r.register(Arc::new(WriteFileTool)).unwrap();
    r.register(Arc::new(BashTool)).unwrap();
    r
}

fn user(c: &str) -> Vec<Message> {
    vec![Message::User(UserMessage {
        content: c.to_string(),
    })]
}

async fn collect<S: futures::Stream<Item = AgentEvent> + Unpin>(mut s: S) -> Vec<AgentEvent> {
    let mut v = Vec::new();
    while let Some(e) = s.next().await {
        v.push(e);
    }
    v
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn architect_phase_produces_plan_complete_event() {
    let arch = ScriptProvider::new(vec![text_response(
        "Step 1: Read the file\nStep 2: Edit it",
    )]);
    let edit = ScriptProvider::new(vec![text_response("Done editing.")]);
    let mut messages = user("Refactor the code");
    let mut r = ToolRegistry::new();
    r.register(Arc::new(ReadFileTool)).unwrap();
    r.register(Arc::new(WriteFileTool)).unwrap();
    let stream = run_architect_pipeline(
        arch,
        edit,
        r,
        &mut messages,
        AgentLoopOptions::default(),
        ArchitectOptions::default(),
    );
    let events = collect(stream).await;
    let plan = events
        .iter()
        .find(|e| matches!(e, AgentEvent::PlanComplete { .. }))
        .expect("plan_complete");
    if let AgentEvent::PlanComplete { plan } = plan {
        assert!(plan.contains("Step 1"), "got: {plan}");
    }
}

#[tokio::test]
async fn editor_phase_produces_final_response() {
    let arch = ScriptProvider::new(vec![text_response("Plan: do the thing")]);
    let edit = ScriptProvider::new(vec![text_response("I have done the thing.")]);
    let mut messages = user("Do the thing");
    let mut r = ToolRegistry::new();
    r.register(Arc::new(ReadFileTool)).unwrap();
    let stream = run_architect_pipeline(
        arch,
        edit,
        r,
        &mut messages,
        AgentLoopOptions::default(),
        ArchitectOptions::default(),
    );
    let events = collect(stream).await;
    let assistants: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::AssistantMessage { .. }))
        .collect();
    assert!(assistants.len() >= 2);
    if let AgentEvent::AssistantMessage { message, .. } = assistants.last().unwrap() {
        assert_eq!(message.content.as_deref(), Some("I have done the thing."));
    }
}

#[tokio::test]
async fn architect_only_sees_read_only_tools() {
    let arch = ToolCaptureProvider::new(text_response("Plan: read first, then write"));
    let edit = ToolCaptureProvider::new(text_response("Done."));
    let arch_clone = arch.clone();
    let edit_clone = edit.clone();
    let mut messages = user("Fix the bug");
    let stream = run_architect_pipeline(
        arch,
        edit,
        full_registry(),
        &mut messages,
        AgentLoopOptions::default(),
        ArchitectOptions::default(),
    );
    collect(stream).await;

    let arch_tools = arch_clone
        .tools_seen
        .lock()
        .unwrap()
        .clone()
        .expect("arch tools");
    let arch_names: Vec<&str> = arch_tools
        .iter()
        .map(|t| t.function.name.as_str())
        .collect();
    assert!(arch_names.contains(&"read_file"), "got: {arch_names:?}");
    assert!(!arch_names.contains(&"write_file"), "got: {arch_names:?}");
    assert!(!arch_names.contains(&"bash"), "got: {arch_names:?}");

    let edit_tools = edit_clone
        .tools_seen
        .lock()
        .unwrap()
        .clone()
        .expect("edit tools");
    let edit_names: Vec<&str> = edit_tools
        .iter()
        .map(|t| t.function.name.as_str())
        .collect();
    assert!(edit_names.contains(&"read_file"));
    assert!(edit_names.contains(&"write_file"));
    assert!(edit_names.contains(&"bash"));
}

#[tokio::test]
async fn on_plan_ready_callback_can_abort_pipeline() {
    let arch = ScriptProvider::new(vec![text_response("A bad plan")]);
    let edit = ScriptProvider::new(vec![text_response("Should not reach here")]);
    let on_plan: OnPlanReady = Arc::new(|_plan| Box::pin(async move { false }));
    let mut messages = user("Do something");
    let stream = run_architect_pipeline(
        arch,
        edit,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
        ArchitectOptions {
            on_plan_ready: Some(on_plan),
        },
    );
    let events = collect(stream).await;
    let err = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Error { .. }));
    assert!(err.is_some(), "expected error event");
    let assistants = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::AssistantMessage { .. }))
        .count();
    assert_eq!(assistants, 1, "only the architect's assistant message");
}

#[tokio::test]
async fn on_plan_ready_callback_receives_plan_text() {
    let captured = Arc::new(Mutex::new(String::new()));
    let captured_for_cb = captured.clone();
    let on_plan: OnPlanReady = Arc::new(move |plan| {
        let captured = captured_for_cb.clone();
        Box::pin(async move {
            *captured.lock().unwrap() = plan;
            true
        })
    });

    let arch = ScriptProvider::new(vec![text_response("Step 1: Do X\nStep 2: Do Y")]);
    let edit = ScriptProvider::new(vec![text_response("Done")]);
    let mut messages = user("Plan and do");
    let stream = run_architect_pipeline(
        arch,
        edit,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
        ArchitectOptions {
            on_plan_ready: Some(on_plan),
        },
    );
    collect(stream).await;
    assert_eq!(*captured.lock().unwrap(), "Step 1: Do X\nStep 2: Do Y");
}

#[tokio::test]
async fn editor_messages_include_plan_from_architect() {
    let arch = ScriptProvider::new(vec![text_response("The plan is: refactor utils")]);
    let edit = ToolCaptureProvider::new(text_response("Refactored."));
    let edit_clone = edit.clone();
    let mut messages = user("Refactor");
    let stream = run_architect_pipeline(
        arch,
        edit,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
        ArchitectOptions::default(),
    );
    collect(stream).await;

    let editor_messages = edit_clone
        .messages_seen
        .lock()
        .unwrap()
        .clone()
        .expect("editor messages");
    assert!(editor_messages.len() > 1);
    let last_user = editor_messages.iter().rev().find_map(|m| match m {
        Message::User(u) => Some(u.content.clone()),
        _ => None,
    });
    let last = last_user.expect("expected a user message");
    assert!(last.contains("The plan is: refactor utils"), "got: {last}");
}

#[tokio::test]
async fn events_from_both_phases_are_yielded() {
    let arch = ScriptProvider::new(vec![text_response("The plan")]);
    let edit = ScriptProvider::new(vec![text_response("Executed")]);
    let mut messages = user("Go");
    let stream = run_architect_pipeline(
        arch,
        edit,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
        ArchitectOptions::default(),
    );
    let events = collect(stream).await;
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            AgentEvent::Usage { .. } => "usage",
            AgentEvent::AssistantMessage { .. } => "assistant_message",
            AgentEvent::PlanComplete { .. } => "plan_complete",
            AgentEvent::Error { .. } => "error",
            _ => "other",
        })
        .collect();
    assert!(kinds.contains(&"plan_complete"), "kinds: {kinds:?}");
    assert_eq!(
        kinds.iter().filter(|k| **k == "assistant_message").count(),
        2
    );
    assert_eq!(kinds.iter().filter(|k| **k == "usage").count(), 2);
}

#[tokio::test]
async fn handles_architect_provider_error_gracefully() {
    let arch = Arc::new(FailingProvider);
    let edit = ScriptProvider::new(vec![text_response("Should not run")]);
    let mut messages = user("Go");
    let stream = run_architect_pipeline(
        arch,
        edit,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
        ArchitectOptions::default(),
    );
    let events = collect(stream).await;
    let err = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Error { .. }));
    assert!(err.is_some(), "expected error event");
}
