//! Feature flags and mode defaults.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureFlags {
    pub history: bool,
    pub usage_data: bool,
    pub facets: bool,
    pub file_history: bool,
    pub paste_cache: bool,
    pub status_line: bool,
    pub hooks: bool,
    pub tasks: bool,
    pub checkpoints: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct FeatureFlagsOverride {
    pub history: Option<bool>,
    pub usage_data: Option<bool>,
    pub facets: Option<bool>,
    pub file_history: Option<bool>,
    pub paste_cache: Option<bool>,
    pub status_line: Option<bool>,
    pub hooks: Option<bool>,
    pub tasks: Option<bool>,
    pub checkpoints: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Interactive,
    NonInteractive,
    Headless,
}

const INTERACTIVE: FeatureFlags = FeatureFlags {
    history: true,
    usage_data: true,
    facets: true,
    file_history: true,
    paste_cache: true,
    status_line: true,
    hooks: true,
    tasks: true,
    checkpoints: true,
};

const NON_INTERACTIVE: FeatureFlags = FeatureFlags {
    history: false,
    usage_data: true,
    facets: true,
    file_history: true,
    paste_cache: true,
    status_line: false,
    hooks: true,
    tasks: true,
    checkpoints: false,
};

const HEADLESS: FeatureFlags = FeatureFlags {
    history: false,
    usage_data: true,
    facets: false,
    file_history: true,
    paste_cache: false,
    status_line: false,
    hooks: true,
    tasks: true,
    checkpoints: false,
};

pub fn mode_defaults(mode: Mode) -> FeatureFlags {
    match mode {
        Mode::Interactive => INTERACTIVE,
        Mode::NonInteractive => NON_INTERACTIVE,
        Mode::Headless => HEADLESS,
    }
}

pub fn get_features(mode: Mode, overrides: Option<&FeatureFlagsOverride>) -> FeatureFlags {
    let mut out = mode_defaults(mode);
    if let Some(o) = overrides {
        if let Some(v) = o.history {
            out.history = v;
        }
        if let Some(v) = o.usage_data {
            out.usage_data = v;
        }
        if let Some(v) = o.facets {
            out.facets = v;
        }
        if let Some(v) = o.file_history {
            out.file_history = v;
        }
        if let Some(v) = o.paste_cache {
            out.paste_cache = v;
        }
        if let Some(v) = o.status_line {
            out.status_line = v;
        }
        if let Some(v) = o.hooks {
            out.hooks = v;
        }
        if let Some(v) = o.tasks {
            out.tasks = v;
        }
        if let Some(v) = o.checkpoints {
            out.checkpoints = v;
        }
    }
    out
}
