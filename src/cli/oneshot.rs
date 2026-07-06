//! Non-interactive single-prompt execution (for `-p`).

use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use serde_json::json;

use crate::agent::loop_::{run_agent_loop, AgentLoopOptions};
use crate::agent::types::AgentEvent;
use crate::provider::types::Provider;
use crate::session::setup::{create_session, SessionOptions};
use crate::tools::registry::ToolRegistry;
use crate::types::{Message, UserMessage};

#[derive(Debug, Clone, Default)]
pub struct OneshotOptions {
    pub prompt: String,
    pub json: bool,
    pub quiet: bool,
    pub agent: Option<String>,
    pub session_options: Option<SessionOptions>,
}

#[derive(Debug, Clone, Default)]
pub struct OneshotResult {
    pub output: String,
    pub exit_code: i32,
    pub tool_calls: u32,
}

pub async fn run_oneshot_with_context(
    prompt: &str,
    provider: Arc<dyn Provider>,
    registry: ToolRegistry,
    messages: &mut Vec<Message>,
) -> OneshotResult {
    if prompt.is_empty() {
        return OneshotResult {
            output: "No prompt provided".to_string(),
            exit_code: 1,
            tool_calls: 0,
        };
    }
    messages.push(Message::User(UserMessage {
        content: prompt.to_string(),
    }));

    let mut output = String::new();
    let mut tool_calls = 0u32;
    let mut stream = run_agent_loop(provider, registry, messages, AgentLoopOptions::default());
    while let Some(event) = stream.next().await {
        match event {
            AgentEvent::AssistantMessage { message, .. } => {
                output = message.content.unwrap_or_default();
            }
            AgentEvent::ToolStart { .. } => tool_calls += 1,
            AgentEvent::Error { message } => {
                return OneshotResult {
                    output: message,
                    exit_code: 1,
                    tool_calls,
                }
            }
            _ => {}
        }
    }
    OneshotResult {
        output,
        exit_code: 0,
        tool_calls,
    }
}

pub async fn run_oneshot(options: OneshotOptions) -> OneshotResult {
    if options.prompt.is_empty() {
        return OneshotResult {
            output: "No prompt provided".to_string(),
            exit_code: 1,
            tool_calls: 0,
        };
    }
    let mut session_opts = options.session_options.clone().unwrap_or_default();
    if options.agent.is_some() {
        session_opts.agent = options.agent.clone();
    }
    let session = match create_session(session_opts).await {
        Ok(s) => s,
        Err(e) => {
            return OneshotResult {
                output: e.to_string(),
                exit_code: 1,
                tool_calls: 0,
            }
        }
    };
    let mut messages = session.messages;
    run_oneshot_with_context(
        &options.prompt,
        session.provider,
        session.registry,
        &mut messages,
    )
    .await
}

pub fn format_oneshot_output(result: &OneshotResult, options: &OneshotOptions) -> String {
    if options.json {
        return json!({
            "output": result.output,
            "exitCode": result.exit_code,
            "toolCalls": result.tool_calls,
        })
        .to_string();
    }
    result.output.clone()
}

#[allow(dead_code)]
fn _r() -> Result<()> {
    Ok(())
}
