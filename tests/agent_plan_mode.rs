use async_trait::async_trait;
use futures::StreamExt;
use heddle::agent::loop_::{run_agent_loop, AgentLoopOptions};
use heddle::agent::types::AgentEvent;
use heddle::config::loader::ApprovalMode;
use heddle::permissions::checker::{read_only_tool_filter, PermissionChecker};
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::{ChatCompletionResponse, Message, ToolDefinition, UserMessage};
use parking_lot::Mutex as PlMutex;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

mod common;
use common::mocks::{text_response, tool_call_response};

// ─── Tools ───────────────────────────────────────────────────────────────

struct ReadTool;
#[async_trait]
impl HeddleTool for ReadTool {
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
struct WriteTool;
#[async_trait]
impl HeddleTool for WriteTool {
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
        format!("wrote {p}")
    }
}
struct BashTool;
#[async_trait]
impl HeddleTool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "Execute bash"
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
        format!("executed: {c}")
    }
}

// ─── Provider helpers ────────────────────────────────────────────────────

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

/// Provider that captures the tool list passed to send().
struct ToolCaptureProvider {
    captured: Mutex<Option<Vec<ToolDefinition>>>,
    response: ChatCompletionResponse,
}
impl ToolCaptureProvider {
    fn new(response: ChatCompletionResponse) -> Arc<Self> {
        Arc::new(Self {
            captured: Mutex::new(None),
            response,
        })
    }
}
#[async_trait]
impl Provider for ToolCaptureProvider {
    async fn send(
        &self,
        _messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        _overrides: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        *self.captured.lock().unwrap() = tools.map(|t| t.to_vec());
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

// ─── Helpers ─────────────────────────────────────────────────────────────

fn full_registry() -> ToolRegistry {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(ReadTool)).unwrap();
    r.register(Arc::new(WriteTool)).unwrap();
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
async fn tool_filter_strips_non_read_tools() {
    let p = ToolCaptureProvider::new(text_response("Here is my plan..."));
    let p2 = p.clone();
    let mut messages = user("plan something");
    let stream = run_agent_loop(
        p,
        full_registry(),
        &mut messages,
        AgentLoopOptions {
            tool_filter: Some(Arc::new(|defs| read_only_tool_filter(defs))),
            ..Default::default()
        },
    );
    collect(stream).await;
    let captured = p2.captured.lock().unwrap().clone().expect("tools captured");
    let names: Vec<&str> = captured.iter().map(|t| t.function.name.as_str()).collect();
    assert!(names.contains(&"read_file"), "got: {names:?}");
    assert!(!names.contains(&"write_file"), "got: {names:?}");
    assert!(!names.contains(&"bash"), "got: {names:?}");
}

#[tokio::test]
async fn write_tool_denied_even_when_llm_hallucinates() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("write_file", json!({ "path": "foo.txt", "content": "bar" }))]),
        text_response("Plan complete"),
    ]);
    let mut messages = user("plan");
    let stream = run_agent_loop(
        p,
        full_registry(),
        &mut messages,
        AgentLoopOptions {
            permission_checker: Some(Arc::new(PlMutex::new(PermissionChecker::new(
                ApprovalMode::Plan,
                None,
                None,
            )))),
            tool_filter: Some(Arc::new(|defs| read_only_tool_filter(defs))),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let denied = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::PermissionDenied { .. }))
        .count();
    assert_eq!(denied, 1);
    let tool_ends = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolEnd { .. }))
        .count();
    assert_eq!(tool_ends, 0);
}

#[tokio::test]
async fn plan_complete_event_contains_final_assistant_text() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("read_file", json!({ "path": "src/index.ts" }))]),
        text_response("## Plan\n1. Do X\n2. Do Y\n3. Do Z"),
    ]);
    let mut messages = user("plan");
    let mut r = ToolRegistry::new();
    r.register(Arc::new(ReadTool)).unwrap();
    let stream = run_agent_loop(
        p,
        r,
        &mut messages,
        AgentLoopOptions {
            plan_mode: true,
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let plan_complete: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::PlanComplete { .. }))
        .collect();
    assert_eq!(plan_complete.len(), 1);
    if let AgentEvent::PlanComplete { plan } = plan_complete[0] {
        assert_eq!(plan, "## Plan\n1. Do X\n2. Do Y\n3. Do Z");
    }
}

#[tokio::test]
async fn plan_mode_with_filter_and_checker_integration() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("read_file", json!({ "path": "src/main.ts" }))]),
        text_response("Here is my plan: refactor main.ts"),
    ]);
    let mut messages = user("plan the refactor");
    let stream = run_agent_loop(
        p,
        full_registry(),
        &mut messages,
        AgentLoopOptions {
            permission_checker: Some(Arc::new(PlMutex::new(PermissionChecker::new(
                ApprovalMode::Plan,
                None,
                None,
            )))),
            tool_filter: Some(Arc::new(|defs| read_only_tool_filter(defs))),
            plan_mode: true,
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let tool_ends: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolEnd { .. }))
        .collect();
    assert_eq!(tool_ends.len(), 1);
    if let AgentEvent::ToolEnd { name, .. } = tool_ends[0] {
        assert_eq!(name, "read_file");
    }
    let plan_complete: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::PlanComplete { .. }))
        .collect();
    assert_eq!(plan_complete.len(), 1);
    if let AgentEvent::PlanComplete { plan } = plan_complete[0] {
        assert!(plan.contains("refactor main.ts"), "got: {plan}");
    }
}

#[tokio::test]
async fn plan_complete_not_emitted_when_plan_mode_false() {
    let p = ScriptProvider::new(vec![text_response("Just a response")]);
    let mut messages = user("hello");
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;
    let plan_complete = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::PlanComplete { .. }))
        .count();
    assert_eq!(plan_complete, 0);
}

#[tokio::test]
async fn plan_complete_uses_last_assistant_message_without_tools() {
    let p = ScriptProvider::new(vec![text_response("My plan is: do nothing")]);
    let mut messages = user("plan");
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions {
            plan_mode: true,
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let plan_complete: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::PlanComplete { .. }))
        .collect();
    assert_eq!(plan_complete.len(), 1);
    if let AgentEvent::PlanComplete { plan } = plan_complete[0] {
        assert_eq!(plan, "My plan is: do nothing");
    }
}
