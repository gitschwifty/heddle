use async_trait::async_trait;
use futures::StreamExt;
use heddle::agent::loop_::{run_agent_loop, AgentLoopOptions};
use heddle::provider::types::{ChunkStream, Provider};
use heddle::session::jsonl::{append_message, load_session, write_session_meta, SessionMeta};
use heddle::tools::edit::create_edit_tool;
use heddle::tools::read::create_read_tool;
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::tools::write::create_write_tool;
use heddle::types::{
    ChatCompletionResponse, Message, SystemMessage, ToolDefinition, ToolMessage, UserMessage,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

mod common;
use common::{
    mocks::{text_response, tool_call_response},
    Sandbox,
};

// ─── Scripted provider ───────────────────────────────────────────────────

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

fn echo_registry() -> ToolRegistry {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    r
}

fn sys() -> Message {
    Message::System(SystemMessage {
        content: "You are a helpful assistant.".to_string(),
    })
}
fn user(c: &str) -> Message {
    Message::User(UserMessage {
        content: c.to_string(),
    })
}

async fn run(provider: Arc<dyn Provider>, registry: ToolRegistry, messages: &mut Vec<Message>) {
    let stream = run_agent_loop(provider, registry, messages, AgentLoopOptions::default());
    futures::pin_mut!(stream);
    while stream.next().await.is_some() {}
}

#[tokio::test]
async fn message_accumulation_across_two_turns() {
    let mut messages = vec![sys(), user("What is 2+2?")];

    let p1 = ScriptProvider::new(vec![
        tool_call_response(&[("echo", json!({ "text": "4" }))]),
        text_response("The answer is 4."),
    ]);
    run(p1, echo_registry(), &mut messages).await;

    assert_eq!(messages.len(), 5);
    assert!(matches!(messages[0], Message::System(_)));
    assert!(matches!(messages[1], Message::User(_)));
    assert!(matches!(messages[2], Message::Assistant(_)));
    assert!(matches!(messages[3], Message::Tool(_)));
    assert!(matches!(messages[4], Message::Assistant(_)));

    messages.push(user("And 3+3?"));
    let p2 = ScriptProvider::new(vec![text_response("The answer is 6.")]);
    run(p2, echo_registry(), &mut messages).await;

    assert_eq!(messages.len(), 7);
    assert!(matches!(messages[5], Message::User(_)));
    assert!(matches!(messages[6], Message::Assistant(_)));
}

#[tokio::test]
async fn context_carryover_read_then_edit_across_turns() {
    let _sb = Sandbox::new("mt-read-edit");
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.txt");
    std::fs::write(&file, "count: 0").unwrap();

    let mut r = ToolRegistry::new();
    r.register(create_read_tool()).unwrap();
    r.register(create_edit_tool()).unwrap();

    let mut messages = vec![sys(), user(&format!("Read the file at {}", file.display()))];

    let p1 = ScriptProvider::new(vec![
        tool_call_response(&[("read_file", json!({ "file_path": file.to_string_lossy() }))]),
        text_response("The file contains: count: 0"),
    ]);
    run(p1, r.clone(), &mut messages).await;

    let first_tool = messages
        .iter()
        .find_map(|m| match m {
            Message::Tool(ToolMessage { content, .. }) => Some(content),
            _ => None,
        })
        .expect("expected a tool message");
    assert!(first_tool.contains("count: 0"));

    messages.push(user("Change count to 1"));
    let p2 = ScriptProvider::new(vec![
        tool_call_response(&[(
            "edit_file",
            json!({
                "file_path": file.to_string_lossy(),
                "old_string": "count: 0",
                "new_string": "count: 1",
            }),
        )]),
        text_response("Done, count is now 1."),
    ]);
    run(p2, r, &mut messages).await;

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "count: 1");
    let tool_count = messages
        .iter()
        .filter(|m| matches!(m, Message::Tool(_)))
        .count();
    assert_eq!(tool_count, 2);
}

#[tokio::test]
async fn multi_tool_chain_across_three_turns() {
    let _sb = Sandbox::new("mt-chain");
    let dir = tempdir().unwrap();
    let file = dir.path().join("chain.txt");

    let mut r = ToolRegistry::new();
    r.register(create_write_tool()).unwrap();
    r.register(create_read_tool()).unwrap();
    r.register(create_edit_tool()).unwrap();

    let mut messages = vec![sys(), user("Create a file")];

    let p1 = ScriptProvider::new(vec![
        tool_call_response(&[(
            "write_file",
            json!({ "file_path": file.to_string_lossy(), "content": "hello world" }),
        )]),
        text_response("Created the file."),
    ]);
    run(p1, r.clone(), &mut messages).await;
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello world");

    messages.push(user("Read the file"));
    let p2 = ScriptProvider::new(vec![
        tool_call_response(&[("read_file", json!({ "file_path": file.to_string_lossy() }))]),
        text_response("It says: hello world"),
    ]);
    run(p2, r.clone(), &mut messages).await;

    messages.push(user("Change hello to goodbye"));
    let p3 = ScriptProvider::new(vec![
        tool_call_response(&[(
            "edit_file",
            json!({
                "file_path": file.to_string_lossy(),
                "old_string": "hello",
                "new_string": "goodbye",
            }),
        )]),
        text_response("Changed hello to goodbye."),
    ]);
    run(p3, r, &mut messages).await;
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "goodbye world");
}

#[tokio::test]
async fn no_duplicate_messages_across_turns() {
    let mut messages = vec![sys(), user("Say hello")];
    let p1 = ScriptProvider::new(vec![text_response("Hello!")]);
    run(p1, echo_registry(), &mut messages).await;

    messages.push(user("Say goodbye"));
    let p2 = ScriptProvider::new(vec![text_response("Goodbye!")]);
    run(p2, echo_registry(), &mut messages).await;

    let serialized: Vec<String> = messages
        .iter()
        .map(|m| serde_json::to_string(m).unwrap())
        .collect();
    let unique: std::collections::HashSet<&String> = serialized.iter().collect();
    assert_eq!(serialized.len(), unique.len());
}

#[tokio::test]
async fn session_round_trip_write_and_reload() {
    let _sb = Sandbox::new("mt-session-rt");
    let dir = tempdir().unwrap();
    let mut messages = vec![sys(), user("Echo ping")];

    let p1 = ScriptProvider::new(vec![
        tool_call_response(&[("echo", json!({ "text": "ping" }))]),
        text_response("Got: ping"),
    ]);
    run(p1, echo_registry(), &mut messages).await;

    messages.push(user("Thanks"));
    let p2 = ScriptProvider::new(vec![text_response("You're welcome!")]);
    run(p2, echo_registry(), &mut messages).await;

    let session_path = dir.path().join("session.jsonl");
    write_session_meta(
        &session_path,
        &SessionMeta {
            kind: "session_meta".to_string(),
            id: "test-session-001".to_string(),
            cwd: dir.path().to_string_lossy().to_string(),
            model: "test-model".to_string(),
            created: "2026-01-01T00:00:00Z".to_string(),
            heddle_version: "0.0.1-test".to_string(),
            name: None,
            forked_from: None,
            extra: Default::default(),
        },
    )
    .unwrap();
    for m in &messages {
        append_message(&session_path, m).unwrap();
    }

    let mut loaded = load_session(&session_path);
    assert_eq!(loaded.len(), messages.len());

    fn role(m: &Message) -> &'static str {
        match m {
            Message::System(_) => "system",
            Message::User(_) => "user",
            Message::Assistant(_) => "assistant",
            Message::Tool(_) => "tool",
        }
    }
    let original_roles: Vec<&str> = messages.iter().map(role).collect();
    let loaded_roles: Vec<&str> = loaded.iter().map(role).collect();
    assert_eq!(loaded_roles, original_roles);

    // Run another turn on loaded messages
    loaded.push(user("Are you still there?"));
    let p3 = ScriptProvider::new(vec![text_response("Continuing from loaded session.")]);
    run(p3, echo_registry(), &mut loaded).await;
    assert_eq!(loaded.len(), messages.len() + 2);
    assert!(matches!(loaded.last().unwrap(), Message::Assistant(_)));
}
