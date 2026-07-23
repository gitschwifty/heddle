//! Wire types for the headless JSON-over-stdio protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::errors::ErrorEnvelope;
use crate::config::types::PermissionsConfigSchema;
use crate::hooks::types::HooksConfig;
use crate::provider::types::AppAttribution;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Default,
    Isolated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePlacementConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RuntimeMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherit_ambient_config: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutingMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouping_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routed_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveRuntimeMetadata {
    pub mode: RuntimeMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_root: Option<String>,
    pub transcript_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureDetails {
    pub code: String,
    pub termination_reason: String,
    pub iterations: u32,
    pub tool_calls_made: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitConfig {
    pub model: String,
    pub system_prompt: String,
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_attribution: Option<AppAttribution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionsConfigSchema>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HooksConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimePlacementConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<RoutingMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcRequest {
    Init {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        protocol_version: Option<String>,
        config: Box<InitConfig>,
    },
    Send {
        id: String,
        message: String,
    },
    Status {
        id: String,
    },
    Shutdown {
        id: String,
    },
    Cancel {
        id: String,
        target_id: String,
    },
}

impl IpcRequest {
    pub fn id(&self) -> &str {
        match self {
            Self::Init { id, .. }
            | Self::Send { id, .. }
            | Self::Status { id }
            | Self::Shutdown { id }
            | Self::Cancel { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkerEvent {
    ContentDelta {
        text: String,
    },
    ToolStart {
        name: String,
        args: Value,
    },
    ToolEnd {
        name: String,
        result_preview: String,
    },
    Usage {
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_micros: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_currency: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cached_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_write_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        generation_id: Option<String>,
    },
    RoutedModel {
        model: String,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<Value>,
    },
    PermissionRequest {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    PermissionDenied {
        name: String,
        reason: String,
    },
    PlanComplete {
        plan: String,
    },
    ContextPrune {
        messages_pruned: u64,
        tokens_before: u64,
        tokens_after: u64,
    },
    ContextCompact,
    ContextHandoff,
    Heartbeat {
        duration_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallSummary {
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcResponse {
    InitOk {
        id: String,
        session_id: String,
        protocol_version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ErrorEnvelope>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime: Option<EffectiveRuntimeMetadata>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routing: Option<RoutingMetadata>,
    },
    Event {
        event: WorkerEvent,
        event_seq: u64,
        send_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worker_id: Option<String>,
    },
    Result {
        id: String,
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response: Option<String>,
        tool_calls_made: Vec<ToolCallSummary>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<UsageSummary>,
        iterations: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ErrorEnvelope>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worker_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_latency_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_latency_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total_latency_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime: Option<EffectiveRuntimeMetadata>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routing: Option<RoutingMetadata>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failure: Option<FailureDetails>,
    },
    StatusOk {
        id: String,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_routed_model: Option<String>,
        messages_count: u64,
        session_id: String,
        active: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime: Option<EffectiveRuntimeMetadata>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routing: Option<RoutingMetadata>,
    },
    ShutdownOk {
        id: String,
    },
}
