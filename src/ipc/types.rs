//! Wire types for the headless JSON-over-stdio protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::errors::ErrorEnvelope;
use crate::config::types::PermissionsConfigSchema;
use crate::hooks::types::HooksConfig;

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
    pub permissions: Option<PermissionsConfigSchema>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HooksConfig>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallSummary {
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcResponse {
    InitOk {
        id: String,
        session_id: String,
        protocol_version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ErrorEnvelope>,
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
    },
    StatusOk {
        id: String,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_routed_model: Option<String>,
        messages_count: u64,
        session_id: String,
        active: bool,
    },
    ShutdownOk {
        id: String,
    },
}
