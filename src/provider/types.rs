//! Provider trait. Streaming `stream()` returns a heap-allocated `Stream` so
//! impls can be swapped freely.

use std::pin::Pin;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use serde_json::Value;

use crate::types::{ChatCompletionResponse, Message, StreamChunk, ToolDefinition};

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
