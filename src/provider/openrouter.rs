//! OpenRouter chat-completions client (streaming + non-streaming).
//!

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Map, Value};

use super::overrides::validate_overrides;
use super::types::{AppAttribution, ChunkStream, Provider, ProviderConfig};
use crate::debug::debug;
use crate::types::{ChatCompletionResponse, Message, StreamChunk, ToolDefinition};

const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_MAX_RETRIES: u32 = 3;
const DEFAULT_BASE_DELAY_MS: u64 = 1000;
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 45;
const DEFAULT_REFERER: &str = "https://github.com/gitschwifty/heddle";
const DEFAULT_TITLE: &str = "Heddle";

pub struct OpenRouterProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

pub fn create_openrouter_provider(config: ProviderConfig) -> Arc<dyn Provider> {
    Arc::new(OpenRouterProvider {
        config,
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new()),
    })
}

impl OpenRouterProvider {
    fn base_url(&self) -> &str {
        self.config.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)
    }

    fn build_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let attribution = effective_attribution(self.config.app_attribution.as_ref());
        headers.insert("HTTP-Referer", HeaderValue::from_str(attribution.referer)?);
        headers.insert(
            "X-OpenRouter-Title",
            HeaderValue::from_str(attribution.title)?,
        );
        headers.insert("X-Title", HeaderValue::from_str(attribution.title)?);
        if let Some(categories) = attribution.categories {
            headers.insert(
                "X-OpenRouter-Categories",
                HeaderValue::from_str(categories)?,
            );
        }
        Ok(headers)
    }

    fn build_body(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        stream: bool,
        overrides: &Value,
    ) -> Value {
        let validated = validate_overrides(overrides);

        let mut body = Map::new();
        body.insert("model".into(), Value::String(self.config.model.clone()));
        body.insert(
            "messages".into(),
            serde_json::to_value(messages).unwrap_or(Value::Null),
        );
        body.insert("stream".into(), Value::Bool(stream));

        if let Some(extra) = self
            .config
            .request_params
            .as_ref()
            .and_then(Value::as_object)
        {
            for (k, v) in extra {
                body.insert(k.clone(), v.clone());
            }
        }
        if let Some(extra) = validated.as_object() {
            for (k, v) in extra {
                body.insert(k.clone(), v.clone());
            }
        }
        // Explicit model override (overrides.model > config.model)
        if let Some(model) = validated.get("model").and_then(Value::as_str) {
            body.insert("model".into(), Value::String(model.to_string()));
        }
        if let Some(t) = tools {
            if !t.is_empty() {
                body.insert(
                    "tools".into(),
                    serde_json::to_value(t).unwrap_or(Value::Null),
                );
            }
        }
        debug("provider", "request ready");
        Value::Object(body)
    }

    async fn fetch_with_retry(
        &self,
        url: &str,
        headers: HeaderMap,
        body: &Value,
    ) -> Result<reqwest::Response> {
        let retry_cfg = self.config.retry.clone();
        let max_retries = retry_cfg
            .as_ref()
            .map(|r| {
                if r.max_retries == 0 {
                    DEFAULT_MAX_RETRIES
                } else {
                    r.max_retries
                }
            })
            .unwrap_or(0);
        let base_delay_ms = retry_cfg
            .as_ref()
            .map(|r| {
                if r.base_delay_ms == 0 {
                    DEFAULT_BASE_DELAY_MS
                } else {
                    r.base_delay_ms
                }
            })
            .unwrap_or(DEFAULT_BASE_DELAY_MS);

        for attempt in 0..=max_retries {
            let resp = self
                .client
                .post(url)
                .headers(headers.clone())
                .json(body)
                .send()
                .await?;

            if resp.status().as_u16() != 429 || retry_cfg.is_none() || attempt == max_retries {
                return Ok(resp);
            }

            let retry_after_ms = resp
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(|s| s * 1000);
            let delay = retry_after_ms.unwrap_or_else(|| base_delay_ms * (1u64 << attempt));
            debug(
                "provider",
                &format!(
                    "429 rate limited, retry {}/{max_retries} after {delay}ms",
                    attempt + 1
                ),
            );
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        Err(anyhow!("Retry loop exited unexpectedly"))
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    async fn send(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        overrides: &Value,
    ) -> Result<ChatCompletionResponse> {
        let body = self.build_body(messages, tools, false, overrides);
        let url = format!("{}/chat/completions", self.base_url());
        let headers = self.build_headers()?;
        let resp = self.fetch_with_retry(&url, headers, &body).await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            debug("provider", &format!("error {status}: {text}"));
            return Err(anyhow!("OpenRouter API error ({status}): {text}"));
        }
        let parsed: ChatCompletionResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("error decoding provider JSON response: {e}"))?;
        Ok(parsed)
    }

    fn stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        overrides: Value,
    ) -> ChunkStream {
        let url = format!("{}/chat/completions", self.base_url());
        let api_key = self.config.api_key.clone();
        let base_url = self.base_url().to_string();
        let request_params = self.config.request_params.clone();
        let app_attribution = self.config.app_attribution.clone();
        let model = self.config.model.clone();
        let retry = self.config.retry.clone();
        let client = self.client.clone();

        let stream = try_stream! {
            let provider = OpenRouterProvider {
                config: ProviderConfig {
                    api_key: api_key.clone(),
                    model: model.clone(),
                    base_url: Some(base_url),
                    request_params,
                    app_attribution,
                    retry,
                },
                client,
            };
            let body = provider.build_body(&messages, tools.as_deref(), true, &overrides);
            let headers = provider.build_headers().map_err(|e| anyhow!(e))?;
            let resp = provider.fetch_with_retry(&url, headers, &body).await?;
            let status = resp.status();
            let mut byte_stream = if status.is_success() {
                resp.bytes_stream()
            } else {
                let text = resp.text().await.unwrap_or_default();
                Err::<reqwest::Response, _>(anyhow!("OpenRouter API error ({status}): {text}"))?
                    .bytes_stream()
            };
            let mut buffer = String::new();
            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk
                    .map_err(|e| anyhow!("error reading streaming response body: {e}"))?;
                buffer.push_str(std::str::from_utf8(&chunk).unwrap_or(""));

                while let Some(nl_idx) = buffer.find('\n') {
                    let line = buffer[..nl_idx].trim().to_string();
                    buffer.drain(..=nl_idx);

                    if line.is_empty() || !line.starts_with("data: ") {
                        continue;
                    }
                    let data = &line[6..];
                    if data == "[DONE]" {
                        return;
                    }
                    let parsed: StreamChunk = serde_json::from_str(data).map_err(|e| {
                        let preview: String = data.chars().take(500).collect();
                        anyhow!("error decoding streaming response chunk: {e}; data={preview}")
                    })?;
                    yield parsed;
                }
            }
            let trimmed = buffer.trim();
            if let Some(data) = trimmed.strip_prefix("data: ") {
                if data != "[DONE]" {
                    let parsed: StreamChunk = serde_json::from_str(data).map_err(|e| {
                        let preview: String = data.chars().take(500).collect();
                        anyhow!("error decoding trailing streaming response chunk: {e}; data={preview}")
                    })?;
                    yield parsed;
                }
            }
        };
        Box::pin(stream)
    }

    fn with(&self, overrides: Value) -> Arc<dyn Provider> {
        let validated = validate_overrides(&overrides);
        let mut new_config = self.config.clone();
        if let Some(model) = validated.get("model").and_then(Value::as_str) {
            new_config.model = model.to_string();
        }
        // Merge into request_params
        let mut merged = self
            .config
            .request_params
            .clone()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        if let Some(extra) = validated.as_object() {
            for (k, v) in extra {
                merged.insert(k.clone(), v.clone());
            }
        }
        new_config.request_params = Some(Value::Object(merged));
        create_openrouter_provider(new_config)
    }
}

struct EffectiveAttribution<'a> {
    referer: &'a str,
    title: &'a str,
    categories: Option<&'a str>,
}

fn effective_attribution(attribution: Option<&AppAttribution>) -> EffectiveAttribution<'_> {
    match attribution {
        Some(attr) if !attr.referer.trim().is_empty() && !attr.title.trim().is_empty() => {
            EffectiveAttribution {
                referer: attr.referer.trim(),
                title: attr.title.trim(),
                categories: attr
                    .categories
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
            }
        }
        _ => EffectiveAttribution {
            referer: DEFAULT_REFERER,
            title: DEFAULT_TITLE,
            categories: None,
        },
    }
}

/// Construct a JSON `Value` from key-value pairs (small ergonomics helper for
/// tests/callers that need to build overrides inline).
#[doc(hidden)]
pub fn empty_overrides() -> Value {
    json!({})
}
