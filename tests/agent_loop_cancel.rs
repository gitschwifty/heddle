use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use heddle::agent::loop_::{run_agent_loop, run_agent_loop_streaming, AgentLoopOptions};
use heddle::agent::types::AgentEvent;
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::{ChatCompletionResponse, Message, StreamChunk, ToolDefinition, UserMessage};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

mod common;
use common::mocks::{finish_chunk, text_chunk, text_response, tool_call_chunk, tool_call_response};

// ─── Scripted provider (FIFO) ─────────────────────────────────────────────

struct ScriptProvider {
    responses: Mutex<Vec<ChatCompletionResponse>>,
    chunk_sets: Mutex<Vec<Vec<StreamChunk>>>,
}
impl ScriptProvider {
    fn new(rs: Vec<ChatCompletionResponse>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(rs),
            chunk_sets: Mutex::new(Vec::new()),
        })
    }
    fn streaming(sets: Vec<Vec<StreamChunk>>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(Vec::new()),
            chunk_sets: Mutex::new(sets),
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
        let mut sets = self.chunk_sets.lock().unwrap();
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

// ─── Slow tool that honors cancellation ──────────────────────────────────

struct SlowTool;
#[async_trait]
impl HeddleTool for SlowTool {
    fn name(&self) -> &str {
        "slow"
    }
    fn description(&self) -> &str {
        "A tool that takes a while"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "ms": { "type": "number" } },
            "required": ["ms"]
        })
    }
    async fn execute(&self, params: Value, options: ExecOptions) -> String {
        let ms = params.get("ms").and_then(Value::as_u64).unwrap_or(0);
        let signal = options.signal.clone();
        if let Some(s) = &signal {
            if s.is_cancelled() {
                return "aborted".to_string();
            }
        }
        let dur = Duration::from_millis(ms);
        match &signal {
            Some(s) => {
                tokio::select! {
                    _ = tokio::time::sleep(dur) => "done".to_string(),
                    _ = s.cancelled() => "aborted".to_string(),
                }
            }
            None => {
                tokio::time::sleep(dur).await;
                "done".to_string()
            }
        }
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

fn registry_with_slow() -> ToolRegistry {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(SlowTool)).unwrap();
    r
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn loop_exits_immediately_when_signal_already_cancelled() {
    let p = ScriptProvider::new(vec![text_response("Hello!")]);
    let token = CancellationToken::new();
    token.cancel();
    let mut messages = user("Hi");
    let stream = run_agent_loop(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions {
            signal: Some(token),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    assert!(events.is_empty(), "got {} events", events.len());
}

#[tokio::test]
async fn loop_exits_mid_iteration_when_signal_cancelled_during_tool() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("slow", json!({ "ms": 5000 }))]),
        text_response("Done!"),
    ]);
    let token = CancellationToken::new();
    let token_for_cancel = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        token_for_cancel.cancel();
    });
    let mut messages = user("Do something slow");
    let stream = run_agent_loop(
        p,
        registry_with_slow(),
        &mut messages,
        AgentLoopOptions {
            signal: Some(token),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let second = events.iter().find(|e| {
        matches!(
            e,
            AgentEvent::AssistantMessage { message, .. } if message.content.as_deref() == Some("Done!")
        )
    });
    assert!(second.is_none(), "should not reach second iteration");
}

#[tokio::test]
async fn streaming_loop_exits_immediately_when_signal_already_cancelled() {
    let p = ScriptProvider::streaming(vec![vec![text_chunk("Hello!"), finish_chunk("stop")]]);
    let token = CancellationToken::new();
    token.cancel();
    let mut messages = user("Hi");
    let stream = run_agent_loop_streaming(
        p,
        ToolRegistry::new(),
        &mut messages,
        AgentLoopOptions {
            signal: Some(token),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    assert!(events.is_empty(), "got {} events", events.len());
}

#[tokio::test]
async fn streaming_loop_does_not_reach_second_iteration_after_cancel() {
    let p = ScriptProvider::streaming(vec![
        vec![
            tool_call_chunk(0, Some("call_0"), Some("slow"), Some(r#"{"ms":5000}"#)),
            finish_chunk("tool_calls"),
        ],
        vec![text_chunk("Done!"), finish_chunk("stop")],
    ]);
    let token = CancellationToken::new();
    let token_for_cancel = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        token_for_cancel.cancel();
    });
    let mut messages = user("Go");
    let stream = run_agent_loop_streaming(
        p,
        registry_with_slow(),
        &mut messages,
        AgentLoopOptions {
            signal: Some(token),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let second = events
        .iter()
        .find(|e| matches!(e, AgentEvent::ContentDelta { text } if text == "Done!"));
    assert!(second.is_none(), "should not reach second iteration");
}
