//! Per-session metrics. Mirrors `ts-src/usage/collector.ts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageCounts {
    pub user: u64,
    pub assistant: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorCounts {
    pub tool: u64,
    pub provider: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMetrics {
    #[serde(rename = "messageCount")]
    pub message_count: MessageCounts,
    #[serde(rename = "toolCalls")]
    pub tool_calls: BTreeMap<String, u64>,
    pub errors: ErrorCounts,
    pub tokens: TokenCounts,
    pub turns: u64,
}

#[derive(Debug, Default)]
pub struct MetricsCollector {
    user_messages: u64,
    assistant_messages: u64,
    tool_calls: BTreeMap<String, u64>,
    tool_errors: u64,
    provider_errors: u64,
    input_tokens: u64,
    output_tokens: u64,
    turns: u64,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_assistant_message(&mut self) {
        self.assistant_messages += 1;
    }

    pub fn on_user_message(&mut self) {
        self.user_messages += 1;
        self.turns += 1;
    }

    pub fn on_tool_call(&mut self, name: &str) {
        *self.tool_calls.entry(name.to_string()).or_insert(0) += 1;
    }

    pub fn on_tool_error(&mut self) {
        self.tool_errors += 1;
    }

    pub fn on_provider_error(&mut self) {
        self.provider_errors += 1;
    }

    pub fn on_usage(&mut self, prompt_tokens: u64, completion_tokens: u64) {
        self.input_tokens += prompt_tokens;
        self.output_tokens += completion_tokens;
    }

    pub fn metrics(&self) -> SessionMetrics {
        SessionMetrics {
            message_count: MessageCounts {
                user: self.user_messages,
                assistant: self.assistant_messages,
            },
            tool_calls: self.tool_calls.clone(),
            errors: ErrorCounts {
                tool: self.tool_errors,
                provider: self.provider_errors,
            },
            tokens: TokenCounts {
                input: self.input_tokens,
                output: self.output_tokens,
            },
            turns: self.turns,
        }
    }
}
