//! Weak-model compaction. Mirrors `ts-src/context/compaction.ts`.

use anyhow::Result;
use serde_json::json;

use super::pruning::estimate_tokens;
use crate::provider::types::Provider;
use crate::types::{AssistantMessage, Message, SystemMessage, UserMessage};

#[derive(Debug, Clone, Copy)]
pub struct CompactionConfig {
    pub compact_trigger: f64,
    pub prune_protect: u64,
    pub prune_minimum: usize,
    pub compact_buffer: f64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            compact_trigger: 0.8,
            prune_protect: 40_000,
            prune_minimum: 4,
            compact_buffer: 0.5,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompactionStats {
    pub messages_removed: usize,
    pub tokens_before: u64,
    pub tokens_after: u64,
}

pub fn should_compact(messages: &[Message], model_limit: u64, config: CompactionConfig) -> bool {
    let tokens = estimate_tokens(messages) as f64;
    tokens / model_limit as f64 >= config.compact_trigger
}

fn protection_boundary(messages: &[Message], protect: u64) -> usize {
    let mut accumulated: u64 = 0;
    let mut boundary = messages.len();
    if messages.len() > 1 {
        for i in (1..messages.len()).rev() {
            let json = serde_json::to_string(&messages[i]).unwrap_or_default();
            accumulated += ((json.len() + 3) / 4) as u64;
            if accumulated > protect {
                boundary = i + 1;
                break;
            }
        }
    }
    boundary
}

fn is_summary_anchor(msg: &Message) -> bool {
    matches!(msg.content_str(), Some(s) if s.starts_with("[Context Summary]"))
}

pub fn get_compactable_messages(messages: &[Message], config: CompactionConfig) -> Vec<usize> {
    let boundary = protection_boundary(messages, config.prune_protect);
    let mut indices = Vec::new();
    for i in 1..boundary {
        if is_summary_anchor(&messages[i]) {
            continue;
        }
        indices.push(i);
    }
    if indices.len() < config.prune_minimum {
        return Vec::new();
    }
    indices
}

pub fn build_compaction_prompt(messages: &[&Message]) -> String {
    let mut lines = Vec::new();
    for msg in messages {
        let content = msg.content_str().unwrap_or("(no content)");
        lines.push(format!("[{}]: {}", msg.role(), content));
    }
    format!(
        "Summarize the following conversation, preserving:\n- Key decisions made\n- File paths mentioned\n- Tool results and their outcomes\n- Errors encountered and how they were resolved\n\nBe concise but complete. Do not lose important context.\n\n---\n{}\n---\n\nProvide a structured summary:",
        lines.join("\n")
    )
}

pub async fn compact_context(
    messages: &mut Vec<Message>,
    weak_provider: &dyn Provider,
    _model_limit: u64,
    config: CompactionConfig,
) -> Result<CompactionStats> {
    let tokens_before = estimate_tokens(messages);
    let compactable_indices = get_compactable_messages(messages, config);
    if compactable_indices.is_empty() {
        return Ok(CompactionStats {
            messages_removed: 0,
            tokens_before,
            tokens_after: tokens_before,
        });
    }

    // Include any existing anchor that lies in the pre-protection zone.
    let boundary = protection_boundary(messages, config.prune_protect);
    let mut indices_to_remove: std::collections::BTreeSet<usize> =
        compactable_indices.iter().copied().collect();
    for i in 1..boundary {
        if is_summary_anchor(&messages[i]) {
            indices_to_remove.insert(i);
        }
    }

    let to_summarize: Vec<&Message> = indices_to_remove
        .iter()
        .copied()
        .map(|i| &messages[i])
        .collect();

    let prompt = build_compaction_prompt(&to_summarize);

    let summary_messages = vec![
        Message::System(SystemMessage {
            content: "You are a conversation summarizer. Be concise and preserve key context."
                .to_string(),
        }),
        Message::User(UserMessage { content: prompt }),
    ];

    let response = weak_provider
        .send(&summary_messages, None, &json!({}))
        .await?;
    let summary_content = response
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_else(|| "No summary generated.".to_string());

    // Remove high-to-low so indices stay valid.
    let mut sorted: Vec<usize> = indices_to_remove.iter().copied().collect();
    sorted.sort_by(|a, b| b.cmp(a));
    for idx in sorted {
        messages.remove(idx);
    }

    // Insert summary right after the system message.
    let summary = Message::Assistant(AssistantMessage {
        content: Some(format!("[Context Summary] {summary_content}")),
        tool_calls: None,
    });
    let insert_at = if !messages.is_empty() { 1 } else { 0 };
    messages.insert(insert_at, summary);

    let tokens_after = estimate_tokens(messages);
    Ok(CompactionStats {
        messages_removed: indices_to_remove.len(),
        tokens_before,
        tokens_after,
    })
}
