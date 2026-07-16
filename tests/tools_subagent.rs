use async_trait::async_trait;
use heddle::cost::tracker::CostTracker;
use heddle::provider::types::{ChunkStream, Provider};
use heddle::tools::registry::ToolRegistry;
use heddle::tools::subagent::{create_subagent_tool, SubagentOptions};
use heddle::tools::types::{ExecOptions, HeddleTool};
use heddle::types::{
    ChatCompletionResponse, Choice, ChoiceMessage, Message, ToolDefinition, Usage,
};
use parking_lot::Mutex as PlMutex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

mod common;
use common::mocks::{text_response, tool_call_response};

// ─── Providers ──────────────────────────────────────────────────────────

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
    captured_tools: Mutex<Option<Vec<ToolDefinition>>>,
    captured_messages: Mutex<Option<Vec<Message>>>,
    response: ChatCompletionResponse,
}
impl ToolCaptureProvider {
    fn new(response: ChatCompletionResponse) -> Arc<Self> {
        Arc::new(Self {
            captured_tools: Mutex::new(None),
            captured_messages: Mutex::new(None),
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
        *self.captured_tools.lock().unwrap() = tools.map(|t| t.to_vec());
        *self.captured_messages.lock().unwrap() = Some(messages.to_vec());
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
        Err(anyhow::anyhow!("API connection failed"))
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

/// Provider that counts calls and always returns a tool call (would loop forever).
struct LoopingProvider {
    calls: AtomicUsize,
}
impl LoopingProvider {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
        })
    }
}
#[async_trait]
impl Provider for LoopingProvider {
    async fn send(
        &self,
        _messages: &[Message],
        _tools: Option<&[ToolDefinition]>,
        _overrides: &Value,
    ) -> anyhow::Result<ChatCompletionResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(tool_call_response(&[("echo", json!({ "text": "loop" }))]))
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

// ─── Tools ──────────────────────────────────────────────────────────────

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

struct UpperTool;
#[async_trait]
impl HeddleTool for UpperTool {
    fn name(&self) -> &str {
        "uppercase"
    }
    fn description(&self) -> &str {
        "Uppercases the input string"
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
            .to_uppercase()
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn returns_a_tool_with_correct_name_and_schema() {
    let p = ScriptProvider::new(vec![]);
    let r = ToolRegistry::new();
    let tool = create_subagent_tool(p, r, SubagentOptions::default());
    assert_eq!(tool.name(), "subagent");
    assert!(!tool.description().is_empty());
    let schema = tool.parameters();
    assert!(schema.get("properties").is_some());
}

#[tokio::test]
async fn runs_simple_prompt_and_returns_assistant_response() {
    let p = ScriptProvider::new(vec![text_response("The answer is 42.")]);
    let r = ToolRegistry::new();
    let tool = create_subagent_tool(p, r, SubagentOptions::default());
    let result = tool
        .execute(
            json!({ "prompt": "What is the meaning of life?" }),
            ExecOptions::default(),
        )
        .await;
    assert_eq!(result, "The answer is 42.");
}

#[tokio::test]
async fn subagent_can_use_tools_from_registry() {
    let p = ScriptProvider::new(vec![
        tool_call_response(&[("echo", json!({ "text": "hello" }))]),
        text_response("Echo returned: hello"),
    ]);
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    let tool = create_subagent_tool(p, r, SubagentOptions::default());
    let result = tool
        .execute(
            json!({ "prompt": "Use echo to say hello" }),
            ExecOptions::default(),
        )
        .await;
    assert_eq!(result, "Echo returned: hello");
}

#[tokio::test]
async fn filters_tools_when_tools_param_provided() {
    let p = ToolCaptureProvider::new(text_response("Done"));
    let p2 = p.clone();
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    r.register(Arc::new(UpperTool)).unwrap();
    let tool = create_subagent_tool(p, r, SubagentOptions::default());
    tool.execute(
        json!({ "prompt": "Use echo", "tools": ["echo"] }),
        ExecOptions::default(),
    )
    .await;
    let captured = p2.captured_tools.lock().unwrap().clone().expect("tools");
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].function.name, "echo");
}

#[tokio::test]
async fn returns_error_string_when_loop_has_no_content() {
    let empty = ChatCompletionResponse {
        model: None,
        id: "x".to_string(),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                content: None,
                tool_calls: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 0,
            total_tokens: 10,
            ..Default::default()
        }),
    };
    let p = ScriptProvider::new(vec![empty]);
    let r = ToolRegistry::new();
    let tool = create_subagent_tool(p, r, SubagentOptions::default());
    let result = tool
        .execute(json!({ "prompt": "Do something" }), ExecOptions::default())
        .await;
    assert!(result.contains("Error"), "got: {result}");
}

#[tokio::test]
async fn returns_error_string_when_provider_fails() {
    let p = Arc::new(FailingProvider);
    let r = ToolRegistry::new();
    let tool = create_subagent_tool(p, r, SubagentOptions::default());
    let result = tool
        .execute(json!({ "prompt": "Do something" }), ExecOptions::default())
        .await;
    assert!(result.contains("Error"), "got: {result}");
    assert!(result.contains("API connection failed"), "got: {result}");
}

#[tokio::test]
async fn respects_max_iterations_option() {
    let p = LoopingProvider::new();
    let p2 = p.clone();
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    let tool = create_subagent_tool(
        p,
        r,
        SubagentOptions {
            max_iterations: Some(2),
            ..Default::default()
        },
    );
    tool.execute(json!({ "prompt": "Loop forever" }), ExecOptions::default())
        .await;
    assert!(p2.calls.load(Ordering::SeqCst) <= 2);
}

#[tokio::test]
async fn accumulates_usage_into_cost_tracker() {
    let cost = Arc::new(PlMutex::new(CostTracker::new()));
    let p = ScriptProvider::new(vec![text_response("Done")]);
    let r = ToolRegistry::new();
    let tool = create_subagent_tool(
        p,
        r,
        SubagentOptions {
            cost_tracker: Some(cost.clone()),
            ..Default::default()
        },
    );
    tool.execute(json!({ "prompt": "Quick task" }), ExecOptions::default())
        .await;
    let ct = cost.lock();
    assert_eq!(ct.total_input_tokens(), 10);
    assert_eq!(ct.total_output_tokens(), 5);
}

#[tokio::test]
async fn subagent_messages_are_isolated_from_parent_context() {
    let p = ToolCaptureProvider::new(text_response("Isolated response"));
    let p2 = p.clone();
    let r = ToolRegistry::new();
    let tool = create_subagent_tool(p, r, SubagentOptions::default());
    tool.execute(json!({ "prompt": "Do a task" }), ExecOptions::default())
        .await;
    let messages = p2
        .captured_messages
        .lock()
        .unwrap()
        .clone()
        .expect("messages captured");
    assert_eq!(messages.len(), 2);
    assert!(matches!(messages[0], Message::System(_)));
    if let Message::User(u) = &messages[1] {
        assert_eq!(u.content, "Do a task");
    } else {
        panic!("expected user message at position 1");
    }
}

// ─── ToolRegistry::subset (covered here per TS test layout) ────────────

#[tokio::test]
async fn subset_returns_new_registry_with_only_named_tools() {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    r.register(Arc::new(UpperTool)).unwrap();
    let sub = r.subset(&["echo".to_string()]);
    assert_eq!(sub.all().len(), 1);
    assert!(sub.get("echo").is_some());
    assert!(sub.get("uppercase").is_none());
}

#[tokio::test]
async fn subset_ignores_nonexistent_names() {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    let sub = r.subset(&["echo".to_string(), "nonexistent".to_string()]);
    assert_eq!(sub.all().len(), 1);
    assert!(sub.get("echo").is_some());
}

#[tokio::test]
async fn subset_returns_empty_when_no_matches() {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    let sub = r.subset(&["nonexistent".to_string()]);
    assert_eq!(sub.all().len(), 0);
}

#[tokio::test]
async fn subset_is_independent_from_parent() {
    let mut r = ToolRegistry::new();
    r.register(Arc::new(EchoTool)).unwrap();
    let sub = r.subset(&["echo".to_string()]);
    r.register(Arc::new(UpperTool)).unwrap();
    assert_eq!(sub.all().len(), 1);
    assert_eq!(r.all().len(), 2);
}
