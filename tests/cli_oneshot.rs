use async_trait::async_trait;
use heddle::cli::oneshot::{
    format_oneshot_output, run_oneshot_with_context, OneshotOptions, OneshotResult,
};
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::{ChatCompletionResponse, Message, SystemMessage, ToolDefinition};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

mod common;
use common::mocks::{text_response, tool_call_response};

// ─── Scripted provider (FIFO) ────────────────────────────────────────────
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

struct ErrorProvider;
#[async_trait]
impl Provider for ErrorProvider {
    async fn send(
        &self,
        _m: &[Message],
        _t: Option<&[ToolDefinition]>,
        _o: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        Err(anyhow::anyhow!("Server exploded"))
    }
    fn stream(&self, _m: Vec<Message>, _t: Option<Vec<ToolDefinition>>, _o: Value) -> ChunkStream {
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
        json!({ "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] })
    }
    async fn execute(&self, p: Value, _o: ExecOptions) -> String {
        p.get("text")
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

fn sys_messages() -> Vec<Message> {
    vec![Message::System(SystemMessage {
        content: "You are a test assistant.".to_string(),
    })]
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn basic_prompt_returns_response_with_exit_code_zero() {
    let provider = ScriptProvider::new(vec![text_response("Hello from the model!")]);
    let mut messages = sys_messages();
    let r =
        run_oneshot_with_context("Say hello", provider, ToolRegistry::new(), &mut messages).await;
    assert_eq!(r.output, "Hello from the model!");
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.tool_calls, 0);
}

#[tokio::test]
async fn response_with_tool_calls_tracks_tool_call_count() {
    let provider = ScriptProvider::new(vec![
        tool_call_response(&[("echo", json!({ "text": "ping" }))]),
        text_response("Got: ping"),
    ]);
    let mut messages = sys_messages();
    let r = run_oneshot_with_context("Echo ping", provider, echo_registry(), &mut messages).await;
    assert_eq!(r.output, "Got: ping");
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.tool_calls, 1);
}

#[tokio::test]
async fn multiple_tool_calls_in_sequence_are_counted() {
    let provider = ScriptProvider::new(vec![
        tool_call_response(&[
            ("echo", json!({ "text": "one" })),
            ("echo", json!({ "text": "two" })),
        ]),
        text_response("Done with two calls"),
    ]);
    let mut messages = sys_messages();
    let r = run_oneshot_with_context("Echo both", provider, echo_registry(), &mut messages).await;
    assert_eq!(r.output, "Done with two calls");
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.tool_calls, 2);
}

#[tokio::test]
async fn provider_error_returns_exit_code_one() {
    let provider = Arc::new(ErrorProvider);
    let mut messages = sys_messages();
    let r = run_oneshot_with_context(
        "This will fail",
        provider,
        ToolRegistry::new(),
        &mut messages,
    )
    .await;
    assert_eq!(r.exit_code, 1);
    assert!(r.output.contains("Server exploded"), "got: {}", r.output);
}

#[tokio::test]
async fn empty_prompt_returns_error_with_exit_code_one() {
    let provider = ScriptProvider::new(vec![]);
    let mut messages = sys_messages();
    let r = run_oneshot_with_context("", provider, ToolRegistry::new(), &mut messages).await;
    assert_eq!(r.exit_code, 1);
    assert!(r.output.contains("No prompt provided"));
}

#[test]
fn format_json_mode_returns_json_with_all_fields() {
    let r = OneshotResult {
        output: "The answer is 42".into(),
        exit_code: 0,
        tool_calls: 2,
    };
    let opts = OneshotOptions {
        prompt: "test".into(),
        json: true,
        ..Default::default()
    };
    let formatted = format_oneshot_output(&r, &opts);
    let parsed: Value = serde_json::from_str(&formatted).unwrap();
    assert_eq!(parsed["output"], "The answer is 42");
    assert_eq!(parsed["exitCode"], 0);
    assert_eq!(parsed["toolCalls"], 2);
}

#[test]
fn format_default_mode_returns_output_text() {
    let r = OneshotResult {
        output: "The answer is 42".into(),
        exit_code: 0,
        tool_calls: 2,
    };
    let opts = OneshotOptions {
        prompt: "test".into(),
        ..Default::default()
    };
    let formatted = format_oneshot_output(&r, &opts);
    assert_eq!(formatted, "The answer is 42");
}

#[test]
fn format_json_mode_with_error_result() {
    let r = OneshotResult {
        output: "Something went wrong".into(),
        exit_code: 1,
        tool_calls: 0,
    };
    let opts = OneshotOptions {
        prompt: "test".into(),
        json: true,
        ..Default::default()
    };
    let formatted = format_oneshot_output(&r, &opts);
    let parsed: Value = serde_json::from_str(&formatted).unwrap();
    assert_eq!(parsed["exitCode"], 1);
    assert_eq!(parsed["toolCalls"], 0);
}
