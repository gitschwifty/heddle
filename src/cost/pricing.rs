//! Model pricing data fetched from the OpenRouter `/models` endpoint.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::Deserialize;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ModelPricingInfo {
    pub id: String,
    pub name: String,
    pub prompt_price: f64,
    pub completion_price: f64,
    pub context_length: u64,
    pub max_completion_tokens: u64,
    pub modality: String,
    pub supported_parameters: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApiModelEntry {
    id: String,
    name: String,
    pricing: ApiPricing,
    context_length: u64,
    top_provider: ApiTopProvider,
    architecture: ApiArchitecture,
    #[serde(default)]
    supported_parameters: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApiPricing {
    prompt: String,
    completion: String,
}

#[derive(Debug, Deserialize)]
struct ApiTopProvider {
    #[serde(default)]
    max_completion_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct ApiArchitecture {
    modality: String,
}

#[derive(Debug, Deserialize)]
struct ApiModelsResponse {
    data: Vec<ApiModelEntry>,
}

#[derive(Clone)]
pub struct ModelPricing {
    inner: Arc<Mutex<Inner>>,
    api_key: String,
    base_url: String,
}

struct Inner {
    models: Option<HashMap<String, ModelPricingInfo>>,
}

impl ModelPricing {
    pub fn new(api_key: impl Into<String>, base_url: Option<&str>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner { models: None })),
            api_key: api_key.into(),
            base_url: base_url
                .unwrap_or("https://openrouter.ai/api/v1")
                .to_string(),
        }
    }

    pub async fn get_model(&self, model_id: &str) -> Option<ModelPricingInfo> {
        self.ensure_loaded().await.ok()?;
        let guard = self.inner.lock().await;
        guard.models.as_ref()?.get(model_id).cloned()
    }

    pub async fn get_all_models(&self) -> Vec<ModelPricingInfo> {
        if self.ensure_loaded().await.is_err() {
            return Vec::new();
        }
        let guard = self.inner.lock().await;
        guard
            .models
            .as_ref()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn estimate_cost(
        &self,
        model_id: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
    ) -> Option<f64> {
        let model = self.get_model(model_id).await?;
        Some(
            prompt_tokens as f64 * model.prompt_price
                + completion_tokens as f64 * model.completion_price,
        )
    }

    pub async fn is_loaded(&self) -> bool {
        self.inner.lock().await.models.is_some()
    }

    async fn ensure_loaded(&self) -> Result<()> {
        // Hold the lock across the fetch so concurrent callers dedupe to a
        // single network round-trip (mirrors TS in-flight Promise sharing).
        let mut guard = self.inner.lock().await;
        if guard.models.is_some() {
            return Ok(());
        }
        let map = self.fetch_models().await?;
        guard.models = Some(map);
        Ok(())
    }

    async fn fetch_models(&self) -> Result<HashMap<String, ModelPricingInfo>> {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/models", self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("Models API returned {}", resp.status()));
        }
        let parsed: ApiModelsResponse = resp.json().await?;
        let mut map = HashMap::new();
        for entry in parsed.data {
            map.insert(
                entry.id.clone(),
                ModelPricingInfo {
                    id: entry.id,
                    name: entry.name,
                    prompt_price: entry.pricing.prompt.parse().unwrap_or(0.0),
                    completion_price: entry.pricing.completion.parse().unwrap_or(0.0),
                    context_length: entry.context_length,
                    max_completion_tokens: entry.top_provider.max_completion_tokens,
                    modality: entry.architecture.modality,
                    supported_parameters: entry.supported_parameters,
                },
            );
        }
        Ok(map)
    }
}
