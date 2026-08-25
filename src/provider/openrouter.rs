//! OpenRouter chat-completions client (streaming + non-streaming).
//!

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use rand::Rng;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Map, Value};

use super::overrides::validate_overrides;
use super::types::{
    AppAttribution, ChunkStream, Provider, ProviderConfig, ProviderFailure, ProviderFailureKind,
    ProviderTelemetry,
};
use crate::debug::debug;
use crate::types::{ChatCompletionResponse, Message, StreamChunk, ToolDefinition};

const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_MAX_RETRIES: u32 = 3;
const DEFAULT_BASE_DELAY_MS: u64 = 1000;
const DEFAULT_MAX_DELAY_MS: u64 = 15_000;
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
        // OpenRouter only returns the actual selected provider and fallback
        // context when explicitly requested. This response-only metadata is
        // safe to retain and lets callers distinguish facts from preferences.
        headers.insert("X-OpenRouter-Metadata", HeaderValue::from_static("enabled"));
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
        let max_delay_ms = retry_cfg
            .as_ref()
            .map(|r| {
                if r.max_delay_ms == 0 {
                    DEFAULT_MAX_DELAY_MS
                } else {
                    r.max_delay_ms
                }
            })
            .unwrap_or(DEFAULT_MAX_DELAY_MS);

        for attempt in 0..=max_retries {
            let resp = self
                .client
                .post(url)
                .headers(headers.clone())
                .json(body)
                .send()
                .await
                .map_err(|err| {
                    let detail = format!("{err:#}");
                    debug("provider", &format!("transport error: {detail}"));
                    anyhow!("OpenRouter transport error: {detail}")
                })?;

            if resp.status().as_u16() != 429 || retry_cfg.is_none() || attempt == max_retries {
                return Ok(resp);
            }

            let headers = resp.headers().clone();
            let body = resp.text().await.unwrap_or_default();
            let exponential_delay_ms = base_delay_ms.saturating_mul(1u64 << attempt);
            let delay = retry_delay_ms(&headers, &body, exponential_delay_ms, max_delay_ms);
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

const MAX_PROVIDER_DETAIL_CHARS: usize = 500;

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn response_telemetry(headers: &HeaderMap, status: Option<u16>) -> ProviderTelemetry {
    ProviderTelemetry {
        request_id: header_value(headers, "x-request-id")
            .or_else(|| header_value(headers, "x-openrouter-request-id")),
        generation_id: header_value(headers, "x-generation-id"),
        status,
        content_type: header_value(headers, "content-type"),
        retry_after_ms: retry_after_ms(headers),
        ..ProviderTelemetry::default()
    }
}

fn bounded_preview(body: &[u8]) -> String {
    String::from_utf8_lossy(body)
        .chars()
        .take(MAX_PROVIDER_DETAIL_CHARS)
        .collect()
}

fn provider_failure(
    headers: &HeaderMap,
    status: Option<u16>,
    body: &[u8],
    message: impl Into<String>,
) -> ProviderFailure {
    let mut telemetry = response_telemetry(headers, status);
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        telemetry.generation_id = telemetry.generation_id.or_else(|| {
            value
                .get("id")
                .or_else(|| value.get("generation_id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        telemetry.provider = value
            .pointer("/openrouter_metadata/endpoints/available")
            .and_then(Value::as_array)
            .and_then(|endpoints| {
                endpoints.iter().find_map(|endpoint| {
                    endpoint
                        .get("selected")
                        .and_then(Value::as_bool)
                        .filter(|selected| *selected)
                        .and_then(|_| endpoint.get("provider"))
                        .and_then(Value::as_str)
                })
            })
            .map(str::to_string);
        telemetry.error_type = value
            .pointer("/error/metadata/error_type")
            .or_else(|| value.get("error_type"))
            .and_then(Value::as_str)
            .map(str::to_string);
        telemetry.provider_code = value
            .pointer("/error/metadata/provider_code")
            .and_then(Value::as_str)
            .map(str::to_string);
        telemetry.detail = value
            .pointer("/error/message")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .map(|value| value.chars().take(MAX_PROVIDER_DETAIL_CHARS).collect());
    }
    ProviderFailure {
        message: message.into(),
        telemetry,
        debug_detail: None,
    }
}

fn parse_stream_chunk(headers: &HeaderMap, status: u16, data: &str) -> Result<StreamChunk> {
    serde_json::from_str(data).map_err(|error| {
        let mut failure = provider_failure(
            headers,
            Some(status),
            data.as_bytes(),
            if serde_json::from_str::<Value>(data)
                .ok()
                .is_some_and(|value| value.get("error").is_some())
            {
                "OpenRouter streaming provider error".to_string()
            } else {
                let preview: String = data.chars().take(MAX_PROVIDER_DETAIL_CHARS).collect();
                debug(
                    "provider",
                    &format!("stream chunk decode failed: {error}; preview={preview}"),
                );
                format!("error decoding streaming response chunk: {error}")
            },
        );
        if serde_json::from_str::<Value>(data)
            .ok()
            .is_none_or(|value| value.get("error").is_none())
        {
            failure.debug_detail = Some(format!(
                "stream chunk decode failed; preview={}",
                bounded_preview(data.as_bytes())
            ));
        }
        anyhow::Error::new(failure)
    })
}

fn retry_delay_ms(
    headers: &HeaderMap,
    body: &str,
    exponential_delay_ms: u64,
    max_delay_ms: u64,
) -> u64 {
    let requested_delay_ms = retry_after_ms(headers)
        .or_else(|| rate_limit_reset_delay_ms(headers, body))
        .unwrap_or(exponential_delay_ms);
    let capped_delay_ms = requested_delay_ms.min(max_delay_ms);
    if capped_delay_ms == 0 {
        return 0;
    }

    // Spread callers within the configured cap to avoid a thundering herd at
    // the rate-limit boundary.
    let jitter_max_ms = (capped_delay_ms / 10).max(1);
    let jitter_ms = rand::thread_rng().gen_range(0..=jitter_max_ms);
    capped_delay_ms.saturating_add(jitter_ms).min(max_delay_ms)
}

fn retry_after_ms(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("Retry-After")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1000))
}

fn rate_limit_reset_delay_ms(headers: &HeaderMap, body: &str) -> Option<u64> {
    let reset = headers
        .get("X-RateLimit-Reset")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            let body: Value = serde_json::from_str(body).ok()?;
            let reset = body.pointer("/error/metadata/headers/X-RateLimit-Reset")?;
            reset
                .as_str()
                .map(str::to_string)
                .or_else(|| reset.as_u64().map(|value| value.to_string()))
        })?;
    let epoch_ms = reset.parse::<u64>().ok()?;
    let epoch_ms = if epoch_ms < 10_000_000_000 {
        epoch_ms.saturating_mul(1000)
    } else {
        epoch_ms
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some(epoch_ms.saturating_sub(now_ms))
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
        let response_headers = resp.headers().clone();
        let response_body = resp.bytes().await.unwrap_or_default();
        if !status.is_success() {
            let failure = provider_failure(
                &response_headers,
                Some(status.as_u16()),
                &response_body,
                format!("OpenRouter API error ({status})"),
            );
            let mut failure = failure;
            if let Some(detail) = failure.telemetry.detail.clone() {
                failure.message = detail;
            }
            debug(
                "provider",
                &format!("{}: {:?}", failure.message, failure.telemetry),
            );
            return Err(anyhow::Error::new(failure));
        }
        // A 2xx response with an empty JSON body is a provider-side malformed
        // success, not an arbitrary response decode error. Preserve only the
        // status/header correlation data so evals can safely retry it once.
        if response_body.iter().all(u8::is_ascii_whitespace) {
            let mut failure = provider_failure(
                &response_headers,
                Some(status.as_u16()),
                &response_body,
                "provider returned an empty HTTP-success JSON body",
            );
            failure.telemetry.failure_kind = Some(ProviderFailureKind::EmptySuccessBody);
            return Err(anyhow::Error::new(failure));
        }
        let parsed: ChatCompletionResponse =
            serde_json::from_slice(&response_body).map_err(|e| {
                let preview = bounded_preview(&response_body);
                debug(
                    "provider",
                    &format!("response decode failed: {e}; preview={preview}"),
                );
                let mut failure = provider_failure(
                    &response_headers,
                    Some(status.as_u16()),
                    &response_body,
                    format!("error decoding provider JSON response: {e}"),
                );
                failure.debug_detail = Some(format!(
                    "response decode failed: {e}; preview={}",
                    bounded_preview(&response_body)
                ));
                anyhow::Error::new(failure)
            })?;
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
            let response_headers = resp.headers().clone();
            let mut byte_stream = if status.is_success() {
                resp.bytes_stream()
            } else {
                let response_headers = resp.headers().clone();
                let response_body = resp.bytes().await.unwrap_or_default();
                let mut failure = provider_failure(
                    &response_headers,
                    Some(status.as_u16()),
                    &response_body,
                    format!("OpenRouter API error ({status})"),
                );
                if let Some(detail) = failure.telemetry.detail.clone() {
                    failure.message = detail;
                }
                Err::<reqwest::Response, _>(anyhow::Error::new(failure))?
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
                    let parsed = parse_stream_chunk(&response_headers, status.as_u16(), data)?;
                    yield parsed;
                }
            }
            let trimmed = buffer.trim();
            if let Some(data) = trimmed.strip_prefix("data: ") {
                if data != "[DONE]" {
                    let parsed = parse_stream_chunk(&response_headers, status.as_u16(), data)?;
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

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    fn retry_after_header_uses_seconds() {
        let mut headers = HeaderMap::new();
        headers.insert("Retry-After", HeaderValue::from_static("12"));
        assert_eq!(retry_after_ms(&headers), Some(12_000));
    }

    #[test]
    fn reset_is_read_from_openrouter_error_metadata() {
        let headers = HeaderMap::new();
        let future_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 60_000;
        let body = format!(
            r#"{{"error":{{"metadata":{{"headers":{{"X-RateLimit-Reset":"{future_ms}"}}}}}}}}"#
        );
        let delay = rate_limit_reset_delay_ms(&headers, &body).unwrap();
        assert!((59_000..=60_000).contains(&delay), "delay was {delay}");
    }

    #[test]
    fn provider_failure_keeps_only_safe_typed_correlation_fields() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", HeaderValue::from_static("req-123"));
        headers.insert("x-generation-id", HeaderValue::from_static("gen-456"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("retry-after", HeaderValue::from_static("3"));
        let failure = provider_failure(
            &headers,
            Some(429),
            br#"{"error":{"message":"slow down","metadata":{"error_type":"rate_limit_exceeded","provider_code":"rate_limited"}}}"#,
            "OpenRouter API error (429)",
        );
        assert_eq!(failure.telemetry.request_id.as_deref(), Some("req-123"));
        assert_eq!(failure.telemetry.generation_id.as_deref(), Some("gen-456"));
        assert_eq!(failure.telemetry.status, Some(429));
        assert_eq!(failure.telemetry.retry_after_ms, Some(3_000));
        assert_eq!(
            failure.telemetry.error_type.as_deref(),
            Some("rate_limit_exceeded")
        );
        assert_eq!(
            failure.telemetry.provider_code.as_deref(),
            Some("rate_limited")
        );
        assert_eq!(failure.telemetry.detail.as_deref(), Some("slow down"));
    }

    #[test]
    fn malformed_body_does_not_become_normal_telemetry_detail() {
        let headers = HeaderMap::new();
        let failure = provider_failure(
            &headers,
            Some(200),
            b"not valid json; Authorization: secret",
            "error decoding provider JSON response",
        );
        assert!(failure.telemetry.detail.is_none());
        assert_eq!(failure.telemetry.status, Some(200));
    }

    #[test]
    fn stream_error_envelope_becomes_typed_provider_failure() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", HeaderValue::from_static("req-stream"));
        let error = parse_stream_chunk(
            &headers,
            503,
            r#"{"error":{"message":"provider overloaded","metadata":{"error_type":"provider_overloaded","provider_code":"overloaded"}}}"#,
        )
        .unwrap_err();
        let failure = error.downcast_ref::<ProviderFailure>().unwrap();
        assert_eq!(failure.message, "OpenRouter streaming provider error");
        assert_eq!(failure.telemetry.request_id.as_deref(), Some("req-stream"));
        assert_eq!(failure.telemetry.status, Some(503));
        assert_eq!(
            failure.telemetry.error_type.as_deref(),
            Some("provider_overloaded")
        );
    }

    #[test]
    fn retry_delay_is_capped() {
        let headers = HeaderMap::new();
        assert_eq!(retry_delay_ms(&headers, "", 60_000, 0), 0);
    }
}
