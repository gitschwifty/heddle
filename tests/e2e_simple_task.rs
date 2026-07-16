//! E2E tests: agent loop driving real tools (read/edit) with a scripted mock
//! provider.

use async_trait::async_trait;
use futures::StreamExt;
use heddle::agent::loop_::{run_agent_loop, AgentLoopOptions};
use heddle::agent::types::AgentEvent;
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::edit::create_edit_tool;
use heddle::tools::read::create_read_tool;
use heddle::tools::registry::ToolRegistry;
use heddle::types::{ChatCompletionResponse, Message, ToolDefinition, UserMessage};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

mod common;
use common::mocks::{text_response, tool_call_response};

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
        _m: &[Message],
        _t: Option<&[ToolDefinition]>,
        _o: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        let mut v = self.responses.lock().unwrap();
        if v.is_empty() {
            return Err(anyhow::anyhow!("No more mock responses"));
        }
        Ok(v.remove(0))
    }
    fn stream(&self, _m: Vec<Message>, _t: Option<Vec<ToolDefinition>>, _o: Value) -> ChunkStream {
        unimplemented!()
    }
    fn with(&self, _o: Value) -> Arc<dyn Provider> {
        unimplemented!()
    }
}

fn user(c: &str) -> Vec<Message> {
    vec![Message::User(UserMessage { content: c.into() })]
}

async fn collect(
    provider: Arc<dyn Provider>,
    registry: ToolRegistry,
    messages: &mut Vec<Message>,
) -> Vec<AgentEvent> {
    let stream = run_agent_loop(provider, registry, messages, AgentLoopOptions::default());
    futures::pin_mut!(stream);
    let mut out = Vec::new();
    while let Some(e) = stream.next().await {
        out.push(e);
    }
    out
}

fn event_kind(e: &AgentEvent) -> &'static str {
    match e {
        AgentEvent::Usage { .. } => "usage",
        AgentEvent::RoutedModel { .. } => "routed_model",
        AgentEvent::AssistantMessage { .. } => "assistant_message",
        AgentEvent::ToolStart { .. } => "tool_start",
        AgentEvent::ToolEnd { .. } => "tool_end",
        AgentEvent::ContentDelta { .. } => "content_delta",
        AgentEvent::Error { .. } => "error",
        AgentEvent::LoopDetected { .. } => "loop_detected",
        AgentEvent::PermissionDenied { .. } => "permission_denied",
        AgentEvent::PermissionRequest { .. } => "permission_request",
        AgentEvent::PlanComplete { .. } => "plan_complete",
        AgentEvent::ContextPrune { .. } => "context_prune",
        AgentEvent::ContextCompact => "context_compact",
        AgentEvent::ContextHandoff => "context_handoff",
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn read_file_tool_call_then_text_response() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("hello.txt");
    std::fs::write(&file_path, "Hello from the test file!").unwrap();

    let provider = ScriptProvider::new(vec![
        tool_call_response(&[(
            "read_file",
            json!({ "file_path": file_path.to_string_lossy() }),
        )]),
        text_response("The file contains: \"Hello from the test file!\""),
    ]);

    let mut registry = ToolRegistry::new();
    registry.register(create_read_tool()).unwrap();

    let mut messages = user(&format!("Read the file at {}", file_path.display()));
    let events = collect(provider, registry, &mut messages).await;

    let kinds: Vec<&str> = events.iter().map(event_kind).collect();
    assert_eq!(
        kinds,
        vec![
            "usage",
            "assistant_message",
            "tool_start",
            "tool_end",
            "usage",
            "assistant_message"
        ]
    );

    if let AgentEvent::ToolEnd { result, .. } = &events[3] {
        assert!(
            result.contains("Hello from the test file!"),
            "got: {result}"
        );
    } else {
        panic!("expected ToolEnd at index 3");
    }
    if let AgentEvent::AssistantMessage { message, .. } = &events[5] {
        assert!(message
            .content
            .as_deref()
            .unwrap_or("")
            .contains("Hello from the test file!"));
    } else {
        panic!("expected AssistantMessage at index 5");
    }
}

#[tokio::test]
async fn edit_file_tool_call_then_confirmation_response() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("code.ts");
    std::fs::write(
        &file_path,
        "const greeting = \"hello\";\nconsole.log(greeting);",
    )
    .unwrap();

    let provider = ScriptProvider::new(vec![
        tool_call_response(&[(
            "edit_file",
            json!({
                "file_path": file_path.to_string_lossy(),
                "old_string": "\"hello\"",
                "new_string": "\"world\""
            }),
        )]),
        text_response("I've updated the greeting from \"hello\" to \"world\"."),
    ]);

    let mut registry = ToolRegistry::new();
    registry.register(create_edit_tool()).unwrap();

    let mut messages = user("Change the greeting to world");
    let events = collect(provider, registry, &mut messages).await;

    assert_eq!(events.len(), 6);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(
        content,
        "const greeting = \"world\";\nconsole.log(greeting);"
    );

    if let AgentEvent::ToolEnd { result, .. } = &events[3] {
        assert!(result.contains("Applied edit"), "got: {result}");
    } else {
        panic!("expected ToolEnd at index 3");
    }
}

#[tokio::test]
async fn multi_tool_chain_read_then_edit() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("data.txt");
    std::fs::write(&file_path, "count: 0").unwrap();

    let provider = ScriptProvider::new(vec![
        tool_call_response(&[(
            "read_file",
            json!({ "file_path": file_path.to_string_lossy() }),
        )]),
        tool_call_response(&[(
            "edit_file",
            json!({
                "file_path": file_path.to_string_lossy(),
                "old_string": "count: 0",
                "new_string": "count: 1"
            }),
        )]),
        text_response("I read the file, saw count: 0, and updated it to count: 1."),
    ]);

    let mut registry = ToolRegistry::new();
    registry.register(create_read_tool()).unwrap();
    registry.register(create_edit_tool()).unwrap();

    let mut messages = user("Increment the count in data.txt");
    let events = collect(provider, registry, &mut messages).await;

    assert_eq!(events.len(), 10);
    let kinds: Vec<&str> = events.iter().map(event_kind).collect();
    assert_eq!(
        kinds,
        vec![
            "usage",
            "assistant_message",
            "tool_start",
            "tool_end",
            "usage",
            "assistant_message",
            "tool_start",
            "tool_end",
            "usage",
            "assistant_message"
        ]
    );

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "count: 1");
}
