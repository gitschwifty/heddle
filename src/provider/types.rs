//! Provider trait. Streaming `stream()` returns a heap-allocated `Stream` so
//! impls can be swapped freely.

use std::pin::Pin;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use serde_json::Value;

use crate::types::{ChatCompletionResponse, Message, StreamChunk, ToolDefinition};

/// A narrowly-scoped classification for a provider response that was an HTTP
/// success but could not be used by the agent. These values deliberately do
/// not describe arbitrary decoder failures, which remain non-retryable.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFailureKind {
    EmptySuccessBody,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AppAttribution {
    pub referer: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RetryConfig {
    /// Default 3.
    pub max_retries: u32,
    /// Default 1000ms.
    pub base_delay_ms: u64,
    /// Maximum delay for a single retry. Defaults to 15 seconds for normal
    /// interactive/headless clients; batch callers can opt into a longer wait.
    pub max_delay_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
    /// Extra fields merged into every request body.
    pub request_params: Option<Value>,
    /// Optional app attribution headers for provider dashboards.
    pub app_attribution: Option<AppAttribution>,
    /// `None` ⇒ retry disabled; `Some(_)` ⇒ retry on 429.
    pub retry: Option<RetryConfig>,
}

/// Safe, bounded provider-side correlation data. It intentionally excludes
/// request bodies, authorization, and arbitrary response payloads.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ProviderTelemetry {
    /// Safe classification of a malformed HTTP-success response. This is
    /// separate from the provider's own error type because there may be no
    /// provider error envelope at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<ProviderFailureKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderFailure {
    pub message: String,
    pub telemetry: ProviderTelemetry,
    /// Bounded sanitized diagnostic for the dedicated debug-error artifact.
    /// Never serialize this into normal eval evidence.
    pub debug_detail: Option<String>,
}

impl std::fmt::Display for ProviderFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ProviderFailure {}

pub type StreamItem = Result<StreamChunk>;
pub type ChunkStream = Pin<Box<dyn Stream<Item = StreamItem> + Send>>;

#[async_trait]
pub trait Provider: Send + Sync {
    async fn send(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        overrides: &Value,
    ) -> Result<ChatCompletionResponse>;

    fn stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        overrides: Value,
    ) -> ChunkStream;

    /// Return a provider that merges the given overrides into every call.
    fn with(&self, overrides: Value) -> std::sync::Arc<dyn Provider>;
}
