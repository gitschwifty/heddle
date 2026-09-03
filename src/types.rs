//! Core message and tool types.
//!
//! Wire format follows OpenAI Chat Completions. `serde` is the schema source of
//! truth (TypeBox lived double duty as TS type + JSON schema; we don't need the
//! JSON schema side, so plain serde structs suffice).

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Message Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System(SystemMessage),
    User(UserMessage),
    Assistant(AssistantMessage),
    Tool(ToolMessage),
}

impl Message {
    pub fn role(&self) -> &'static str {
        match self {
            Message::System(_) => "system",
            Message::User(_) => "user",
            Message::Assistant(_) => "assistant",
            Message::Tool(_) => "tool",
        }
    }

    /// Returns the textual content of the message if it has one.
    ///
    /// Assistant messages may have `null` content when they only contain tool
    /// calls; that case returns `None`.
    pub fn content_str(&self) -> Option<&str> {
        match self {
            Message::System(m) => Some(&m.content),
            Message::User(m) => Some(&m.content),
            Message::Assistant(m) => m.content.as_deref(),
            Message::Tool(m) => Some(&m.content),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemMessage {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessage {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantMessage {
    /// Per the OpenAI schema, content can be null when only tool_calls are
    /// present. `Option<String>` plus `serialize_with` keeps that wire shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolMessage {
    pub tool_call_id: String,
    pub content: String,
}

// ── Tool Calls ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: ToolCallKind,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolCallKind {
    #[default]
    Function,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

// ── Tool Definitions ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: ToolCallKind,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

// ── API Response Types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    #[serde(default)]
    pub model: Option<String>,
    /// OpenRouter's observed upstream provider. This is a fact reported by
    /// the response, not the requested routing preference.
    #[serde(default, alias = "provider_name")]
    pub provider: Option<String>,
    #[serde(default)]
    pub openrouter_metadata: Option<OpenRouterMetadata>,
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChoiceMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChoiceMessage {
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<PromptTokenDetails>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<CompletionTokenDetails>,
    /// Straitly reports cache creation at the top level rather than under
    /// `prompt_tokens_details`. Keep these provider aliases here so all
    /// callers consume one normalized usage shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_5m_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_1h_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

impl Usage {
    pub fn cached_tokens(&self) -> Option<u64> {
        self.prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens)
            .or(self.cache_read_input_tokens)
    }

    /// Normalized cache writes across OpenAI-compatible provider schemas.
    pub fn cache_write_tokens(&self) -> Option<u64> {
        // Straitly returns `cache_creation_input_tokens` as the total, and
        // repeats its breakdown in the 5m/1h fields and OpenAI-style nested
        // details. Prefer that total so aliases never inflate usage.
        if let Some(total) = self.cache_creation_input_tokens {
            return Some(total);
        }
        if let Some(nested) = self
            .prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cache_write_tokens)
        {
            return Some(nested);
        }
        let tiers = self.cache_creation_5m_input_tokens.unwrap_or(0)
            + self.cache_creation_1h_input_tokens.unwrap_or(0);
        (tiers > 0).then_some(tiers)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptTokenDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompletionTokenDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

// ── Streaming Delta Types ──────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunk {
    pub id: String,
    #[serde(default)]
    pub model: Option<String>,
    /// OpenRouter may include the upstream provider on stream chunks.
    #[serde(default, alias = "provider_name")]
    pub provider: Option<String>,
    #[serde(default)]
    pub openrouter_metadata: Option<OpenRouterMetadata>,
    pub choices: Vec<StreamChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// Additive, opt-in routing information from OpenRouter. Keep this permissive:
/// OpenRouter may add pipeline fields without a schema bump.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenRouterMetadata {
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub endpoints: Option<OpenRouterEndpoints>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenRouterEndpoints {
    #[serde(default)]
    pub available: Vec<OpenRouterEndpoint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenRouterEndpoint {
    pub provider: String,
    #[serde(default)]
    pub selected: bool,
}

impl OpenRouterMetadata {
    pub fn selected_provider(&self) -> Option<&str> {
        self.endpoints
            .as_ref()?
            .available
            .iter()
            .find(|endpoint| endpoint.selected)
            .map(|endpoint| endpoint.provider.as_str())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    pub index: u32,
    pub delta: Delta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallDelta {
    pub index: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: Option<ToolCallKind>,
    #[serde(default)]
    pub function: Option<FunctionCallDelta>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FunctionCallDelta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}
