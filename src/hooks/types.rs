//! Hook types — mirrors `ts-src/hooks/types.ts`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
    PrePrompt,
    PreTool,
    PostTool,
    PostTurn,
    Error,
}

impl std::str::FromStr for HookEvent {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "session_start" => Ok(Self::SessionStart),
            "session_end" => Ok(Self::SessionEnd),
            "pre_prompt" => Ok(Self::PrePrompt),
            "pre_tool" => Ok(Self::PreTool),
            "post_tool" => Ok(Self::PostTool),
            "post_turn" => Ok(Self::PostTurn),
            "error" => Ok(Self::Error),
            _ => Err(()),
        }
    }
}

impl HookEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::SessionEnd => "session_end",
            Self::PrePrompt => "pre_prompt",
            Self::PreTool => "pre_tool",
            Self::PostTool => "post_tool",
            Self::PostTurn => "post_turn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookMode {
    Interactive,
    Headless,
    Both,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookMatchers {
    /// Either a single tool name or a list of names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_args: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_input: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolMatch {
    Single(String),
    Many(Vec<String>),
}

/// Wire-format hook entry from TOML/JSON before defaults are applied.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookDefinition {
    pub command: String,
    pub timeout: Option<u64>,
    pub mode: Option<HookMode>,
    #[serde(rename = "async", default)]
    pub r#async: Option<bool>,
    pub matchers: Option<HookMatchers>,
}

/// Full wire-format hooks config (each event maps to a list).
pub type HooksConfig = HashMap<String, Vec<HookDefinition>>;

#[derive(Debug, Clone)]
pub struct ResolvedHookDefinition {
    pub command: String,
    pub timeout: u64,
    pub mode: HookMode,
    pub r#async: bool,
    pub matchers: Option<HookMatchers>,
}

pub type ResolvedHooksConfig = HashMap<HookEvent, Vec<ResolvedHookDefinition>>;

#[derive(Debug, Clone, Default)]
pub struct HookContext {
    pub session_id: String,
    pub project: String,
    pub model: String,
    pub event: String,
    pub tool_name: Option<String>,
    pub tool_args: Option<String>,
    pub tool_result: Option<String>,
    pub user_input: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct HookResult {
    pub blocked: bool,
    pub reason: Option<String>,
    pub feedback: Option<String>,
    pub timed_out: bool,
}
