//! JSON-over-stdio adapter for embedding heddle in other tools.
//!
//! Reads `IpcRequest`s line-by-line from stdin, processes them serially, and
//! writes `IpcResponse`s to stdout. Cancellation flips an `AbortToken` watched
//! by the in-flight agent loop.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tokio_util::sync::CancellationToken;

use crate::agent::loop_::{run_agent_loop_streaming, AgentLoopOptions};
use crate::agent::types::AgentEvent;
use crate::debug::set_headless;
use crate::hooks::loader::{load_hooks, merge_hooks_with_ipc};
use crate::hooks::runner::HooksRunner;
use crate::hooks::types::HookMode;
use crate::ipc::codec::{
    build_error, build_result, decode_request, encode_response, wrap_event, BuildResultArgs,
    CorrelationContext, DecodeResult,
};
use crate::ipc::errors::{normalize_error, ErrorEnvelope};
use crate::ipc::protocol::{check_compatibility, PROTOCOL_VERSION};
use crate::ipc::types::{
    InitConfig, IpcRequest, IpcResponse, ToolCallSummary, UsageSummary, WorkerEvent,
};
use crate::session::jsonl::append_message;
use crate::session::setup::{create_session, PermissionOverrides, SessionContext, SessionOptions};
use crate::tools::ask_user::create_ask_user_tool;
use crate::types::{Message, UserMessage};

struct State {
    session: Option<SessionContext>,
    correlation: CorrelationContext,
    active_id: Option<String>,
    cancel_target_id: Option<String>,
    active_cancel: Option<CancellationToken>,
    pending_cancel_ids: Vec<String>,
}

impl State {
    fn new() -> Self {
        Self {
            session: None,
            correlation: CorrelationContext::default(),
            active_id: None,
            cancel_target_id: None,
            active_cancel: None,
            pending_cancel_ids: Vec::new(),
        }
    }
}

fn write_line(resp: &IpcResponse) {
    println!("{}", encode_response(resp));
}

fn protocol_error(id: Option<&str>, message: impl Into<String>) -> IpcResponse {
    build_error(
        id,
        ErrorEnvelope {
            code: "protocol_error".to_string(),
            message: message.into(),
            retryable: false,
            details: None,
        },
        None,
    )
}

pub async fn run_headless() -> Result<()> {
    set_headless(true);

    let state: Arc<Mutex<State>> = Arc::new(Mutex::new(State::new()));
    let (tx, mut rx) = mpsc::unbounded_channel::<IpcRequest>();

    // stdin reader task
    let state_for_reader = state.clone();
    let reader = tokio::spawn(async move {
        let stdin = tokio::io::stdin();
        let mut lines = BufReader::new(stdin).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match decode_request(&line) {
                DecodeResult::Err(e) => write_line(&protocol_error(None, e)),
                DecodeResult::Ok(req) => {
                    // Cancel for active send → flip immediately.
                    // Cancel arriving before send dispatch → queue for the send to consume.
                    if let IpcRequest::Cancel { target_id, .. } = &req {
                        let mut s = state_for_reader.lock();
                        if s.active_id.as_deref() == Some(target_id) {
                            s.cancel_target_id = s.active_id.clone();
                            if let Some(tok) = &s.active_cancel {
                                tok.cancel();
                            }
                        } else {
                            s.pending_cancel_ids.push(target_id.clone());
                        }
                    }
                    let _ = tx.send(req);
                }
            }
        }
    });

    while let Some(request) = rx.recv().await {
        handle_request(&state, request).await;
    }
    let _ = reader.await;
    Ok(())
}

async fn handle_request(state: &Arc<Mutex<State>>, request: IpcRequest) {
    match request {
        IpcRequest::Init { .. } => handle_init(state, request).await,
        IpcRequest::Send { .. } => handle_send(state, request).await,
        IpcRequest::Status { id } => handle_status(state, id),
        IpcRequest::Shutdown { id } => {
            write_line(&IpcResponse::ShutdownOk { id });
            std::process::exit(0);
        }
        IpcRequest::Cancel { .. } => {
            // Cancel handled in reader; nothing more to do here.
        }
    }
}

async fn handle_init(state: &Arc<Mutex<State>>, request: IpcRequest) {
    let (id, protocol_version, config) = match request {
        IpcRequest::Init {
            id,
            protocol_version,
            config,
        } => (id, protocol_version, config),
        _ => unreachable!(),
    };

    if let Some(client_v) = &protocol_version {
        let compat = check_compatibility(client_v);
        if !compat.compatible {
            write_line(&build_result(
                &id,
                BuildResultArgs {
                    status: "error".into(),
                    error: Some(ErrorEnvelope {
                        code: "protocol_version_mismatch".into(),
                        message: "protocol_version_mismatch".into(),
                        retryable: false,
                        details: None,
                    }),
                    ..Default::default()
                },
            ));
            std::process::exit(1);
        }
    }

    let session_opts = build_session_options(&config);
    let session = match create_session(session_opts).await {
        Ok(s) => s,
        Err(e) => {
            write_line(&protocol_error(Some(&id), e.to_string()));
            return;
        }
    };

    let session = wire_ipc_overrides(session, &config);

    let session_id = session.session_id.clone();
    {
        let mut s = state.lock();
        s.correlation = CorrelationContext {
            session_id: Some(session_id.clone()),
            task_id: config.task_id.clone(),
            worker_id: config.worker_id.clone(),
        };
        s.session = Some(session);
    }

    write_line(&IpcResponse::InitOk {
        id,
        session_id,
        protocol_version: PROTOCOL_VERSION.clone(),
        error: None,
    });
}

fn build_session_options(config: &InitConfig) -> SessionOptions {
    SessionOptions {
        model: Some(config.model.clone()),
        system_prompt: Some(config.system_prompt.clone()),
        tools: Some(config.tools.clone()),
        permission_overrides: config.permissions.as_ref().map(|p| PermissionOverrides {
            allow: p.allow.clone(),
            deny: p.deny.clone(),
            ask: p.ask.clone(),
        }),
        ..Default::default()
    }
}

fn wire_ipc_overrides(mut session: SessionContext, config: &InitConfig) -> SessionContext {
    let _ = session
        .registry
        .register(create_ask_user_tool(Arc::new(|_question, _options| {
            Box::pin(async move { "User interaction not available in headless mode".to_string() })
        })));

    if session.features.hooks {
        let mut hooks = session.config.hooks.clone().unwrap_or_default();
        if let Some(ipc_hooks) = &config.hooks {
            let raw = serde_json::json!({ "hooks": ipc_hooks });
            // Convert from JSON to TOML Value via roundtrip
            if let Ok(tv) = toml::Value::try_from(raw) {
                let parsed = load_hooks(&tv, &toml::Value::Table(Default::default()));
                hooks = merge_hooks_with_ipc(hooks, parsed);
            }
        }
        if !hooks.is_empty() {
            let project = std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            session.hooks_runner = Some(Arc::new(HooksRunner::new(
                hooks,
                HookMode::Headless,
                session.session_id.clone(),
                project,
                session.config.model.clone(),
            )));
        }
    }
    session
}

async fn handle_send(state: &Arc<Mutex<State>>, request: IpcRequest) {
    let (id, message) = match request {
        IpcRequest::Send { id, message } => (id, message),
        _ => unreachable!(),
    };

    let (mut session, correlation) = {
        let mut s = state.lock();
        if s.session.is_none() {
            write_line(&protocol_error(
                Some(&id),
                "Not initialized. Send 'init' first.",
            ));
            return;
        }
        if s.active_id.is_some() {
            write_line(&protocol_error(Some(&id), "A send is already in progress."));
            return;
        }
        s.active_id = Some(id.clone());
        s.cancel_target_id = None;
        let cancel = CancellationToken::new();
        s.active_cancel = Some(cancel.clone());
        // If a cancel for this send arrived before dispatch, honor it now.
        if let Some(pos) = s.pending_cancel_ids.iter().position(|p| p == &id) {
            s.pending_cancel_ids.remove(pos);
            s.cancel_target_id = Some(id.clone());
            cancel.cancel();
        }
        let session = s.session.take().unwrap();
        let correlation = s.correlation.clone();
        (session, correlation)
    };

    let cancel = state.lock().active_cancel.clone().unwrap_or_default();

    let mut event_seq: u64 = 0;
    let user_msg = Message::User(UserMessage {
        content: message.clone(),
    });
    session.messages.push(user_msg.clone());
    let _ = append_message(&session.session_file, &user_msg);

    let mut tool_calls_made: Vec<ToolCallSummary> = Vec::new();
    let mut iterations: u32 = 0;
    let mut response: Option<String> = None;
    let mut total_usage: Option<UsageSummary> = None;
    let mut saw_content_delta = false;
    let mut error_envelope: Option<ErrorEnvelope> = None;
    let start = Instant::now();
    let mut tool_latency_ms: u64 = 0;
    let mut tool_start: Option<Instant> = None;

    // heartbeat
    let heartbeat_ms: u64 = std::env::var("HEDDLE_HEARTBEAT_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);
    let id_for_hb = id.clone();
    let correlation_for_hb = correlation.clone();
    let heartbeat_token = CancellationToken::new();
    let heartbeat_token_inner = heartbeat_token.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut tick = interval(Duration::from_millis(heartbeat_ms));
        // `tokio::time::interval` fires immediately at t=0; consume that tick so the
        // first heartbeat actually waits for the interval (matching the TS setInterval).
        tick.tick().await;
        let mut local_seq: u64 = 0;
        let started = Instant::now();
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    write_line(&wrap_event(
                        WorkerEvent::Heartbeat { duration_ms: started.elapsed().as_millis() as u64 },
                        &id_for_hb,
                        local_seq,
                        Some(&correlation_for_hb),
                    ));
                    local_seq += 1;
                }
                _ = heartbeat_token_inner.cancelled() => break,
            }
        }
    });

    let loop_opts = AgentLoopOptions {
        permission_checker: session.permission_checker.clone(),
        hooks_runner: session.hooks_runner.clone(),
        signal: Some(cancel.clone()),
        ..AgentLoopOptions::default()
    };

    let provider = session.provider.clone();
    let registry = session.registry.clone();
    let mut messages = std::mem::take(&mut session.messages);
    let mut stream = run_agent_loop_streaming(provider, registry, &mut messages, loop_opts);

    while let Some(event) = stream.next().await {
        if cancel.is_cancelled() {
            heartbeat_token.cancel();
            let _ = heartbeat_handle.await;
            let total = start.elapsed().as_millis() as u64;
            write_line(&build_result(
                &id,
                BuildResultArgs {
                    status: "error".into(),
                    error: Some(ErrorEnvelope {
                        code: "cancelled".into(),
                        message: "cancelled".into(),
                        retryable: false,
                        details: None,
                    }),
                    tool_calls_made,
                    iterations,
                    correlation: Some(correlation.clone()),
                    total_latency_ms: Some(total),
                    tool_latency_ms: Some(tool_latency_ms),
                    model_latency_ms: Some(total.saturating_sub(tool_latency_ms)),
                    ..Default::default()
                },
            ));
            drop(stream);
            session.messages = messages;
            return_session(state, session);
            return;
        }
        if let Some(mapped) = map_agent_event(&event) {
            write_line(&wrap_event(mapped, &id, event_seq, Some(&correlation)));
            event_seq += 1;
        }
        match event {
            AgentEvent::ContentDelta { .. } => saw_content_delta = true,
            AgentEvent::ToolStart { name, call } => {
                let args: Value =
                    serde_json::from_str(&call.function.arguments).unwrap_or(Value::Null);
                tool_calls_made.push(ToolCallSummary { name, args });
                tool_start = Some(Instant::now());
            }
            AgentEvent::ToolEnd { .. } => {
                if let Some(t) = tool_start.take() {
                    tool_latency_ms += t.elapsed().as_millis() as u64;
                }
            }
            AgentEvent::AssistantMessage { message } => {
                iterations += 1;
                if !saw_content_delta {
                    if let Some(c) = message.content {
                        response = Some(c);
                    }
                }
            }
            AgentEvent::Usage { usage } => {
                total_usage = Some(UsageSummary {
                    prompt_tokens: usage.prompt_tokens,
                    completion_tokens: usage.completion_tokens,
                    total_tokens: usage.total_tokens,
                });
            }
            AgentEvent::LoopDetected { count } => {
                error_envelope = Some(ErrorEnvelope {
                    code: "loop_detected".into(),
                    message: format!("Doom loop detected: {count} iterations"),
                    retryable: false,
                    details: None,
                });
            }
            AgentEvent::Error { message } => {
                let n = normalize_error(&message, "provider_error");
                error_envelope = Some(ErrorEnvelope {
                    code: n.code,
                    message: n.message,
                    retryable: n.retryable,
                    details: n.details,
                });
            }
            _ => {}
        }
    }
    drop(stream);
    session.messages = messages;
    heartbeat_token.cancel();
    let _ = heartbeat_handle.await;

    if cancel.is_cancelled() {
        let total = start.elapsed().as_millis() as u64;
        write_line(&build_result(
            &id,
            BuildResultArgs {
                status: "error".into(),
                error: Some(ErrorEnvelope {
                    code: "cancelled".into(),
                    message: "cancelled".into(),
                    retryable: false,
                    details: None,
                }),
                tool_calls_made,
                iterations,
                correlation: Some(correlation.clone()),
                total_latency_ms: Some(total),
                tool_latency_ms: Some(tool_latency_ms),
                model_latency_ms: Some(total.saturating_sub(tool_latency_ms)),
                ..Default::default()
            },
        ));
        return_session(state, session);
        return;
    }

    let total = start.elapsed().as_millis() as u64;
    let result_args = BuildResultArgs {
        status: if error_envelope.is_some() {
            "error".into()
        } else {
            "ok".into()
        },
        response: if error_envelope.is_some() {
            None
        } else {
            response.or_else(|| {
                if saw_content_delta {
                    session
                        .messages
                        .last()
                        .and_then(|m| m.content_str().map(String::from))
                } else {
                    None
                }
            })
        },
        tool_calls_made,
        usage: total_usage,
        iterations,
        error: error_envelope,
        correlation: Some(correlation.clone()),
        total_latency_ms: Some(total),
        tool_latency_ms: Some(tool_latency_ms),
        model_latency_ms: Some(total.saturating_sub(tool_latency_ms)),
    };
    write_line(&build_result(&id, result_args));

    // Persist new messages
    let persist_from = session
        .messages
        .iter()
        .position(|m| matches!(m, Message::User(u) if u.content == message))
        .map(|i| i + 1)
        .unwrap_or(session.messages.len());
    for msg in session.messages.iter().skip(persist_from) {
        let _ = append_message(&session.session_file, msg);
    }
    return_session(state, session);
}

fn return_session(state: &Arc<Mutex<State>>, session: SessionContext) {
    let mut s = state.lock();
    s.session = Some(session);
    s.active_cancel = None;
    s.active_id = None;
}

fn handle_status(state: &Arc<Mutex<State>>, id: String) {
    let s = state.lock();
    let session = match &s.session {
        Some(s) => s,
        None => {
            write_line(&protocol_error(
                Some(&id),
                "Not initialized. Send 'init' first.",
            ));
            return;
        }
    };
    write_line(&IpcResponse::StatusOk {
        id,
        model: session.config.model.clone(),
        messages_count: session.messages.len() as u64,
        session_id: session.session_id.clone(),
        active: s.active_id.is_some(),
    });
}

fn map_agent_event(event: &AgentEvent) -> Option<WorkerEvent> {
    match event {
        AgentEvent::ContentDelta { text } => Some(WorkerEvent::ContentDelta { text: text.clone() }),
        AgentEvent::ToolStart { name, call } => {
            let args: Value = serde_json::from_str(&call.function.arguments).unwrap_or(Value::Null);
            Some(WorkerEvent::ToolStart {
                name: name.clone(),
                args,
            })
        }
        AgentEvent::ToolEnd { name, result, .. } => Some(WorkerEvent::ToolEnd {
            name: name.clone(),
            result_preview: result.chars().take(500).collect(),
        }),
        AgentEvent::Usage { usage } => Some(WorkerEvent::Usage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        }),
        AgentEvent::LoopDetected { count } => Some(WorkerEvent::Error {
            code: "loop_detected".into(),
            message: format!("Doom loop detected: {count} iterations"),
            retryable: false,
            provider: None,
            details: None,
        }),
        AgentEvent::Error { message } => {
            let n = normalize_error(message, "provider_error");
            Some(WorkerEvent::Error {
                code: n.code,
                message: n.message,
                retryable: n.retryable,
                provider: n.provider,
                details: n.details,
            })
        }
        AgentEvent::PermissionDenied { name, reason, .. } => Some(WorkerEvent::PermissionDenied {
            name: name.clone(),
            reason: reason.clone(),
        }),
        AgentEvent::PlanComplete { plan } => Some(WorkerEvent::PlanComplete { plan: plan.clone() }),
        AgentEvent::ContextPrune {
            messages_pruned,
            tokens_before,
            tokens_after,
        } => Some(WorkerEvent::ContextPrune {
            messages_pruned: *messages_pruned,
            tokens_before: *tokens_before,
            tokens_after: *tokens_after,
        }),
        _ => None,
    }
}
