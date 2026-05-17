use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use heddle::agent::loop_::{
    run_agent_loop, run_agent_loop_streaming, AgentLoopOptions, PermissionResolver,
    PermissionResponse,
};
use heddle::agent::types::AgentEvent;
use heddle::config::loader::{ApprovalMode, PermissionsLayer};
use heddle::permissions::checker::PermissionChecker;
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::{ChatCompletionResponse, Message, StreamChunk, ToolDefinition, UserMessage};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

mod common;
use common::mocks::{finish_chunk, text_chunk, text_response, tool_call_chunk, tool_call_response};

// ─── Scripted provider (FIFO) ────────────────────────────────────────────

struct ScriptProvider {
    responses: std::sync::Mutex<Vec<ChatCompletionResponse>>,
    chunk_sets: std::sync::Mutex<Vec<Vec<StreamChunk>>>,
}

impl ScriptProvider {
    fn new(rs: Vec<ChatCompletionResponse>) -> Arc<Self> {
        Arc::new(Self {
            responses: std::sync::Mutex::new(rs),
            chunk_sets: std::sync::Mutex::new(Vec::new()),
        })
    }
    fn streaming(sets: Vec<Vec<StreamChunk>>) -> Arc<Self> {
        Arc::new(Self {
            responses: std::sync::Mutex::new(Vec::new()),
            chunk_sets: std::sync::Mutex::new(sets),
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
        let path = params.get("path").and_then(Value::as_str).unwrap_or("?");
        format!("wrote {path}")
    }
}

fn registry_with_write() -> ToolRegistry {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(WriteTool)).unwrap();
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

fn checker(mode: ApprovalMode) -> Arc<Mutex<PermissionChecker>> {
    Arc::new(Mutex::new(PermissionChecker::new(mode, None, None)))
}

fn checker_with_layers(
    mode: ApprovalMode,
    layers: Vec<PermissionsLayer>,
) -> Arc<Mutex<PermissionChecker>> {
    Arc::new(Mutex::new(PermissionChecker::new(
        mode,
        Some(&layers),
        None,
    )))
}

fn allow_resolver() -> PermissionResolver {
    Arc::new(|_, _| Box::pin(async move { PermissionResponse::Allow }))
}

fn deny_resolver() -> PermissionResolver {
    Arc::new(|_, _| Box::pin(async move { PermissionResponse::Deny }))
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn plan_mode_denies_write_file() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("write_file", json!({ "path": "foo.txt", "content": "bar" }))]),
        text_response("I cannot write in plan mode"),
    ]);
    let mut messages = user("write foo");
    let stream = run_agent_loop(
        p,
        registry_with_write(),
        &mut messages,
        AgentLoopOptions {
            permission_checker: Some(checker(ApprovalMode::Plan)),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let denied: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::PermissionDenied { .. }))
        .collect();
    assert_eq!(denied.len(), 1);
    if let AgentEvent::PermissionDenied { name, reason, .. } = denied[0] {
        assert_eq!(name, "write_file");
        assert!(!reason.is_empty());
    }
    let tool_ends = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolEnd { .. }))
        .count();
    assert_eq!(tool_ends, 0);
}

#[tokio::test]
async fn ask_resolver_allow_executes_tool() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("write_file", json!({ "path": "foo.txt", "content": "bar" }))]),
        text_response("wrote it"),
    ]);
    let mut messages = user("write foo");
    let stream = run_agent_loop(
        p,
        registry_with_write(),
        &mut messages,
        AgentLoopOptions {
            permission_checker: Some(checker(ApprovalMode::Suggest)),
            permission_resolver: Some(allow_resolver()),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let tool_ends: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolEnd { .. }))
        .collect();
    assert_eq!(tool_ends.len(), 1);
    if let AgentEvent::ToolEnd { result, .. } = tool_ends[0] {
        assert_eq!(result, "wrote foo.txt");
    }
}

#[tokio::test]
async fn ask_resolver_deny_blocks_tool() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("write_file", json!({ "path": "foo.txt", "content": "bar" }))]),
        text_response("denied"),
    ]);
    let mut messages = user("write foo");
    let stream = run_agent_loop(
        p,
        registry_with_write(),
        &mut messages,
        AgentLoopOptions {
            permission_checker: Some(checker(ApprovalMode::Suggest)),
            permission_resolver: Some(deny_resolver()),
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
async fn ask_resolver_always_only_called_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_resolver = calls.clone();
    let resolver: PermissionResolver = Arc::new(move |_, _| {
        let c = calls_for_resolver.clone();
        Box::pin(async move {
            c.fetch_add(1, Ordering::SeqCst);
            PermissionResponse::Always
        })
    });

    let p = ScriptProvider::new(vec![
        tool_call_response(&[("write_file", json!({ "path": "a.txt", "content": "1" }))]),
        tool_call_response(&[("write_file", json!({ "path": "b.txt", "content": "2" }))]),
        text_response("done"),
    ]);
    let mut messages = user("write two files");
    let stream = run_agent_loop(
        p,
        registry_with_write(),
        &mut messages,
        AgentLoopOptions {
            permission_checker: Some(checker(ApprovalMode::Suggest)),
            permission_resolver: Some(resolver),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let tool_ends = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolEnd { .. }))
        .count();
    assert_eq!(tool_ends, 2);
}

#[tokio::test]
async fn no_resolver_denies_by_default_in_ask_mode() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("write_file", json!({ "path": "foo.txt", "content": "bar" }))]),
        text_response("ok"),
    ]);
    let mut messages = user("write foo");
    let stream = run_agent_loop(
        p,
        registry_with_write(),
        &mut messages,
        AgentLoopOptions {
            permission_checker: Some(checker(ApprovalMode::Suggest)),
            permission_resolver: None,
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let denied = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::PermissionDenied { .. }))
        .count();
    assert_eq!(denied, 1);
}

#[tokio::test]
async fn env_deny_rule_overrides_full_auto() {
    let layers = vec![PermissionsLayer {
        allow: vec![],
        deny: vec!["Write(.env*)".to_string(), "Edit(.env*)".to_string()],
        ask: vec![],
    }];
    let p = ScriptProvider::new(vec![
        tool_call_response(&[(
            "write_file",
            json!({ "path": ".env.local", "content": "SECRET=x" }),
        )]),
        text_response("cannot write .env"),
    ]);
    let mut messages = user("write .env");
    let stream = run_agent_loop(
        p,
        registry_with_write(),
        &mut messages,
        AgentLoopOptions {
            permission_checker: Some(checker_with_layers(ApprovalMode::FullAuto, layers)),
            ..Default::default()
        },
    );
    let events = collect(stream).await;
    let denied = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::PermissionDenied { .. }))
        .count();
    assert_eq!(denied, 1);
}

#[tokio::test]
async fn streaming_loop_emits_deny_event() {
    let p = ScriptProvider::streaming(vec![
        vec![
            tool_call_chunk(
                0,
                Some("call_0"),
                Some("write_file"),
                Some(r#"{"path":"foo.txt","content":"bar"}"#),
            ),
            finish_chunk("tool_calls"),
        ],
        vec![
            text_chunk("I cannot write in plan mode"),
            finish_chunk("stop"),
        ],
    ]);
    let mut messages = user("write foo");
    let stream = run_agent_loop_streaming(
        p,
        registry_with_write(),
        &mut messages,
        AgentLoopOptions {
            permission_checker: Some(checker(ApprovalMode::Plan)),
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
