//! Agent event types yielded by the streaming + non-streaming loops.

use crate::types::{AssistantMessage, ToolCall, Usage};

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
    },
    RoutedModel {
        model: String,
    },
    LoopDetected {
        count: u32,
    },
    Error {
        message: String,
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
