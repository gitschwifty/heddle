use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use heddle::agent::loop_::{run_agent_loop, run_agent_loop_streaming, AgentLoopOptions};
use heddle::agent::types::AgentEvent;
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::{ChatCompletionResponse, Message, StreamChunk, ToolDefinition, UserMessage};
use heddle::types::{Choice, ChoiceMessage, Usage};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

mod common;
use common::mocks::{
    finish_chunk, text_chunk, text_response, tool_call_chunk, tool_call_response, usage_chunk,
};

// ─── Providers ──────────────────────────────────────────────────────────

struct StreamScript {
    sets: Mutex<Vec<Vec<StreamChunk>>>,
    last_overrides: Mutex<Option<Value>>,
}

impl StreamScript {
    fn new(sets: Vec<Vec<StreamChunk>>) -> Arc<Self> {
        Arc::new(Self {
            sets: Mutex::new(sets),
            last_overrides: Mutex::new(None),
        })
    }
}

#[async_trait]
impl Provider for StreamScript {
    async fn send(
        &self,
        _messages: &[Message],
        _tools: Option<&[ToolDefinition]>,
        _overrides: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        Err(anyhow::anyhow!("send not used in streaming tests"))
    }
    fn stream(
        &self,
        _messages: Vec<Message>,
        _tools: Option<Vec<ToolDefinition>>,
        overrides: Value,
    ) -> ChunkStream {
        *self.last_overrides.lock().unwrap() = Some(overrides);
        let mut sets = self.sets.lock().unwrap();
        let chunks = if sets.is_empty() {
            Vec::new()
        } else {
            sets.remove(0)
        };
        Box::pin(try_stream! {
            for c in chunks { yield c; }
        })
    }
    fn with(&self, _o: Value) -> Arc<dyn Provider> {
        unimplemented!()
    }
}

struct ResponseScript {
    responses: Mutex<Vec<ChatCompletionResponse>>,
}

impl ResponseScript {
    fn new(rs: Vec<ChatCompletionResponse>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(rs),
        })
    }
}

#[async_trait]
impl Provider for ResponseScript {
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

fn empty_text_response() -> ChatCompletionResponse {
    ChatCompletionResponse {
        model: None,
        id: "chatcmpl-test".to_string(),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                content: None,
                tool_calls: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(Usage {
            prompt_tokens: 11,
            completion_tokens: 7,
            total_tokens: 18,
            ..Default::default()
        }),
    }
}

fn registry_with_echo() -> ToolRegistry {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    r
}

async fn collect<S: futures::Stream<Item = AgentEvent> + Unpin>(mut s: S) -> Vec<AgentEvent> {
    let mut v = Vec::new();
    while let Some(e) = s.next().await {
        v.push(e);
    }
    v
}

fn kind(e: &AgentEvent) -> &'static str {
    match e {
        AgentEvent::Usage { .. } => "usage",
        AgentEvent::AssistantMessage { .. } => "assistant_message",
        AgentEvent::ToolStart { .. } => "tool_start",
        AgentEvent::ToolEnd { .. } => "tool_end",
        AgentEvent::Error { .. } => "error",
        AgentEvent::ContentDelta { .. } => "content_delta",
        AgentEvent::LoopDetected { .. } => "loop_detected",
        _ => "other",
    }
}

// ─── Streaming Tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn text_only_streaming_yields_deltas_then_assistant_message() {
    let p = StreamScript::new(vec![vec![
        text_chunk("Hello"),
        text_chunk(" world"),
        finish_chunk("stop"),
    ]]);
    let mut messages = user("Hi");
    let stream = run_agent_loop_streaming(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;

    let deltas: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ContentDelta { .. }))
        .collect();
    assert_eq!(deltas.len(), 2);
    if let AgentEvent::ContentDelta { text } = deltas[0] {
        assert_eq!(text, "Hello");
    }
    if let AgentEvent::ContentDelta { text } = deltas[1] {
        assert_eq!(text, " world");
    }
    let assistants: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::AssistantMessage { .. }))
        .collect();
    assert_eq!(assistants.len(), 1);
    if let AgentEvent::AssistantMessage { message, .. } = assistants[0] {
        assert_eq!(message.content.as_deref(), Some("Hello world"));
    }
}

#[tokio::test]
async fn streaming_chunk_yields_routed_model_event() {
    let mut chunk = text_chunk("hello");
    chunk.model = Some("openai/gpt-4o-mini".to_string());
    let provider = StreamScript::new(vec![vec![chunk, finish_chunk("stop")]]);
    let mut messages = user("hi");
    let stream = run_agent_loop_streaming(
        provider,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );

    let events = collect(stream).await;

    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::RoutedModel { model } if model == "openai/gpt-4o-mini"
    )));
}

#[tokio::test]
async fn tool_call_deltas_assembled_and_executed() {
    let p = StreamScript::new(vec![
        vec![
            tool_call_chunk(0, Some("call_0"), Some("echo"), None),
            tool_call_chunk(0, None, None, Some(r#"{"te"#)),
            tool_call_chunk(0, None, None, Some(r#"xt":""#)),
            tool_call_chunk(0, None, None, Some(r#"ping"}"#)),
            finish_chunk("tool_calls"),
        ],
        vec![text_chunk("Got: ping"), finish_chunk("stop")],
    ]);
    let mut messages = user("echo ping");
    let stream = run_agent_loop_streaming(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;

    let tool_start = events
        .iter()
        .find(|e| matches!(e, AgentEvent::ToolStart { .. }))
        .expect("tool_start");
    if let AgentEvent::ToolStart { name, .. } = tool_start {
        assert_eq!(name, "echo");
    }
    let tool_end = events
        .iter()
        .find(|e| matches!(e, AgentEvent::ToolEnd { .. }))
        .expect("tool_end");
    if let AgentEvent::ToolEnd { name, result, .. } = tool_end {
        assert_eq!(name, "echo");
        assert_eq!(result, "ping");
    }
}

#[tokio::test]
async fn mixed_content_and_tool_calls_in_stream() {
    let p = StreamScript::new(vec![
        vec![
            text_chunk("Let me "),
            text_chunk("do that."),
            tool_call_chunk(0, Some("call_0"), Some("echo"), None),
            tool_call_chunk(0, None, None, Some(r#"{"text":"hi"}"#)),
            finish_chunk("tool_calls"),
        ],
        vec![text_chunk("Done"), finish_chunk("stop")],
    ]);
    let mut messages = user("do it");
    let stream = run_agent_loop_streaming(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;

    let deltas = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ContentDelta { .. }))
        .count();
    assert_eq!(deltas, 3, "got {deltas} content_delta events");

    let assistants: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::AssistantMessage { .. }))
        .collect();
    assert_eq!(assistants.len(), 2);
    if let AgentEvent::AssistantMessage { message, .. } = assistants[0] {
        assert_eq!(message.content.as_deref(), Some("Let me do that."));
        assert_eq!(message.tool_calls.as_ref().map(|v| v.len()), Some(1));
    }
}

#[tokio::test]
async fn parallel_tool_calls_from_stream() {
    let p = StreamScript::new(vec![
        vec![
            tool_call_chunk(0, Some("call_0"), Some("echo"), None),
            tool_call_chunk(0, None, None, Some(r#"{"text":"a"}"#)),
            tool_call_chunk(1, Some("call_1"), Some("echo"), None),
            tool_call_chunk(1, None, None, Some(r#"{"text":"b"}"#)),
            finish_chunk("tool_calls"),
        ],
        vec![text_chunk("Both done"), finish_chunk("stop")],
    ]);
    let mut messages = user("parallel");
    let stream = run_agent_loop_streaming(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;

    let starts = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolStart { .. }))
        .count();
    let ends: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolEnd { .. }))
        .collect();
    assert_eq!(starts, 2);
    assert_eq!(ends.len(), 2);
    if let AgentEvent::ToolEnd { result, .. } = ends[0] {
        assert_eq!(result, "a");
    }
    if let AgentEvent::ToolEnd { result, .. } = ends[1] {
        assert_eq!(result, "b");
    }
}

#[tokio::test]
async fn multi_turn_streaming_tool_then_text() {
    let p = StreamScript::new(vec![
        vec![
            tool_call_chunk(0, Some("call_0"), Some("echo"), None),
            tool_call_chunk(0, None, None, Some(r#"{"text":"first"}"#)),
            finish_chunk("tool_calls"),
        ],
        vec![text_chunk("Got first"), finish_chunk("stop")],
    ]);
    let mut messages = user("do it");
    let stream = run_agent_loop_streaming(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;
    let kinds: Vec<&str> = events.iter().map(kind).collect();
    assert_eq!(
        kinds,
        vec![
            "assistant_message",
            "tool_start",
            "tool_end",
            "content_delta",
            "assistant_message"
        ]
    );
    assert_eq!(messages.len(), 4);
}

#[tokio::test]
async fn empty_streaming_response_yields_error_with_usage_details() {
    let p = StreamScript::new(vec![vec![
        finish_chunk("stop"),
        usage_chunk(42, 9, 51, None),
    ]]);
    let mut messages = user("Hi");
    let stream = run_agent_loop_streaming(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;

    assert_eq!(messages.len(), 1, "empty assistant should not be appended");
    let error = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Error { .. }))
        .expect("expected error");
    if let AgentEvent::Error { message } = error {
        assert!(message.contains("Empty assistant response"));
        assert!(message.contains("finish_reason=stop"));
        assert!(message.contains("completion_tokens=9"));
    }
}

#[tokio::test]
async fn empty_streaming_response_retries_once_and_can_recover() {
    let p = StreamScript::new(vec![
        vec![finish_chunk("stop"), usage_chunk(42, 9, 51, None)],
        vec![text_chunk("Recovered"), finish_chunk("stop")],
    ]);
    let mut messages = user("Hi");
    let stream = run_agent_loop_streaming(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;

    let errors = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Error { .. }))
        .count();
    assert_eq!(errors, 1);
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::AssistantMessage { message, .. }
                if message.content.as_deref() == Some("Recovered")
        )),
        "expected recovered assistant message"
    );
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn empty_non_streaming_response_yields_error_with_usage_details() {
    let p = ResponseScript::new(vec![empty_text_response()]);
    let mut messages = user("Hi");
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;

    assert_eq!(messages.len(), 1, "empty assistant should not be appended");
    let error = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Error { .. }))
        .expect("expected error");
    if let AgentEvent::Error { message } = error {
        assert!(message.contains("Empty assistant response"));
        assert!(message.contains("finish_reason=stop"));
        assert!(message.contains("completion_tokens=7"));
    }
}

#[tokio::test]
async fn empty_non_streaming_response_retries_once_and_can_recover() {
    let p = ResponseScript::new(vec![empty_text_response(), text_response("Recovered")]);
    let mut messages = user("Hi");
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;

    let errors = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Error { .. }))
        .count();
    assert_eq!(errors, 1);
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::AssistantMessage { message, .. }
                if message.content.as_deref() == Some("Recovered")
        )),
        "expected recovered assistant message"
    );
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn request_overrides_pass_through_to_stream() {
    let p = StreamScript::new(vec![vec![text_chunk("Hi"), finish_chunk("stop")]]);
    let p2 = p.clone();
    let overrides = json!({ "temperature": 0.7 });
    let mut messages = user("Hi");
    let stream = run_agent_loop_streaming(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions {
            request_overrides: Some(overrides.clone()),
            ..Default::default()
        },
    );
    collect(stream).await;
    let captured = p2.last_overrides.lock().unwrap().clone().unwrap();
    assert_eq!(captured, overrides);
}

#[tokio::test]
async fn doom_loop_detected_after_three_identical_iterations_streaming() {
    let same = || {
        vec![
            tool_call_chunk(0, Some("call_0"), Some("echo"), None),
            tool_call_chunk(0, None, None, Some(r#"{"text":"same"}"#)),
            finish_chunk("tool_calls"),
        ]
    };
    let p = StreamScript::new(vec![
        same(),
        same(),
        same(),
        vec![text_chunk("unreachable"), finish_chunk("stop")],
    ]);
    let mut messages = user("loop");
    let stream = run_agent_loop_streaming(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions {
            doom_loop_threshold: Some(3),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let loops: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::LoopDetected { .. }))
        .collect();
    assert_eq!(loops.len(), 1);
    if let AgentEvent::LoopDetected { count } = loops[0] {
        assert_eq!(*count, 3);
    }
}

#[tokio::test]
async fn two_identical_calls_below_threshold_does_not_trigger_loop() {
    let same = || {
        vec![
            tool_call_chunk(0, Some("call_0"), Some("echo"), None),
            tool_call_chunk(0, None, None, Some(r#"{"text":"same"}"#)),
            finish_chunk("tool_calls"),
        ]
    };
    let p = StreamScript::new(vec![
        same(),
        same(),
        vec![text_chunk("done"), finish_chunk("stop")],
    ]);
    let mut messages = user("twice");
    let stream = run_agent_loop_streaming(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions {
            doom_loop_threshold: Some(3),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let loops = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::LoopDetected { .. }))
        .count();
    assert_eq!(loops, 0);
}

#[tokio::test]
async fn usage_event_from_streaming_via_usage_chunk() {
    let p = StreamScript::new(vec![vec![
        text_chunk("Hello"),
        finish_chunk("stop"),
        usage_chunk(20, 10, 30, None),
    ]]);
    let mut messages = user("Hi");
    let stream = run_agent_loop_streaming(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;
    let usages: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Usage { .. }))
        .collect();
    assert_eq!(usages.len(), 1);
    if let AgentEvent::Usage { usage } = usages[0] {
        assert_eq!(usage.prompt_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }
}

#[tokio::test]
async fn no_usage_event_when_no_chunk_has_usage() {
    let p = StreamScript::new(vec![vec![text_chunk("Hello"), finish_chunk("stop")]]);
    let mut messages = user("Hi");
    let stream = run_agent_loop_streaming(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;
    let usages = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Usage { .. }))
        .count();
    assert_eq!(usages, 0);
}

#[tokio::test]
async fn usage_event_comes_after_assistant_message() {
    let p = StreamScript::new(vec![vec![
        text_chunk("Hello"),
        finish_chunk("stop"),
        usage_chunk(20, 10, 30, None),
    ]]);
    let mut messages = user("Hi");
    let stream = run_agent_loop_streaming(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;
    let kinds: Vec<&str> = events.iter().map(kind).collect();
    let assistant_idx = kinds
        .iter()
        .position(|k| *k == "assistant_message")
        .unwrap();
    let usage_idx = kinds.iter().position(|k| *k == "usage").unwrap();
    assert!(usage_idx > assistant_idx, "kinds: {kinds:?}");
}

#[tokio::test]
async fn multi_turn_streaming_two_rounds_two_usage_events() {
    let p = StreamScript::new(vec![
        vec![
            tool_call_chunk(0, Some("call_0"), Some("echo"), None),
            tool_call_chunk(0, None, None, Some(r#"{"text":"ping"}"#)),
            finish_chunk("tool_calls"),
            usage_chunk(20, 10, 30, None),
        ],
        vec![
            text_chunk("Done"),
            finish_chunk("stop"),
            usage_chunk(30, 15, 45, None),
        ],
    ]);
    let mut messages = user("echo");
    let stream = run_agent_loop_streaming(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions::default(),
    );
    let events = collect(stream).await;
    let usages = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Usage { .. }))
        .count();
    assert_eq!(usages, 2);
}

#[tokio::test]
async fn streaming_different_calls_does_not_trigger_doom_loop() {
    let p = StreamScript::new(vec![
        vec![
            tool_call_chunk(0, Some("call_0"), Some("echo"), None),
            tool_call_chunk(0, None, None, Some(r#"{"text":"a"}"#)),
            finish_chunk("tool_calls"),
        ],
        vec![
            tool_call_chunk(0, Some("call_0"), Some("echo"), None),
            tool_call_chunk(0, None, None, Some(r#"{"text":"b"}"#)),
            finish_chunk("tool_calls"),
        ],
        vec![
            tool_call_chunk(0, Some("call_0"), Some("echo"), None),
            tool_call_chunk(0, None, None, Some(r#"{"text":"c"}"#)),
            finish_chunk("tool_calls"),
        ],
        vec![text_chunk("done"), finish_chunk("stop")],
    ]);
    let mut messages = user("different");
    let stream = run_agent_loop_streaming(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions {
            doom_loop_threshold: Some(3),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let loops = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::LoopDetected { .. }))
        .count();
    assert_eq!(loops, 0);
}

// ─── Doom loop detection in non-streaming (`run_agent_loop`) ─────────────

#[tokio::test]
async fn non_streaming_three_identical_calls_yields_loop_detected() {
    let p = ResponseScript::new(vec![
        tool_call_response(&[("echo", json!({ "text": "same" }))]),
        tool_call_response(&[("echo", json!({ "text": "same" }))]),
        tool_call_response(&[("echo", json!({ "text": "same" }))]),
        text_response("unreachable"),
    ]);
    let mut messages = user("loop");
    let stream = run_agent_loop(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions {
            doom_loop_threshold: Some(3),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let loops: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::LoopDetected { .. }))
        .collect();
    assert_eq!(loops.len(), 1);
    if let AgentEvent::LoopDetected { count } = loops[0] {
        assert_eq!(*count, 3);
    }
}

#[tokio::test]
async fn non_streaming_different_calls_do_not_trigger_doom_loop() {
    let p = ResponseScript::new(vec![
        tool_call_response(&[("echo", json!({ "text": "a" }))]),
        tool_call_response(&[("echo", json!({ "text": "b" }))]),
        tool_call_response(&[("echo", json!({ "text": "c" }))]),
        text_response("done"),
    ]);
    let mut messages = user("different");
    let stream = run_agent_loop(
        p,
        registry_with_echo(),
        &mut messages,
        AgentLoopOptions {
            doom_loop_threshold: Some(3),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let loops = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::LoopDetected { .. }))
        .count();
    assert_eq!(loops, 0);
    let assistants = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::AssistantMessage { .. }))
        .count();
    assert!(assistants >= 4, "got {assistants}");
}
