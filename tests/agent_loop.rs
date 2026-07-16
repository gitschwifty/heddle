use async_trait::async_trait;
use futures::StreamExt;
use heddle::agent::loop_::{run_agent_loop, AgentLoopOptions};
use heddle::agent::types::AgentEvent;
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::{ChatCompletionResponse, Message, ToolDefinition, Usage, UserMessage};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

mod common;
use common::mocks::{text_response, tool_call_response};

// ─── Test helpers ────────────────────────────────────────────────────────

/// Provider that pops scripted responses; captures the overrides passed to send().
struct CapturingMock {
    responses: Mutex<Vec<ChatCompletionResponse>>,
    last_overrides: Mutex<Option<Value>>,
}

impl CapturingMock {
    fn new(responses: Vec<ChatCompletionResponse>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(responses),
            last_overrides: Mutex::new(None),
        })
    }
}

#[async_trait]
impl Provider for CapturingMock {
    async fn send(
        &self,
        _messages: &[Message],
        _tools: Option<&[ToolDefinition]>,
        overrides: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        *self.last_overrides.lock().unwrap() = Some(overrides.clone());
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
        unimplemented!("stream not used in loop tests")
    }
    fn with(&self, _overrides: Value) -> Arc<dyn Provider> {
        unimplemented!("with not used in loop tests")
    }
}

struct EchoTool;
#[async_trait]
impl HeddleTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "Returns the input string"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "text": { "type": "string" } },
            "required": ["text"]
        })
    }
    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        params
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }
}

fn user(content: &str) -> Vec<Message> {
    vec![Message::User(UserMessage {
        content: content.to_string(),
    })]
}

async fn collect_events<S>(mut s: S) -> Vec<AgentEvent>
where
    S: futures::Stream<Item = AgentEvent> + Unpin,
{
    let mut events = Vec::new();
    while let Some(e) = s.next().await {
        events.push(e);
    }
    events
}

fn event_kind(e: &AgentEvent) -> &'static str {
    match e {
        AgentEvent::Usage { .. } => "usage",
        AgentEvent::RoutedModel { .. } => "routed_model",
        AgentEvent::AssistantMessage { .. } => "assistant_message",
        AgentEvent::ToolStart { .. } => "tool_start",
        AgentEvent::ToolEnd { .. } => "tool_end",
        AgentEvent::Error { .. } => "error",
        AgentEvent::ContentDelta { .. } => "content_delta",
        AgentEvent::LoopDetected { .. } => "loop_detected",
        AgentEvent::PermissionRequest { .. } => "permission_request",
        AgentEvent::PermissionDenied { .. } => "permission_denied",
        AgentEvent::PlanComplete { .. } => "plan_complete",
        AgentEvent::ContextPrune { .. } => "context_prune",
        AgentEvent::ContextCompact => "context_compact",
        AgentEvent::ContextHandoff => "context_handoff",
    }
}

fn registry_with_echo() -> ToolRegistry {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    r
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn text_only_response_terminates_immediately() {
    let p = CapturingMock::new(vec![text_response("Hello!")]);
    let mut messages = user("Hi");
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect_events(stream).await;

    assert_eq!(events.len(), 2);
    assert_eq!(event_kind(&events[0]), "usage");
    assert_eq!(event_kind(&events[1]), "assistant_message");
    if let AgentEvent::AssistantMessage { message, .. } = &events[1] {
        assert_eq!(message.content.as_deref(), Some("Hello!"));
    } else {
        panic!("expected assistant_message");
    }
}

#[tokio::test]
async fn non_streaming_response_yields_routed_model_event() {
    let mut response = text_response("hi");
    response.model = Some("anthropic/claude-3-sonnet".to_string());
    let provider = CapturingMock::new(vec![response]);
    let mut messages = user("hello");
    let stream = run_agent_loop(
        provider,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );

    let events = collect_events(stream).await;

    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::RoutedModel { model } if model == "anthropic/claude-3-sonnet"
    )));
}

#[tokio::test]
async fn tool_call_then_text_response_single_turn() {
    let p = CapturingMock::new(vec![
        tool_call_response(&[("echo", json!({ "text": "ping" }))]),
        text_response("Got: ping"),
    ]);
    let mut messages = user("echo ping");
    let stream = run_agent_loop(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect_events(stream).await;

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
        ]
    );
    if let AgentEvent::ToolEnd { name, result, .. } = &events[3] {
        assert_eq!(name, "echo");
        assert_eq!(result, "ping");
    } else {
        panic!("expected tool_end");
    }
    if let AgentEvent::AssistantMessage { message, .. } = &events[5] {
        assert_eq!(message.content.as_deref(), Some("Got: ping"));
    } else {
        panic!("expected final assistant_message");
    }
}

#[tokio::test]
async fn multi_turn_tool_loop() {
    let p = CapturingMock::new(vec![
        tool_call_response(&[("echo", json!({ "text": "first" }))]),
        tool_call_response(&[("echo", json!({ "text": "second" }))]),
        text_response("Done"),
    ]);
    let mut messages = user("do two things");
    let stream = run_agent_loop(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let kinds: Vec<&str> = collect_events(stream)
        .await
        .iter()
        .map(event_kind)
        .collect();
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
            "assistant_message",
        ]
    );
}

#[tokio::test]
async fn parallel_tool_calls_in_single_response() {
    let p = CapturingMock::new(vec![
        tool_call_response(&[
            ("echo", json!({ "text": "a" })),
            ("echo", json!({ "text": "b" })),
        ]),
        text_response("Both done"),
    ]);
    let mut messages = user("parallel");
    let stream = run_agent_loop(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect_events(stream).await;
    let starts = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolStart { .. }))
        .count();
    let ends = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolEnd { .. }))
        .count();
    assert_eq!(starts, 2);
    assert_eq!(ends, 2);
    // Sequence: usage, assistant, start, end, start, end, usage, assistant
    assert_eq!(events.len(), 8);
}

#[tokio::test]
async fn max_iterations_prevents_infinite_loop() {
    let mut responses: Vec<ChatCompletionResponse> = (0..20)
        .map(|i| tool_call_response(&[("echo", json!({ "text": format!("loop-{i}") }))]))
        .collect();
    // ensure we have enough responses if the loop tries to keep going
    responses.push(text_response("end"));
    let p = CapturingMock::new(responses);
    let mut messages = user("loop");
    let stream = run_agent_loop(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions {
            max_iterations: Some(3),
            ..Default::default()
        },
    );
    let events = collect_events(stream).await;
    let errors: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Error { .. }))
        .collect();
    assert_eq!(errors.len(), 1);
    if let AgentEvent::Error { message } = errors[0] {
        assert!(message.contains("Max iterations"), "got: {message}");
    }
}

#[tokio::test]
async fn request_overrides_pass_through_to_provider() {
    let p = CapturingMock::new(vec![text_response("Hi")]);
    let p2 = p.clone();
    let mut messages = user("Hi");
    let overrides = json!({ "temperature": 0.7 });
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions {
            request_overrides: Some(overrides.clone()),
            ..Default::default()
        },
    );
    collect_events(stream).await;
    let captured = p2.last_overrides.lock().unwrap().clone().unwrap();
    assert_eq!(captured, overrides);
}

#[tokio::test]
async fn usage_event_yields_token_counts() {
    let p = CapturingMock::new(vec![text_response("Hello!")]);
    let mut messages = user("Hi");
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect_events(stream).await;
    let usages: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Usage { .. }))
        .collect();
    assert_eq!(usages.len(), 1);
    if let AgentEvent::Usage { usage } = usages[0] {
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }
}

#[tokio::test]
async fn no_usage_event_when_response_usage_absent() {
    let no_usage = ChatCompletionResponse {
        model: None,
        id: "x".to_string(),
        choices: vec![heddle::types::Choice {
            index: 0,
            message: heddle::types::ChoiceMessage {
                content: Some("Hi".to_string()),
                tool_calls: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: None,
    };
    let p = CapturingMock::new(vec![no_usage]);
    let mut messages = user("Hi");
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect_events(stream).await;
    let usages = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Usage { .. }))
        .count();
    assert_eq!(usages, 0);
}

#[tokio::test]
async fn two_llm_calls_produce_two_usage_events() {
    let p = CapturingMock::new(vec![
        tool_call_response(&[("echo", json!({ "text": "ping" }))]),
        text_response("Done"),
    ]);
    let mut messages = user("echo");
    let stream = run_agent_loop(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let usages = collect_events(stream)
        .await
        .iter()
        .filter(|e| matches!(e, AgentEvent::Usage { .. }))
        .count();
    assert_eq!(usages, 2);
}

#[tokio::test]
async fn usage_includes_cost_when_present() {
    let with_cost = ChatCompletionResponse {
        model: None,
        id: "x".to_string(),
        choices: vec![heddle::types::Choice {
            index: 0,
            message: heddle::types::ChoiceMessage {
                content: Some("Hi".to_string()),
                tool_calls: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            cost: Some(0.001),
            ..Default::default()
        }),
    };
    let p = CapturingMock::new(vec![with_cost]);
    let mut messages = user("Hi");
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect_events(stream).await;
    if let Some(AgentEvent::Usage { usage }) = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Usage { .. }))
    {
        assert_eq!(usage.cost, Some(0.001));
    } else {
        panic!("expected a usage event");
    }
}

#[tokio::test]
async fn mutates_passed_in_messages_array() {
    let p = CapturingMock::new(vec![
        tool_call_response(&[("echo", json!({ "text": "ping" }))]),
        text_response("Done"),
    ]);
    let mut messages = user("echo ping");
    let stream = run_agent_loop(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    collect_events(stream).await;
    assert_eq!(messages.len(), 4);
    assert!(matches!(messages[0], Message::User(_)));
    assert!(matches!(messages[1], Message::Assistant(_)));
    assert!(matches!(messages[2], Message::Tool(_)));
    assert!(matches!(messages[3], Message::Assistant(_)));
}
