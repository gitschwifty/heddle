//! Agent event types yielded by the streaming + non-streaming loops.

use crate::provider::types::ProviderTelemetry;
use crate::types::{AssistantMessage, ToolCall, Usage};

/// Monotonic timing for one provider call. The call may be one iteration of a
/// tool-use loop, so this is intentionally separate from whole-turn latency.
#[derive(Debug, Clone, Default)]
pub struct ProviderCallTiming {
    pub time_to_first_chunk_ms: Option<u64>,
    pub time_to_first_output_ms: Option<u64>,
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    AssistantMessage {
        message: AssistantMessage,
        finish_reason: Option<String>,
    },
    ContentDelta {
        text: String,
    },
    ToolStart {
        name: String,
        call: ToolCall,
    },
    ToolEnd {
        name: String,
        result: String,
        call: ToolCall,
    },
    Usage {
        usage: Usage,
        generation_id: Option<String>,
        timing: ProviderCallTiming,
    },
    RoutedModel {
        model: String,
    },
    /// An upstream provider observed in the response. Absence deliberately
    /// remains unknown; callers must not infer this from the routed model.
    UpstreamProvider {
        provider: String,
    },
    LoopDetected {
        count: u32,
    },
    Error {
        message: String,
    },
    ProviderError {
        message: String,
        telemetry: ProviderTelemetry,
        debug_detail: Option<String>,
    },
    PermissionRequest {
        name: String,
        call: ToolCall,
        reason: Option<String>,
    },
    PermissionDenied {
        name: String,
        call: ToolCall,
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
}
