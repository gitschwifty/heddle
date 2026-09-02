//! Build the bundle of `(main, weak, editor)` providers from `HeddleConfig`.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

use super::openrouter::{create_openrouter_provider, create_straitly_provider};
use super::types::{Provider, ProviderConfig, RetryConfig};
use crate::config::loader::{HeddleConfig, OpenRouterRoutingMode, ProviderKind};
use crate::credentials::resolve_credential;

const STRAITLY_BASE_URL: &str = "https://api.straitly.ai/v1";

#[derive(Clone)]
pub struct Providers {
    pub main: Arc<dyn Provider>,
    pub weak: Option<Arc<dyn Provider>>,
    pub editor: Option<Arc<dyn Provider>>,
}

fn base_request_params(config: &HeddleConfig) -> Option<Value> {
    let mut map = Map::new();
    if let Some(mt) = config.max_tokens {
        map.insert("max_tokens".into(), Value::Number(mt.into()));
    }
    if let Some(t) = config.temperature {
        if let Some(n) = serde_json::Number::from_f64(t) {
            map.insert("temperature".into(), Value::Number(n));
        }
    }
    if config.provider == ProviderKind::OpenRouter
        && config.openrouter_routing == OpenRouterRoutingMode::Nitro
    {
        map.insert(
            "provider".into(),
            serde_json::json!({ "sort": "throughput" }),
        );
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

pub fn create_providers(config: &HeddleConfig) -> Result<Providers> {
    let credential = match config.provider {
        ProviderKind::OpenRouter => config.openrouter_credential.as_deref(),
        ProviderKind::Straitly => config.straitly_credential.as_deref(),
    };
    let api_key = credential
        .and_then(|reference| resolve_credential(reference).ok())
        .or_else(|| config.api_key.clone())
        .ok_or_else(|| anyhow!("{} credential is required", router_name(config.provider)))?;
    let params = base_request_params(config);

    let build = |model: &str| -> Arc<dyn Provider> {
        let model = if config.provider == ProviderKind::OpenRouter
            && config.openrouter_routing == OpenRouterRoutingMode::Exacto
            && !model.ends_with(":exacto")
        {
            format!("{model}:exacto")
        } else {
            model.to_string()
        };
        let provider_config = ProviderConfig {
            api_key: api_key.clone(),
            model,
            base_url: config.base_url.clone().or_else(|| {
                (config.provider == ProviderKind::Straitly).then(|| STRAITLY_BASE_URL.to_string())
            }),
            request_params: params.clone(),
            app_attribution: config.app_attribution.clone(),
            retry: Some(RetryConfig::default()),
        };
        match config.provider {
            ProviderKind::OpenRouter => create_openrouter_provider(provider_config),
            ProviderKind::Straitly => create_straitly_provider(provider_config),
        }
    };

    let main = build(&config.model);
    let weak = config.weak_model.as_deref().map(build);
    let editor = config.editor_model.as_deref().map(build);

    Ok(Providers { main, weak, editor })
}

fn router_name(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::OpenRouter => "OpenRouter router",
        ProviderKind::Straitly => "Straitly router",
    }
}
