//! JSON-over-stdio adapter for embedding heddle in other tools.
//!
//! Reads `IpcRequest`s line-by-line from stdin, processes them serially, and
//! writes `IpcResponse`s to stdout. Cancellation flips an `AbortToken` watched
//! by the in-flight agent loop.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use parking_lot::Mutex;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tokio_util::sync::CancellationToken;

use crate::config::features::Mode;
use crate::debug::set_headless;
use crate::hooks::loader::{load_hooks, merge_hooks_with_ipc};
use crate::hooks::runner::HooksRunner;
use crate::hooks::types::HookMode;
use crate::ipc::codec::{
    build_error, build_result, decode_request, encode_response, wrap_event, BuildResultArgs,
    CorrelationContext, DecodeResult,
};
use crate::ipc::errors::ErrorEnvelope;
use crate::ipc::protocol::{check_compatibility, PROTOCOL_VERSION};
use crate::ipc::types::{
    InitConfig, IpcRequest, IpcResponse, ToolCallSummary, UsageSummary, WorkerEvent,
};
use crate::runtime::{
    HeddleRuntime, RuntimeError, RuntimeEvent, RuntimeToolCall, RuntimeUsage, TurnOptions,
    TurnOutcome, TurnStatus,
};
use crate::session::setup::{create_session, PermissionOverrides, SessionContext, SessionOptions};
use crate::tools::ask_user::create_ask_user_tool;

struct State {
    runtime: Option<HeddleRuntime>,
    correlation: CorrelationContext,
    active_id: Option<String>,
    cancel_target_id: Option<String>,
    active_cancel: Option<CancellationToken>,
    pending_cancel_ids: Vec<String>,
}

impl State {
    fn new() -> Self {
        Self {
            runtime: None,
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
    let runtime = HeddleRuntime::from_session(session);
    {
        let mut s = state.lock();
        s.correlation = CorrelationContext {
            session_id: Some(session_id.clone()),
            task_id: config.task_id.clone(),
            worker_id: config.worker_id.clone(),
        };
        s.runtime = Some(runtime);
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
        mode: Some(Mode::Headless),
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

    let (mut runtime, correlation) = {
        let mut s = state.lock();
        if s.runtime.is_none() {
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
        let runtime = s.runtime.take().unwrap();
        let correlation = s.correlation.clone();
        (runtime, correlation)
    };

    let cancel = state.lock().active_cancel.clone().unwrap_or_default();

    let event_seq = Arc::new(Mutex::new(0_u64));
    let heartbeat_ms: u64 = std::env::var("HEDDLE_HEARTBEAT_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);
    let id_for_hb = id.clone();
    let correlation_for_hb = correlation.clone();
    let event_seq_for_hb = event_seq.clone();
    let heartbeat_token = CancellationToken::new();
    let heartbeat_token_inner = heartbeat_token.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut tick = interval(Duration::from_millis(heartbeat_ms));
        // `tokio::time::interval` fires immediately at t=0; consume that tick so the
        // first heartbeat actually waits for the interval (matching the TS setInterval).
        tick.tick().await;
        let started = Instant::now();
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let mut seq = event_seq_for_hb.lock();
                    write_line(&wrap_event(
                        WorkerEvent::Heartbeat { duration_ms: started.elapsed().as_millis() as u64 },
                        &id_for_hb,
                        *seq,
                        Some(&correlation_for_hb),
                    ));
                    *seq += 1;
                }
                _ = heartbeat_token_inner.cancelled() => break,
            }
        }
    });

    let id_for_events = id.clone();
    let correlation_for_events = correlation.clone();
    let event_seq_for_events = event_seq.clone();
    let outcome = runtime
        .send(
            message,
            TurnOptions {
                id: id.clone(),
                cancel,
                permission_resolver: None,
            },
            |event| {
                if let Some(mapped) = map_runtime_event(&event) {
                    let mut seq = event_seq_for_events.lock();
                    write_line(&wrap_event(
                        mapped,
                        &id_for_events,
                        *seq,
                        Some(&correlation_for_events),
                    ));
                    *seq += 1;
                }
            },
        )
        .await;

    heartbeat_token.cancel();
    let _ = heartbeat_handle.await;

    write_line(&build_result(&id, build_result_args(outcome, correlation)));
    return_runtime(state, runtime);
}

fn return_runtime(state: &Arc<Mutex<State>>, runtime: HeddleRuntime) {
    let mut s = state.lock();
    s.runtime = Some(runtime);
    s.active_cancel = None;
    s.active_id = None;
}

fn handle_status(state: &Arc<Mutex<State>>, id: String) {
    let s = state.lock();
    let runtime = match &s.runtime {
        Some(runtime) => runtime,
        None => {
            write_line(&protocol_error(
                Some(&id),
                "Not initialized. Send 'init' first.",
            ));
            return;
        }
    };
    let status = runtime.status(s.active_id.is_some());
    write_line(&IpcResponse::StatusOk {
        id,
        model: status.model,
        messages_count: status.messages_count,
        session_id: status.session_id,
        active: status.active,
    });
}

fn build_result_args(outcome: TurnOutcome, correlation: CorrelationContext) -> BuildResultArgs {
    BuildResultArgs {
        status: match outcome.status {
            TurnStatus::Ok => "ok".into(),
            TurnStatus::Error | TurnStatus::Cancelled => "error".into(),
        },
        response: outcome.response,
        tool_calls_made: outcome
            .tool_calls_made
            .into_iter()
            .map(tool_call_summary)
            .collect(),
        usage: outcome.usage.map(usage_summary),
        iterations: outcome.iterations,
        error: outcome.error.map(error_envelope),
        correlation: Some(correlation),
        total_latency_ms: Some(outcome.total_latency_ms),
        tool_latency_ms: Some(outcome.tool_latency_ms),
        model_latency_ms: Some(outcome.model_latency_ms),
    }
}

fn tool_call_summary(call: RuntimeToolCall) -> ToolCallSummary {
    ToolCallSummary {
        name: call.name,
        args: call.args,
    }
}

fn usage_summary(usage: RuntimeUsage) -> UsageSummary {
    UsageSummary {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
    }
}

fn error_envelope(error: RuntimeError) -> ErrorEnvelope {
    ErrorEnvelope {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
        details: error.details,
    }
}

fn map_runtime_event(event: &RuntimeEvent) -> Option<WorkerEvent> {
    match event {
        RuntimeEvent::ContentDelta { text } => {
            Some(WorkerEvent::ContentDelta { text: text.clone() })
        }
        RuntimeEvent::ToolStarted { name, call } => {
            let args: Value = serde_json::from_str(&call.function.arguments).unwrap_or(Value::Null);
            Some(WorkerEvent::ToolStart {
                name: name.clone(),
                args,
            })
        }
        RuntimeEvent::ToolFinished { name, result, .. } => Some(WorkerEvent::ToolEnd {
            name: name.clone(),
            result_preview: result.chars().take(500).collect(),
        }),
        RuntimeEvent::UsageUpdated { usage } => Some(WorkerEvent::Usage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        }),
        RuntimeEvent::Error { error } => Some(WorkerEvent::Error {
            code: error.code.clone(),
            message: error.message.clone(),
            retryable: error.retryable,
            provider: error.provider.clone(),
            details: error.details.clone(),
        }),
        RuntimeEvent::PermissionDenied { name, reason, .. } => {
            Some(WorkerEvent::PermissionDenied {
                name: name.clone(),
                reason: reason.clone(),
            })
        }
        RuntimeEvent::PlanCompleted { plan } => {
            Some(WorkerEvent::PlanComplete { plan: plan.clone() })
        }
        RuntimeEvent::ContextPruned {
            messages_pruned,
            tokens_before,
            tokens_after,
        } => Some(WorkerEvent::ContextPrune {
            messages_pruned: *messages_pruned,
            tokens_before: *tokens_before,
            tokens_after: *tokens_after,
        }),
        RuntimeEvent::ContextCompacted => Some(WorkerEvent::ContextCompact),
        RuntimeEvent::ContextHandoff => Some(WorkerEvent::ContextHandoff),
        RuntimeEvent::PermissionRequested { .. }
        | RuntimeEvent::AssistantMessage { .. }
        | RuntimeEvent::TurnStateChanged { .. } => None,
    }
}
