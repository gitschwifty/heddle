//! subagent tool — recursively runs the agent loop with an isolated context.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use uuid::Uuid;

use super::registry::ToolRegistry;
use super::types::{ExecOptions, HeddleTool};
use crate::agent::loop_::{run_agent_loop, AgentLoopOptions};
use crate::agent::types::AgentEvent;
use crate::cost::tracker::CostTracker;
use crate::hooks::runner::HooksRunner;
use crate::permissions::checker::PermissionChecker;
use crate::provider::types::Provider;
use crate::session::jsonl::{append_context_marker, append_message};
use crate::types::{Message, SystemMessage, UserMessage};

#[derive(Clone, Default)]
pub struct SubagentOptions {
    pub permission_checker: Option<Arc<Mutex<PermissionChecker>>>,
    pub cost_tracker: Option<Arc<Mutex<CostTracker>>>,
    pub hooks_runner: Option<Arc<HooksRunner>>,
    pub max_iterations: Option<u32>,
    /// Directory where child transcripts for this parent session are stored.
    pub transcript_dir: Option<PathBuf>,
    /// Durable identifier of the parent session, used to correlate child logs.
    pub parent_session_id: Option<String>,
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
            Message::User(UserMessage {
                content: prompt.clone(),
            }),
        ];
        let transcript = self.create_transcript(&prompt, &messages);

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
            if let AgentEvent::Usage { usage, .. } = &event {
                if let Some(ct) = &self.options.cost_tracker {
                    ct.lock().add_usage(usage);
                }
            }
            if let Some(path) = &transcript {
                self.record_event(path, &event);
            }
            events.push(event);
        }
        drop(stream);

        if let Some(path) = &transcript {
            // This is the authoritative model-facing conversation. Event records
            // above preserve timing and permission details; this snapshot also
            // includes tool messages synthesized for denied or hook-blocked calls.
            let _ = append_context_marker(
                path,
                &json!({
                    "type": "subagent_complete",
                    "timestamp": Utc::now().to_rfc3339(),
                    "messages": messages,
                }),
            );
        }

        let last_assistant_content = events.iter().rev().find_map(|e| match e {
            AgentEvent::AssistantMessage { message, .. } => message.content.clone(),
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

impl SubagentTool {
    fn create_transcript(&self, prompt: &str, messages: &[Message]) -> Option<PathBuf> {
        let root = self.options.transcript_dir.as_ref()?;
        let id = Uuid::new_v4().to_string();
        let path = root.join(format!("{id}.jsonl"));
        let _ = append_context_marker(
            &path,
            &json!({
                "type": "subagent_transcript",
                "id": id,
                "parent_session_id": self.options.parent_session_id,
                "created": Utc::now().to_rfc3339(),
                "prompt": prompt,
            }),
        );
        for message in messages {
            let _ = append_message(&path, message);
        }
        Some(path)
    }

    fn record_event(&self, path: &std::path::Path, event: &AgentEvent) {
        let value = match event {
            AgentEvent::AssistantMessage {
                message,
                finish_reason,
            } => {
                let _ = append_message(path, &Message::Assistant(message.clone()));
                json!({ "type": "subagent_event", "event": "assistant_message", "finish_reason": finish_reason })
            }
            AgentEvent::ToolStart { name, call } => {
                json!({ "type": "subagent_event", "event": "tool_start", "name": name, "call": call })
            }
            AgentEvent::ToolEnd { name, result, call } => {
                json!({ "type": "subagent_event", "event": "tool_end", "name": name, "call": call, "result": result })
            }
            AgentEvent::Usage {
                usage,
                generation_id,
                timing,
            } => {
                json!({ "type": "subagent_event", "event": "usage", "usage": usage, "generation_id": generation_id, "timing": timing })
            }
            AgentEvent::RoutedModel { model } => {
                json!({ "type": "subagent_event", "event": "routed_model", "model": model })
            }
            AgentEvent::UpstreamProvider { provider } => {
                json!({ "type": "subagent_event", "event": "upstream_provider", "provider": provider })
            }
            AgentEvent::LoopDetected { count } => {
                json!({ "type": "subagent_event", "event": "loop_detected", "count": count })
            }
            AgentEvent::Error { message } => {
                json!({ "type": "subagent_event", "event": "error", "message": message })
            }
            AgentEvent::ProviderError {
                message, telemetry, ..
            } => {
                json!({ "type": "subagent_event", "event": "provider_error", "message": message, "telemetry": telemetry })
            }
            AgentEvent::PermissionRequest { name, call, reason } => {
                json!({ "type": "subagent_event", "event": "permission_request", "name": name, "call": call, "reason": reason })
            }
            AgentEvent::PermissionDenied { name, call, reason } => {
                json!({ "type": "subagent_event", "event": "permission_denied", "name": name, "call": call, "reason": reason })
            }
            AgentEvent::ContentDelta { text } => {
                json!({ "type": "subagent_event", "event": "content_delta", "text": text })
            }
            AgentEvent::PlanComplete { plan } => {
                json!({ "type": "subagent_event", "event": "plan_complete", "plan": plan })
            }
            AgentEvent::ContextPrune {
                messages_pruned,
                tokens_before,
                tokens_after,
            } => {
                json!({ "type": "subagent_event", "event": "context_prune", "messages_pruned": messages_pruned, "tokens_before": tokens_before, "tokens_after": tokens_after })
            }
            AgentEvent::ContextCompact => {
                json!({ "type": "subagent_event", "event": "context_compact" })
            }
            AgentEvent::ContextHandoff => {
                json!({ "type": "subagent_event", "event": "context_handoff" })
            }
        };
        let mut value = value;
        if let Value::Object(fields) = &mut value {
            fields.insert("timestamp".into(), Value::String(Utc::now().to_rfc3339()));
        }
        let _ = append_context_marker(path, &value);
    }
}
