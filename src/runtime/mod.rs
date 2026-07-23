//! Runtime facade shared by frontends and protocol adapters.
//!
//! This layer owns session turn lifecycle and emits semantic runtime events.
//! UI and IPC adapters decide how those events are rendered or serialized.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use futures::StreamExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::agent::loop_::{
    run_agent_loop_streaming, AgentLoopOptions, PermissionResolver, PermissionResponse,
};
use crate::agent::types::AgentEvent;
use crate::ipc::errors::normalize_error;
use crate::session::jsonl::{append_context_marker, append_message, CONTEXT_RESET_MARKER_TYPE};
use crate::session::setup::{create_session, fresh_system_message, SessionContext, SessionOptions};
use crate::types::{AssistantMessage, Message, ToolCall, Usage, UserMessage};

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub session: SessionOptions,
}

#[derive(Debug, Clone)]
pub struct RuntimeStatus {
    pub session_id: String,
    pub model: String,
    pub last_routed_model: Option<String>,
    pub messages_count: u64,
    pub active: bool,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RuntimeUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: Option<u64>,
    pub cost_currency: Option<String>,
    pub cached_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub generation_id: Option<String>,
}

impl From<Usage> for RuntimeUsage {
    fn from(value: Usage) -> Self {
        let prompt_tokens_details = value.prompt_tokens_details;
        let completion_tokens_details = value.completion_tokens_details;
        Self {
            prompt_tokens: value.prompt_tokens,
            completion_tokens: value.completion_tokens,
            total_tokens: value.total_tokens,
            cost_micros: value.cost.map(cost_usd_to_micros),
            cost_currency: value.cost.map(|_| "USD".to_string()),
            cached_tokens: prompt_tokens_details.as_ref().and_then(|d| d.cached_tokens),
            cache_write_tokens: prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cache_write_tokens),
            reasoning_tokens: completion_tokens_details
                .as_ref()
                .and_then(|d| d.reasoning_tokens),
            generation_id: None,
        }
    }
}

fn cost_usd_to_micros(cost: f64) -> u64 {
    if !cost.is_finite() || cost <= 0.0 {
        return 0;
    }
    (cost * 1_000_000.0).round() as u64
}

#[derive(Debug, Clone)]
pub struct RuntimeToolCall {
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub provider: Option<String>,
    pub details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnStatus {
    Ok,
    Error,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub status: TurnStatus,
    pub response: Option<String>,
    pub tool_calls_made: Vec<RuntimeToolCall>,
    pub usage: Option<RuntimeUsage>,
    pub iterations: u32,
    pub error: Option<RuntimeError>,
    pub model_latency_ms: u64,
    pub tool_latency_ms: u64,
    pub total_latency_ms: u64,
}

#[derive(Clone)]
pub struct TurnOptions {
    pub id: String,
    pub cancel: CancellationToken,
    pub permission_resolver: Option<RuntimePermissionResolver>,
}

#[derive(Debug, Clone)]
pub struct RuntimePermissionRequest {
    pub name: String,
    pub call: ToolCall,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePermissionResponse {
    Allow,
    Deny,
    Always,
}

pub type RuntimePermissionResolver = Arc<
    dyn Fn(
            RuntimePermissionRequest,
        ) -> Pin<Box<dyn Future<Output = RuntimePermissionResponse> + Send>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnState {
    Queued,
    Running,
    Cancelling,
    Completed,
}

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    TurnStateChanged {
        turn_id: String,
        state: TurnState,
    },
    ContentDelta {
        text: String,
    },
    AssistantMessage {
        message: AssistantMessage,
        finish_reason: Option<String>,
    },
    ToolStarted {
        name: String,
        call: ToolCall,
    },
    ToolFinished {
        name: String,
        result: String,
        call: ToolCall,
    },
    UsageUpdated {
        usage: RuntimeUsage,
    },
    RoutedModel {
        model: String,
    },
    Error {
        error: RuntimeError,
    },
    PermissionRequested {
        name: String,
        call: ToolCall,
        reason: Option<String>,
    },
    PermissionDenied {
        name: String,
        call: ToolCall,
        reason: String,
    },
    PlanCompleted {
        plan: String,
    },
    ContextPruned {
        messages_pruned: u64,
        tokens_before: u64,
        tokens_after: u64,
    },
    ContextCompacted,
    ContextHandoff,
}

pub struct HeddleRuntime {
    session: SessionContext,
    last_routed_model: Option<String>,
}

impl HeddleRuntime {
    pub async fn init(config: RuntimeConfig) -> Result<Self> {
        let session = create_session(config.session).await?;
        Ok(Self::from_session(session))
    }

    pub fn from_session(session: SessionContext) -> Self {
        Self {
            session,
            last_routed_model: None,
        }
    }

    pub fn into_session(self) -> SessionContext {
        self.session
    }

    pub fn session(&self) -> &SessionContext {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut SessionContext {
        &mut self.session
    }

    pub fn status(&self, active: bool) -> RuntimeStatus {
        let tracker = self.session.cost_tracker.lock();
        RuntimeStatus {
            session_id: self.session.session_id.clone(),
            model: self.session.config.model.clone(),
            last_routed_model: self.last_routed_model.clone(),
            messages_count: self
                .session
                .messages
                .iter()
                .filter(|message| matches!(message, Message::User(_) | Message::Assistant(_)))
                .count() as u64,
            active,
            total_input_tokens: tracker.total_input_tokens(),
            total_output_tokens: tracker.total_output_tokens(),
            cost_usd: tracker.total_cost(),
        }
    }

    pub fn clear_context(&mut self) -> Result<()> {
        let marker = serde_json::json!({
            "type": CONTEXT_RESET_MARKER_TYPE,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        append_context_marker(&self.session.session_file, &marker)?;
        let system_msg = fresh_system_message(&self.session)?;
        append_message(&self.session.session_file, &system_msg)?;
        self.session.messages = vec![system_msg];
        Ok(())
    }

    pub async fn send<F>(
        &mut self,
        message: String,
        options: TurnOptions,
        mut on_event: F,
    ) -> TurnOutcome
    where
        F: FnMut(RuntimeEvent),
    {
        on_event(RuntimeEvent::TurnStateChanged {
            turn_id: options.id.clone(),
            state: TurnState::Running,
        });

        let start = Instant::now();
        let persisted_through = self.session.messages.len();
        let user_msg = Message::User(UserMessage { content: message });
        self.session.messages.push(user_msg.clone());
        let _ = append_message(&self.session.session_file, &user_msg);

        let mut tool_calls_made: Vec<RuntimeToolCall> = Vec::new();
        let mut iterations: u32 = 0;
        let mut response: Option<String> = None;
        let mut total_usage: Option<RuntimeUsage> = None;
        let mut saw_content_delta = false;
        let mut error: Option<RuntimeError> = None;
        let mut tool_latency_ms: u64 = 0;
        let mut tool_start: Option<Instant> = None;

        let loop_opts = AgentLoopOptions {
            permission_checker: self.session.permission_checker.clone(),
            permission_resolver: options
                .permission_resolver
                .clone()
                .map(agent_permission_resolver),
            hooks_runner: self.session.hooks_runner.clone(),
            signal: Some(options.cancel.clone()),
            ..AgentLoopOptions::default()
        };

        let provider = self.session.provider.clone();
        let registry = self.session.registry.clone();
        let mut messages = std::mem::take(&mut self.session.messages);
        let mut stream = run_agent_loop_streaming(provider, registry, &mut messages, loop_opts);

        while let Some(event) = stream.next().await {
            if options.cancel.is_cancelled() {
                on_event(RuntimeEvent::TurnStateChanged {
                    turn_id: options.id.clone(),
                    state: TurnState::Cancelling,
                });
                drop(stream);
                self.session.messages = messages;
                return self.cancelled_outcome(start, tool_calls_made, iterations, tool_latency_ms);
            }

            if let Some(runtime_event) = map_agent_event(&event) {
                on_event(runtime_event);
            }

            match event {
                AgentEvent::ContentDelta { .. } => saw_content_delta = true,
                AgentEvent::ToolStart { name, call } => {
                    let args =
                        serde_json::from_str(&call.function.arguments).unwrap_or(Value::Null);
                    tool_calls_made.push(RuntimeToolCall { name, args });
                    tool_start = Some(Instant::now());
                }
                AgentEvent::ToolEnd { .. } => {
                    if let Some(t) = tool_start.take() {
                        tool_latency_ms += t.elapsed().as_millis() as u64;
                    }
                }
                AgentEvent::AssistantMessage { message, .. } => {
                    iterations += 1;
                    if !saw_content_delta {
                        if let Some(c) = message.content {
                            response = Some(c);
                        }
                    }
                }
                AgentEvent::Usage {
                    usage,
                    generation_id,
                } => {
                    self.session.cost_tracker.lock().add_usage(&usage);
                    let mut runtime_usage: RuntimeUsage = usage.into();
                    runtime_usage.generation_id = generation_id;
                    total_usage = Some(runtime_usage);
                }
                AgentEvent::RoutedModel { model } => {
                    self.last_routed_model = Some(model);
                }
                AgentEvent::LoopDetected { count } => {
                    error = Some(RuntimeError {
                        code: "loop_detected".into(),
                        message: format!("Doom loop detected: {count} iterations"),
                        retryable: false,
                        provider: None,
                        details: None,
                    });
                }
                AgentEvent::Error { message } => {
                    error = Some(runtime_error(
                        &message,
                        if message.starts_with("Max iterations (") {
                            "max_iterations"
                        } else {
                            "provider_error"
                        },
                    ));
                }
                _ => {}
            }
        }
        drop(stream);
        self.session.messages = messages;

        if options.cancel.is_cancelled() {
            on_event(RuntimeEvent::TurnStateChanged {
                turn_id: options.id.clone(),
                state: TurnState::Cancelling,
            });
            return self.cancelled_outcome(start, tool_calls_made, iterations, tool_latency_ms);
        }

        for msg in self.session.messages.iter().skip(persisted_through + 1) {
            let _ = append_message(&self.session.session_file, msg);
        }

        on_event(RuntimeEvent::TurnStateChanged {
            turn_id: options.id,
            state: TurnState::Completed,
        });

        let total_latency_ms = start.elapsed().as_millis() as u64;
        let status = if error.is_some() {
            TurnStatus::Error
        } else {
            TurnStatus::Ok
        };
        let response = if error.is_some() {
            None
        } else {
            response.or_else(|| {
                if saw_content_delta {
                    self.session
                        .messages
                        .last()
                        .and_then(|m| m.content_str().map(String::from))
                } else {
                    None
                }
            })
        };

        TurnOutcome {
            status,
            response,
            tool_calls_made,
            usage: total_usage,
            iterations,
            error,
            model_latency_ms: total_latency_ms.saturating_sub(tool_latency_ms),
            tool_latency_ms,
            total_latency_ms,
        }
    }

    fn cancelled_outcome(
        &self,
        start: Instant,
        tool_calls_made: Vec<RuntimeToolCall>,
        iterations: u32,
        tool_latency_ms: u64,
    ) -> TurnOutcome {
        let total_latency_ms = start.elapsed().as_millis() as u64;
        TurnOutcome {
            status: TurnStatus::Cancelled,
            response: None,
            tool_calls_made,
            usage: None,
            iterations,
            error: Some(RuntimeError {
                code: "cancelled".into(),
                message: "cancelled".into(),
                retryable: false,
                provider: None,
                details: None,
            }),
            model_latency_ms: total_latency_ms.saturating_sub(tool_latency_ms),
            tool_latency_ms,
            total_latency_ms,
        }
    }
}

fn map_agent_event(event: &AgentEvent) -> Option<RuntimeEvent> {
    match event {
        AgentEvent::ContentDelta { text } => {
            Some(RuntimeEvent::ContentDelta { text: text.clone() })
        }
        AgentEvent::AssistantMessage {
            message,
            finish_reason,
        } => Some(RuntimeEvent::AssistantMessage {
            message: message.clone(),
            finish_reason: finish_reason.clone(),
        }),
        AgentEvent::ToolStart { name, call } => Some(RuntimeEvent::ToolStarted {
            name: name.clone(),
            call: call.clone(),
        }),
        AgentEvent::ToolEnd { name, result, call } => Some(RuntimeEvent::ToolFinished {
            name: name.clone(),
            result: result.clone(),
            call: call.clone(),
        }),
        AgentEvent::Usage {
            usage,
            generation_id,
        } => {
            let mut runtime_usage: RuntimeUsage = usage.clone().into();
            runtime_usage.generation_id = generation_id.clone();
            Some(RuntimeEvent::UsageUpdated {
                usage: runtime_usage,
            })
        }
        AgentEvent::RoutedModel { model } => Some(RuntimeEvent::RoutedModel {
            model: model.clone(),
        }),
        AgentEvent::LoopDetected { count } => Some(RuntimeEvent::Error {
            error: RuntimeError {
                code: "loop_detected".into(),
                message: format!("Doom loop detected: {count} iterations"),
                retryable: false,
                provider: None,
                details: None,
            },
        }),
        AgentEvent::Error { message } => Some(RuntimeEvent::Error {
            error: runtime_error(
                message,
                if message.starts_with("Max iterations (") {
                    "max_iterations"
                } else {
                    "provider_error"
                },
            ),
        }),
        AgentEvent::PermissionRequest { name, call, reason } => {
            Some(RuntimeEvent::PermissionRequested {
                name: name.clone(),
                call: call.clone(),
                reason: reason.clone(),
            })
        }
        AgentEvent::PermissionDenied { name, call, reason } => {
            Some(RuntimeEvent::PermissionDenied {
                name: name.clone(),
                call: call.clone(),
                reason: reason.clone(),
            })
        }
        AgentEvent::PlanComplete { plan } => {
            Some(RuntimeEvent::PlanCompleted { plan: plan.clone() })
        }
        AgentEvent::ContextPrune {
            messages_pruned,
            tokens_before,
            tokens_after,
        } => Some(RuntimeEvent::ContextPruned {
            messages_pruned: *messages_pruned,
            tokens_before: *tokens_before,
            tokens_after: *tokens_after,
        }),
        AgentEvent::ContextCompact => Some(RuntimeEvent::ContextCompacted),
        AgentEvent::ContextHandoff => Some(RuntimeEvent::ContextHandoff),
    }
}

fn agent_permission_resolver(resolver: RuntimePermissionResolver) -> PermissionResolver {
    Arc::new(move |name, call, reason| {
        let resolver = resolver.clone();
        Box::pin(async move {
            match resolver(RuntimePermissionRequest { name, call, reason }).await {
                RuntimePermissionResponse::Allow => PermissionResponse::Allow,
                RuntimePermissionResponse::Deny => PermissionResponse::Deny,
                RuntimePermissionResponse::Always => PermissionResponse::Always,
            }
        })
    })
}

fn runtime_error(message: &str, fallback_code: &str) -> RuntimeError {
    let normalized = normalize_error(message, fallback_code);
    RuntimeError {
        code: normalized.code,
        message: normalized.message,
        retryable: normalized.retryable,
        provider: normalized.provider,
        details: normalized.details,
    }
}
