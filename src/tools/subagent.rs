//! subagent tool — recursively runs the agent loop with an isolated context.

use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json::{json, Value};

use super::registry::ToolRegistry;
use super::types::{ExecOptions, HeddleTool};
use crate::agent::loop_::{run_agent_loop, AgentLoopOptions};
use crate::agent::types::AgentEvent;
use crate::cost::tracker::CostTracker;
use crate::hooks::runner::HooksRunner;
use crate::permissions::checker::PermissionChecker;
use crate::provider::types::Provider;
use crate::types::{Message, SystemMessage, UserMessage};

#[derive(Clone, Default)]
pub struct SubagentOptions {
    pub permission_checker: Option<Arc<Mutex<PermissionChecker>>>,
    pub cost_tracker: Option<Arc<Mutex<CostTracker>>>,
    pub hooks_runner: Option<Arc<HooksRunner>>,
    pub max_iterations: Option<u32>,
}

pub struct SubagentTool {
    provider: Arc<dyn Provider>,
    registry: ToolRegistry,
    options: SubagentOptions,
}

pub fn create_subagent_tool(
    provider: Arc<dyn Provider>,
    registry: ToolRegistry,
    options: SubagentOptions,
) -> Arc<dyn HeddleTool> {
    Arc::new(SubagentTool {
        provider,
        registry,
        options,
    })
}

#[async_trait]
impl HeddleTool for SubagentTool {
    fn name(&self) -> &str {
        "subagent"
    }
    fn description(&self) -> &str {
        "Spawn a child agent with isolated context to perform a subtask. Returns the agent's final response."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": { "type": "string", "description": "The task for the subagent" },
                "tools":  { "type": "array", "items": { "type": "string" }, "description": "Filter to only these tools from the registry" }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, params: Value, exec_options: ExecOptions) -> String {
        let prompt = match params.get("prompt").and_then(Value::as_str) {
            Some(p) => p.to_string(),
            None => return "Error: missing prompt".to_string(),
        };
        let tool_filter: Option<Vec<String>> =
            params.get("tools").and_then(Value::as_array).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        let effective_registry = match tool_filter {
            Some(names) => self.registry.subset(&names),
            None => self.registry.clone(),
        };

        let messages = vec![
            Message::System(SystemMessage {
                content: "You are a subagent. Complete the given task using available tools. Be concise and focused.".to_string(),
            }),
            Message::User(UserMessage { content: prompt }),
        ];

        let loop_opts = AgentLoopOptions {
            max_iterations: self.options.max_iterations,
            permission_checker: self.options.permission_checker.clone(),
            hooks_runner: self.options.hooks_runner.clone(),
            signal: exec_options.signal.clone(),
            ..AgentLoopOptions::default()
        };

        let mut messages = messages;
        let mut stream = run_agent_loop(
            self.provider.clone(),
            effective_registry,
            &mut messages,
            loop_opts,
        );
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            if let AgentEvent::Usage { usage } = &event {
                if let Some(ct) = &self.options.cost_tracker {
                    ct.lock().add_usage(usage);
                }
            }
            events.push(event);
        }

        let last_assistant_content = events.iter().rev().find_map(|e| match e {
            AgentEvent::AssistantMessage { message } => message.content.clone(),
            _ => None,
        });
        if let Some(c) = last_assistant_content {
            return c;
        }
        let error = events.iter().find_map(|e| match e {
            AgentEvent::Error { message } => Some(message.clone()),
            _ => None,
        });
        if let Some(m) = error {
            return format!("Error: Subagent failed — {m}");
        }
        "Error: Subagent produced no response".to_string()
    }
}
