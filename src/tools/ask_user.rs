//! ask_user tool — defers to a callback supplied by the host.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::types::{ExecOptions, HeddleTool};

pub type AskCallback = Arc<
    dyn Fn(String, Option<Vec<String>>) -> Pin<Box<dyn Future<Output = String> + Send>>
        + Send
        + Sync,
>;

pub struct AskUserTool {
    callback: AskCallback,
}

pub fn create_ask_user_tool(callback: AskCallback) -> Arc<dyn HeddleTool> {
    Arc::new(AskUserTool { callback })
}

#[async_trait]
impl HeddleTool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }
    fn description(&self) -> &str {
        "Ask the user a question. Optionally provide a list of choices."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": { "type": "string", "description": "The question to ask the user" },
                "options":  { "type": "array", "items": { "type": "string" }, "description": "Optional list of choices" }
            },
            "required": ["question"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let question = match params.get("question").and_then(Value::as_str) {
            Some(q) => q.to_string(),
            None => return "Error: missing question".to_string(),
        };
        let options: Option<Vec<String>> =
            params.get("options").and_then(Value::as_array).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });
        (self.callback)(question, options).await
    }
}
