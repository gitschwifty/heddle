//! Layered TOML config loader (defaults → global → local → env).
//! Mirrors `ts-src/config/loader.ts`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use toml::Value as TomlValue;

use crate::config::features::FeatureFlagsOverride;
use crate::config::paths::{get_heddle_home, get_local_heddle_dir};
use crate::debug::debug;
use crate::hooks::loader::load_hooks;
use crate::hooks::types::ResolvedHooksConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalMode {
    Suggest,
    AutoEdit,
    FullAuto,
    Plan,
    Yolo,
}

impl ApprovalMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "suggest" => Some(Self::Suggest),
            "auto-edit" => Some(Self::AutoEdit),
            "full-auto" => Some(Self::FullAuto),
            "plan" => Some(Self::Plan),
            "yolo" => Some(Self::Yolo),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Suggest => "suggest",
            Self::AutoEdit => "auto-edit",
            Self::FullAuto => "full-auto",
            Self::Plan => "plan",
            Self::Yolo => "yolo",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PermissionsLayer {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub ask: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HeddleConfig {
    pub api_key: Option<String>,

    pub model: String,
    pub weak_model: Option<String>,
    pub editor_model: Option<String>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub base_url: Option<String>,

    pub system_prompt: Option<String>,
    pub approval_mode: Option<ApprovalMode>,
    pub instructions: Option<Vec<String>>,
    pub tools: Option<Vec<String>>,
    pub doom_loop_threshold: Option<u32>,
    pub budget_limit: Option<f64>,
    pub compact_trigger: Option<f64>,
    pub prune_protect: Option<u64>,
    pub prune_minimum: Option<u32>,
    pub compact_buffer: Option<f64>,
    pub features: Option<FeatureFlagsOverride>,

    pub permissions_layers: Option<Vec<PermissionsLayer>>,
    pub hooks: Option<ResolvedHooksConfig>,
}

impl Default for HeddleConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            model: "openrouter/free".to_string(),
            weak_model: None,
            editor_model: None,
            max_tokens: None,
            temperature: None,
            base_url: None,
            system_prompt: None,
            approval_mode: None,
            instructions: None,
            tools: None,
            doom_loop_threshold: None,
            budget_limit: None,
            compact_trigger: None,
            prune_protect: None,
            prune_minimum: None,
            compact_buffer: None,
            features: None,
            permissions_layers: None,
            hooks: None,
        }
    }
}

fn load_toml(path: &Path) -> TomlValue {
    if !path.exists() {
        return TomlValue::Table(Default::default());
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return TomlValue::Table(Default::default()),
    };
    if content.trim().is_empty() {
        return TomlValue::Table(Default::default());
    }
    content
        .parse::<TomlValue>()
        .unwrap_or_else(|_| TomlValue::Table(Default::default()))
}

fn as_str(v: &TomlValue) -> Option<&str> {
    v.as_str()
}

fn as_int(v: &TomlValue) -> Option<i64> {
    v.as_integer()
}

fn as_float(v: &TomlValue) -> Option<f64> {
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
}

fn as_bool(v: &TomlValue) -> Option<bool> {
    v.as_bool()
}

fn as_string_array(v: &TomlValue) -> Option<Vec<String>> {
    let arr = v.as_array()?;
    let filtered: Vec<String> = arr
        .iter()
        .filter_map(|x| x.as_str().map(|s| s.to_string()))
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

fn apply_raw(config: &mut HeddleConfig, raw: &TomlValue) {
    let table = match raw.as_table() {
        Some(t) => t,
        None => return,
    };

    if let Some(s) = table.get("model").and_then(as_str) {
        config.model = s.to_string();
    }
    if let Some(s) = table.get("api_key").and_then(as_str) {
        config.api_key = Some(s.to_string());
    }
    if let Some(s) = table.get("system_prompt").and_then(as_str) {
        config.system_prompt = Some(s.to_string());
    }
    if let Some(s) = table.get("weak_model").and_then(as_str) {
        config.weak_model = Some(s.to_string());
    }
    if let Some(s) = table.get("editor_model").and_then(as_str) {
        config.editor_model = Some(s.to_string());
    }
    if let Some(s) = table.get("base_url").and_then(as_str) {
        config.base_url = Some(s.to_string());
    }

    if let Some(n) = table.get("max_tokens").and_then(as_int) {
        config.max_tokens = Some(n as u64);
    }
    if let Some(n) = table.get("temperature").and_then(as_float) {
        config.temperature = Some(n);
    }
    if let Some(n) = table.get("doom_loop_threshold").and_then(as_int) {
        config.doom_loop_threshold = Some(n as u32);
    }
    if let Some(n) = table.get("budget_limit").and_then(as_float) {
        config.budget_limit = Some(n);
    }
    if let Some(n) = table.get("compact_trigger").and_then(as_float) {
        config.compact_trigger = Some(n);
    }
    if let Some(n) = table.get("prune_protect").and_then(as_int) {
        config.prune_protect = Some(n as u64);
    }
    if let Some(n) = table.get("prune_minimum").and_then(as_int) {
        config.prune_minimum = Some(n as u32);
    }
    if let Some(n) = table.get("compact_buffer").and_then(as_float) {
        config.compact_buffer = Some(n);
    }

    if let Some(am) = table
        .get("approval_mode")
        .and_then(as_str)
        .and_then(ApprovalMode::from_str)
    {
        config.approval_mode = Some(am);
    }

    if let Some(arr) = table.get("instructions").and_then(as_string_array) {
        config.instructions = Some(arr);
    }
    if let Some(arr) = table.get("tools").and_then(as_string_array) {
        config.tools = Some(arr);
    }

    if let Some(feat) = table.get("features").and_then(|v| v.as_table()) {
        let mut over = FeatureFlagsOverride::default();
        if let Some(b) = feat.get("history").and_then(as_bool) {
            over.history = Some(b);
        }
        if let Some(b) = feat.get("usage_data").and_then(as_bool) {
            over.usage_data = Some(b);
        }
        if let Some(b) = feat.get("facets").and_then(as_bool) {
            over.facets = Some(b);
        }
        if let Some(b) = feat.get("file_history").and_then(as_bool) {
            over.file_history = Some(b);
        }
        if let Some(b) = feat.get("paste_cache").and_then(as_bool) {
            over.paste_cache = Some(b);
        }
        if let Some(b) = feat.get("status_line").and_then(as_bool) {
            over.status_line = Some(b);
        }
        if let Some(b) = feat.get("hooks").and_then(as_bool) {
            over.hooks = Some(b);
        }
        if let Some(b) = feat.get("tasks").and_then(as_bool) {
            over.tasks = Some(b);
        }
        config.features = Some(over);
    }
}

fn extract_permissions(raw: &TomlValue) -> Option<PermissionsLayer> {
    let perms = raw.as_table()?.get("permissions")?.as_table()?;
    let to_strings = |key: &str| -> Vec<String> {
        perms
            .get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let allow = to_strings("allow");
    let deny = to_strings("deny");
    let ask = to_strings("ask");
    if allow.is_empty() && deny.is_empty() && ask.is_empty() {
        None
    } else {
        Some(PermissionsLayer { allow, deny, ask })
    }
}

/// Load config from defaults → global → local → env vars.
pub fn load_config(local_dir: Option<&Path>) -> HeddleConfig {
    let global_path = get_heddle_home().join("config.toml");
    let local_root = local_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(get_local_heddle_dir);
    let local_path = local_root.join("config.toml");

    let global_raw = load_toml(&global_path);
    let local_raw = load_toml(&local_path);

    let mut merged = HeddleConfig::default();
    apply_raw(&mut merged, &global_raw);
    apply_raw(&mut merged, &local_raw);

    let mut layers: Vec<PermissionsLayer> = Vec::new();
    if let Some(l) = extract_permissions(&global_raw) {
        layers.push(l);
    }
    if let Some(l) = extract_permissions(&local_raw) {
        layers.push(l);
    }
    if !layers.is_empty() {
        merged.permissions_layers = Some(layers);
    }

    let hooks = load_hooks(&global_raw, &local_raw);
    if !hooks.is_empty() {
        merged.hooks = Some(hooks);
    }

    if let Ok(v) = std::env::var("HEDDLE_MODEL") {
        merged.model = v;
    }
    if let Ok(v) = std::env::var("OPENROUTER_API_KEY") {
        merged.api_key = Some(v);
    }
    if let Ok(v) = std::env::var("HEDDLE_BASE_URL") {
        merged.base_url = Some(v);
    }
    if let Ok(v) = std::env::var("HEDDLE_MAX_TOKENS") {
        if let Ok(n) = v.parse::<u64>() {
            merged.max_tokens = Some(n);
        }
    }
    if let Ok(v) = std::env::var("HEDDLE_TEMPERATURE") {
        if let Ok(n) = v.parse::<f64>() {
            merged.temperature = Some(n);
        }
    }
    if let Ok(v) = std::env::var("HEDDLE_WEAK_MODEL") {
        merged.weak_model = Some(v);
    }
    if let Ok(v) = std::env::var("HEDDLE_APPROVAL_MODE") {
        if let Some(am) = ApprovalMode::from_str(&v) {
            merged.approval_mode = Some(am);
        }
    }
    if let Ok(v) = std::env::var("HEDDLE_TOOLS") {
        let parsed: Vec<String> = v
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !parsed.is_empty() {
            merged.tools = Some(parsed);
        }
    }

    debug("config", "loaded (api_key REDACTED if present)");
    merged
}
