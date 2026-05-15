//! Wire-format (snake_case) config schemas. Mirrors `ts-src/config/types.ts`.
//!
//! Used by both the TOML loader and the headless IPC `init` message. Kept as a
//! separate module so the loader can produce internal config from these and the
//! IPC layer can validate inbound payloads against the same shape.

use serde::{Deserialize, Serialize};

use crate::hooks::types::HooksConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalModeWire {
    Suggest,
    AutoEdit,
    FullAuto,
    Plan,
    Yolo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConfigSchema {
    pub model: Option<String>,
    pub weak_model: Option<String>,
    pub editor_model: Option<String>,
    pub max_tokens: Option<f64>,
    pub temperature: Option<f64>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionConfigSchema {
    pub system_prompt: Option<String>,
    pub approval_mode: Option<ApprovalModeWire>,
    pub instructions: Option<Vec<String>>,
    pub tools: Option<Vec<String>>,
    pub doom_loop_threshold: Option<f64>,
    pub budget_limit: Option<f64>,
    pub compact_trigger: Option<f64>,
    pub prune_protect: Option<f64>,
    pub prune_minimum: Option<f64>,
    pub compact_buffer: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeaturesSchema {
    pub history: Option<bool>,
    pub usage_data: Option<bool>,
    pub facets: Option<bool>,
    pub file_history: Option<bool>,
    pub paste_cache: Option<bool>,
    pub status_line: Option<bool>,
    pub hooks: Option<bool>,
    pub tasks: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionsConfigSchema {
    pub allow: Option<Vec<String>>,
    pub deny: Option<Vec<String>>,
    pub ask: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HeddleConfigSchema {
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub weak_model: Option<String>,
    pub editor_model: Option<String>,
    pub max_tokens: Option<f64>,
    pub temperature: Option<f64>,
    pub base_url: Option<String>,
    pub system_prompt: Option<String>,
    pub approval_mode: Option<ApprovalModeWire>,
    pub instructions: Option<Vec<String>>,
    pub tools: Option<Vec<String>>,
    pub doom_loop_threshold: Option<f64>,
    pub budget_limit: Option<f64>,
    pub compact_trigger: Option<f64>,
    pub prune_protect: Option<f64>,
    pub prune_minimum: Option<f64>,
    pub compact_buffer: Option<f64>,
    pub features: Option<FeaturesSchema>,
    pub permissions: Option<PermissionsConfigSchema>,
    pub hooks: Option<HooksConfig>,
}
