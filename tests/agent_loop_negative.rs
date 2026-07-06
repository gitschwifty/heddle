use async_trait::async_trait;
use futures::StreamExt;
use heddle::agent::loop_::{run_agent_loop, AgentLoopOptions};
use heddle::agent::types::AgentEvent;
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::{ChatCompletionResponse, Message, ToolDefinition, UserMessage};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

mod common;
use common::mocks::{text_response, tool_call_response};

struct FailProvider;

#[async_trait]
impl Provider for FailProvider {
    async fn send(
        &self,
        _messages: &[Message],
        _tools: Option<&[ToolDefinition]>,
        _overrides: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        Err(anyhow::anyhow!("API is down"))
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

struct FailingTool;
#[async_trait]
impl HeddleTool for FailingTool {
    fn name(&self) -> &str {
        "fail"
    }
    fn description(&self) -> &str {
        "Always errors"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "input": { "type": "string" } },
            "required": ["input"]
        })
    }
    async fn execute(&self, _params: Value, _o: ExecOptions) -> String {
        "Error: Tool exploded".to_string()
    }
}

struct EchoTool;
#[async_trait]
impl HeddleTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "Echo"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "text": { "type": "string" } },
            "required": ["text"]
        })
    }
    async fn execute(&self, params: Value, _o: ExecOptions) -> String {
        params
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }
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

#[tokio::test]
async fn yields_error_event_when_provider_fails() {
    let mut messages = user("Hi");
    let stream = run_agent_loop(
        Arc::new(FailProvider),
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;
    let err = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Error { .. }))
        .expect("expected an error event");
    if let AgentEvent::Error { message } = err {
        assert!(message.contains("API is down"), "got: {message}");
    }
}

#[tokio::test]
async fn yields_error_when_response_has_empty_choices() {
    let empty = ChatCompletionResponse {
        id: "test".to_string(),
        choices: vec![],
        usage: Some(heddle::types::Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            ..Default::default()
        }),
    };
    let mut messages = user("Hi");
    let stream = run_agent_loop(
        ScriptProvider::new(vec![empty]),
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;
    let err = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Error { .. }))
        .expect("expected an error event");
    if let AgentEvent::Error { message } = err {
        assert!(message.contains("No choice"), "got: {message}");
    }
}

#[tokio::test]
async fn unknown_tool_returns_error_to_model_and_loop_continues() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("nonexistent_tool", json!({ "x": 1 }))]),
        text_response("Done"),
    ]);
    let mut messages = user("call a tool");
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;
    let tool_end = events
        .iter()
        .find(|e| matches!(e, AgentEvent::ToolEnd { .. }))
        .expect("expected tool_end");
    if let AgentEvent::ToolEnd { result, .. } = tool_end {
        assert!(
            result.contains("Error: Unknown tool: nonexistent_tool"),
            "got: {result}"
        );
    }
    let assistant_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::AssistantMessage { .. }))
        .count();
    assert!(assistant_count >= 1);
}

#[tokio::test]
async fn tool_returning_error_string_doesnt_crash_loop() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("fail", json!({ "input": "test" }))]),
        text_response("Handled the error"),
    ]);
    let mut r = ToolRegistry::new();
    r.register(Arc::new(FailingTool)).unwrap();
    let mut messages = user("try it");
    let stream = run_agent_loop(p, r, &mut messages, AgentLoopOptions::default());
    let events = collect(stream).await;
    let tool_ends: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolEnd { .. }))
        .collect();
    assert_eq!(tool_ends.len(), 1);
    if let AgentEvent::ToolEnd { result, .. } = tool_ends[0] {
        assert!(result.contains("Tool exploded"), "got: {result}");
    }
    let assistants = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::AssistantMessage { .. }))
        .count();
    assert_eq!(assistants, 2);
}

#[tokio::test]
async fn max_iterations_one_stops_after_single_tool_round() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("echo", json!({ "text": "a" }))]),
        tool_call_response(&[("echo", json!({ "text": "b" }))]),
        text_response("done"),
    ]);
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    let mut messages = user("go");
    let stream = run_agent_loop(
        p,
        r,
        &mut messages,
        AgentLoopOptions {
            max_iterations: Some(1),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let err = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Error { .. }))
        .expect("expected error");
    if let AgentEvent::Error { message } = err {
        assert!(message.contains("Max iterations"), "got: {message}");
    }
}
