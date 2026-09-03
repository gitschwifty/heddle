//! JSON-over-stdio adapter for embedding heddle in other tools.
//!
//! Reads `IpcRequest`s line-by-line from stdin, processes them serially, and
//! writes `IpcResponse`s to stdout. Cancellation flips an `AbortToken` watched
//! by the in-flight agent loop.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use parking_lot::Mutex;
use serde_json::Value;
use sha2::{Digest, Sha256};
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
    CancellationSource, EffectiveRoutingMetadata, EffectiveRuntimeMetadata, FailureDetails,
    HeadlessCredentialSource, HeadlessRouter, InitConfig, IpcCapabilities, IpcRequest, IpcResponse,
    PermissionFailureDetails, ProfileIdentity, ProviderFailureDetails, RoutingMetadata,
    RuntimeMode, ToolCallSummary, TurnStateEvent, UsageSummary, WorkerEvent,
};
use crate::runtime::{
    HeddleRuntime, RuntimeError, RuntimeEvent, RuntimeToolCall, RuntimeUsage, TurnOptions,
    TurnOutcome, TurnState, TurnStatus,
};
use crate::session::setup::{
    create_session, PermissionOverrides, RuntimePlacement, SessionContext, SessionOptions,
};
use crate::tools::ask_user::create_ask_user_tool;
use crate::tools::registry::ToolRegistry;

struct State {
    runtime: Option<HeddleRuntime>,
    correlation: CorrelationContext,
    active_id: Option<String>,
    cancel_target_id: Option<String>,
    active_cancel: Option<CancellationToken>,
    pending_cancel_ids: Vec<String>,
    runtime_metadata: Option<EffectiveRuntimeMetadata>,
    routing: Option<RoutingMetadata>,
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
            runtime_metadata: None,
            routing: None,
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

    let (session_opts, mut runtime_metadata) = match build_session_options(&config) {
        Ok(value) => value,
        Err(error) => {
            write_line(&protocol_error(Some(&id), error));
            return;
        }
    };
    let session = match create_session(session_opts).await {
        Ok(s) => s,
        Err(e) => {
            write_line(&protocol_error(Some(&id), e.to_string()));
            return;
        }
    };

    let session = wire_ipc_overrides(session, &config);
    let capabilities = ipc_capabilities(&session.registry);
    let profile = profile_identity(&config, &capabilities);

    let session_id = session.session_id.clone();
    if let Some(metadata) = &mut runtime_metadata {
        metadata.transcript_path = session.session_file.to_string_lossy().into_owned();
    }
    let router = match session.config.provider {
        crate::config::loader::ProviderKind::OpenRouter => "openrouter",
        crate::config::loader::ProviderKind::Straitly => "straitly",
    };
    let runtime = HeddleRuntime::from_session(session);
    let effective_routing = Some(EffectiveRoutingMetadata {
        router: Some(router.to_string()),
        ..EffectiveRoutingMetadata::default()
    });
    {
        let mut s = state.lock();
        s.correlation = CorrelationContext {
            session_id: Some(session_id.clone()),
            task_id: config.task_id.clone(),
            worker_id: config.worker_id.clone(),
        };
        s.runtime = Some(runtime);
        s.runtime_metadata = runtime_metadata.clone();
        s.routing = config.routing.clone();
    }

    write_line(&IpcResponse::InitOk {
        id,
        session_id,
        protocol_version: PROTOCOL_VERSION.clone(),
        error: None,
        runtime: runtime_metadata,
        routing: config.routing.clone(),
        requested_routing: config.routing.clone(),
        effective_routing,
        capabilities: Some(capabilities),
        profile: Some(profile),
    });
}

fn profile_identity(config: &InitConfig, capabilities: &IpcCapabilities) -> ProfileIdentity {
    // This intentionally hashes only settings already exposed through the IPC
    // contract or safe booleans. It never includes prompts, credentials,
    // permission patterns, hook commands, or filesystem paths.
    let safe_profile = serde_json::json!({
        "model": config.model,
        "enabled_tools": capabilities.enabled_tools,
        "runtime_mode": config.runtime.as_ref().and_then(|runtime| runtime.mode.clone()),
        "max_iterations": config.max_iterations,
        "permissions_configured": config.permissions.is_some(),
        "hooks_configured": config.hooks.is_some(),
    });
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(&safe_profile).expect("safe profile is serializable"));
    ProfileIdentity {
        fingerprint: format!("sha256:{:x}", hasher.finalize()),
        model: config.model.clone(),
    }
}

fn ipc_capabilities(registry: &ToolRegistry) -> IpcCapabilities {
    IpcCapabilities {
        enabled_tools: registry.names(),
        explicit_tool_allowlist: true,
        runtime_modes: vec![RuntimeMode::Default, RuntimeMode::Isolated],
        transcript_placement: true,
        failure_details_version: "v2".into(),
        routing_request_metadata: true,
        effective_routing_metadata: true,
        cache_usage_metrics: true,
        cancellation: true,
        turn_state_events: true,
    }
}

fn build_session_options(
    config: &InitConfig,
) -> std::result::Result<(SessionOptions, Option<EffectiveRuntimeMetadata>), String> {
    let runtime = config.runtime.as_ref();
    let mode = runtime
        .and_then(|r| r.mode.clone())
        .unwrap_or(RuntimeMode::Default);
    let state_root = runtime
        .and_then(|r| r.state_root.as_ref())
        .map(PathBuf::from);
    let transcript_path = runtime
        .and_then(|r| r.transcript_path.as_ref())
        .map(PathBuf::from);
    let config_path = runtime
        .and_then(|r| r.config_path.as_ref())
        .map(PathBuf::from);
    if matches!(mode, RuntimeMode::Isolated) && state_root.is_none() {
        return Err("runtime.state_root is required when runtime.mode is isolated".into());
    }
    if state_root.as_ref().is_some_and(|path| !path.is_absolute()) {
        return Err("runtime.state_root must be an absolute path".into());
    }
    if transcript_path
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err("runtime.transcript_path must be an absolute path".into());
    }
    if config_path.as_ref().is_some_and(|path| !path.is_absolute()) {
        return Err("runtime.config_path must be an absolute path".into());
    }
    if let Some(path) = &config_path {
        if !path.is_file() {
            return Err(format!(
                "runtime.config_path must name an existing configuration file: {}",
                path.display()
            ));
        }
    }
    let suppress_ambient_context = matches!(mode, RuntimeMode::Isolated)
        && !runtime
            .and_then(|r| r.inherit_ambient_config)
            .unwrap_or(false);
    let placement = runtime.map(|_| RuntimePlacement {
        state_root: state_root.clone(),
        transcript_path: transcript_path.clone(),
        config_path,
        suppress_ambient_context,
    });
    let runtime_metadata = runtime.map(|_| EffectiveRuntimeMetadata {
        mode,
        state_root: state_root.map(|path| path.to_string_lossy().into_owned()),
        transcript_path: String::new(),
    });
    let (headless_environment_credentials_only, headless_credential_reference) =
        match config.credential_source.as_ref() {
            None | Some(HeadlessCredentialSource::Environment) => (true, None),
            Some(HeadlessCredentialSource::Keychain { reference }) => {
                crate::credentials::validate_credential_reference(reference)
                    .map_err(|error| error.to_string())?;
                (false, Some(reference.clone()))
            }
        };
    let router = config.router.map(|router| match router {
        HeadlessRouter::OpenRouter => crate::config::loader::ProviderKind::OpenRouter,
        HeadlessRouter::Straitly => crate::config::loader::ProviderKind::Straitly,
    });
    Ok((
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
            app_attribution: config.app_attribution.clone(),
            runtime_placement: placement,
            router,
            headless_environment_credentials_only,
            headless_credential_reference,
            ..Default::default()
        },
        runtime_metadata,
    ))
}

fn wire_ipc_overrides(mut session: SessionContext, config: &InitConfig) -> SessionContext {
    if config.tools.iter().any(|name| name == "ask_user") {
        let _ = session
            .registry
            .register(create_ask_user_tool(Arc::new(|_question, _options| {
                Box::pin(
                    async move { "User interaction not available in headless mode".to_string() },
                )
            })));
    }

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
    // A headless caller's inventory is a capability boundary, not a hint for
    // filtering only the default file tools. Session setup may have added
    // interactive conveniences such as subagent or task tools; retain them
    // here only when the IPC init explicitly requested their names.
    session.registry = restrict_headless_registry(session.registry, &config.tools);
    session
}

fn restrict_headless_registry(registry: ToolRegistry, tools: &[String]) -> ToolRegistry {
    registry.subset(tools)
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
    {
        let mut seq = event_seq.lock();
        write_line(&wrap_event(
            WorkerEvent::TurnState {
                state: TurnStateEvent::Queued,
            },
            &id,
            *seq,
            Some(&correlation),
        ));
        *seq += 1;
    }
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

    {
        let mut seq = event_seq.lock();
        write_line(&wrap_event(
            WorkerEvent::TurnState {
                state: TurnStateEvent::Completed,
            },
            &id,
            *seq,
            Some(&correlation),
        ));
        *seq += 1;
    }

    let (runtime_metadata, requested_routing) = {
        let s = state.lock();
        (s.runtime_metadata.clone(), s.routing.clone())
    };
    let mut routing = requested_routing.clone();
    let status = runtime.status(false);
    if let Some(metadata) = &mut routing {
        metadata.routed_model = status.last_routed_model.clone();
        metadata.effective_upstream_provider = status.last_upstream_provider.clone();
        metadata.upstream_provider_history = status.upstream_provider_history.clone();
    }
    let effective_routing = effective_routing(&status);
    let cancellation_source = (outcome.status == TurnStatus::Cancelled
        && state.lock().cancel_target_id.as_deref() == Some(id.as_str()))
    .then_some(CancellationSource::User);
    write_line(&build_result(
        &id,
        build_result_args(
            outcome,
            correlation,
            runtime_metadata,
            routing,
            requested_routing,
            effective_routing,
            cancellation_source,
        ),
    ));
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
    let effective_routing = effective_routing(&status);
    write_line(&IpcResponse::StatusOk {
        id,
        model: status.model,
        last_routed_model: status.last_routed_model.clone(),
        messages_count: status.messages_count,
        session_id: status.session_id,
        active: status.active,
        runtime: s.runtime_metadata.clone(),
        routing: s.routing.clone().map(|mut metadata| {
            metadata.routed_model = status.last_routed_model.clone();
            metadata.effective_upstream_provider = status.last_upstream_provider.clone();
            metadata.upstream_provider_history = status.upstream_provider_history.clone();
            metadata
        }),
        requested_routing: s.routing.clone(),
        effective_routing,
    });
}

fn build_result_args(
    outcome: TurnOutcome,
    correlation: CorrelationContext,
    runtime: Option<EffectiveRuntimeMetadata>,
    routing: Option<RoutingMetadata>,
    requested_routing: Option<RoutingMetadata>,
    effective_routing: Option<EffectiveRoutingMetadata>,
    cancellation_source: Option<CancellationSource>,
) -> BuildResultArgs {
    let failure = outcome.error.as_ref().map(|error| FailureDetails {
        code: error.code.clone(),
        termination_reason: error.message.clone(),
        iterations: outcome.iterations,
        tool_calls_made: outcome.tool_calls_made.len() as u32,
        last_tool_name: outcome.tool_calls_made.last().map(|call| call.name.clone()),
        last_tool: outcome
            .tool_calls_made
            .last()
            .cloned()
            .map(tool_call_summary),
        loop_count: outcome.failure_evidence.loop_count,
        loop_threshold: outcome.failure_evidence.loop_threshold,
        provider: provider_failure_details(error),
        permission: outcome
            .failure_evidence
            .permission_denied
            .as_ref()
            .map(|permission| PermissionFailureDetails {
                name: permission.name.clone(),
                call_id: permission.call_id.clone(),
                reason: permission.reason.clone(),
            }),
        malformed_tool_call: outcome
            .failure_evidence
            .malformed_tool_call
            .clone()
            .map(tool_call_summary),
        cancellation_source,
    });
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
        runtime,
        routing,
        requested_routing,
        effective_routing,
        failure,
    }
}

fn provider_failure_details(error: &RuntimeError) -> Option<ProviderFailureDetails> {
    let provider = error.details.as_ref()?.get("provider")?;
    Some(ProviderFailureDetails {
        name: provider
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| error.provider.clone()),
        status: provider
            .get("status")
            .and_then(Value::as_u64)
            .and_then(|status| u16::try_from(status).ok()),
        status_category: provider
            .get("status_category")
            .and_then(Value::as_str)
            .map(str::to_string),
        retry_after_ms: provider.get("retry_after_ms").and_then(Value::as_u64),
        error_type: provider
            .get("error_type")
            .and_then(Value::as_str)
            .map(str::to_string),
        provider_code: provider
            .get("provider_code")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn effective_routing(status: &crate::runtime::RuntimeStatus) -> Option<EffectiveRoutingMetadata> {
    let metadata = EffectiveRoutingMetadata {
        router: Some(status.router.clone()),
        routed_model: status.last_routed_model.clone(),
        upstream_provider: status.last_upstream_provider.clone(),
        upstream_provider_history: status.upstream_provider_history.clone(),
    };
    (metadata.router.is_some()
        || metadata.routed_model.is_some()
        || metadata.upstream_provider.is_some()
        || !metadata.upstream_provider_history.is_empty())
    .then_some(metadata)
}

fn tool_call_summary(call: RuntimeToolCall) -> ToolCallSummary {
    ToolCallSummary {
        id: call.id,
        name: call.name,
        args: call.args,
    }
}

fn usage_summary(usage: RuntimeUsage) -> UsageSummary {
    UsageSummary {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        cost_micros: usage.cost_micros,
        cost_currency: usage.cost_currency,
        cached_tokens: usage.cached_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        generation_id: usage.generation_id,
        router: usage.router,
        model: usage.model,
        time_to_first_chunk_ms: usage.time_to_first_chunk_ms,
        time_to_first_output_ms: usage.time_to_first_output_ms,
        total_duration_ms: (usage.total_duration_ms > 0).then_some(usage.total_duration_ms),
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
            cost_micros: usage.cost_micros,
            cost_currency: usage.cost_currency.clone(),
            cached_tokens: usage.cached_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            generation_id: usage.generation_id.clone(),
            router: usage.router.clone(),
            model: usage.model.clone(),
            time_to_first_chunk_ms: usage.time_to_first_chunk_ms,
            time_to_first_output_ms: usage.time_to_first_output_ms,
            total_duration_ms: (usage.total_duration_ms > 0).then_some(usage.total_duration_ms),
        }),
        RuntimeEvent::RoutedModel { model } => Some(WorkerEvent::RoutedModel {
            model: model.clone(),
        }),
        RuntimeEvent::UpstreamProvider { provider } => Some(WorkerEvent::UpstreamProvider {
            provider: provider.clone(),
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
        RuntimeEvent::TurnStateChanged { state, .. } => match state {
            TurnState::Queued => Some(WorkerEvent::TurnState {
                state: TurnStateEvent::Queued,
            }),
            TurnState::Running => Some(WorkerEvent::TurnState {
                state: TurnStateEvent::Running,
            }),
            TurnState::Cancelling => Some(WorkerEvent::TurnState {
                state: TurnStateEvent::Cancelling,
            }),
            // The adapter emits this once immediately before the final result,
            // including for cancellation paths where runtime returns early.
            TurnState::Completed => None,
        },
        RuntimeEvent::PermissionRequested { .. } | RuntimeEvent::AssistantMessage { .. } => None,
    }
}

#[cfg(test)]
mod headless_tool_inventory_tests {
    use super::*;

    #[test]
    fn inventory_removes_session_added_tools_unless_explicitly_requested() {
        let mut registry = ToolRegistry::new();
        registry
            .register(create_ask_user_tool(Arc::new(|_question, _options| {
                Box::pin(async move { "unavailable".to_string() })
            })))
            .unwrap();

        assert!(restrict_headless_registry(registry.clone(), &[])
            .get("ask_user")
            .is_none());
        assert!(restrict_headless_registry(registry, &["ask_user".into()])
            .get("ask_user")
            .is_some());
    }
}
