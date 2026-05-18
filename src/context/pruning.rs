//! Tool-result pruning. Mirrors `ts-src/context/pruning.ts`.

use crate::types::Message;

#[derive(Debug, Clone, Default)]
pub struct PruneResult {
    pub messages_pruned: u64,
    pub tokens_before: u64,
    pub tokens_after: u64,
}

#[derive(Debug, Clone, Default)]
pub struct PruningOptions {
    pub protect_window_tokens: Option<u64>,
    pub prune_threshold_tokens: Option<u64>,
    pub is_compaction_output: bool,
}

const DEFAULT_PROTECT_WINDOW: u64 = 40_000;
const DEFAULT_PRUNE_THRESHOLD: u64 = 20_000;

/// Char/4 heuristic over JSON-serialized messages.
pub fn estimate_tokens(messages: &[Message]) -> u64 {
    if messages.is_empty() {
        return 0;
    }
    let json = serde_json::to_string(messages).unwrap_or_default();
    json.len().div_ceil(4) as u64
}

fn estimate_one(msg: &Message) -> u64 {
    let json = serde_json::to_string(msg).unwrap_or_default();
    json.len().div_ceil(4) as u64
}

pub fn prune_tool_results(messages: &mut [Message], options: &PruningOptions) -> PruneResult {
    let tokens_before = estimate_tokens(messages);
    if options.is_compaction_output {
        return PruneResult {
            messages_pruned: 0,
            tokens_before,
            tokens_after: tokens_before,
        };
    }

    let protect_window = options
        .protect_window_tokens
        .unwrap_or(DEFAULT_PROTECT_WINDOW);
    let threshold = options
        .prune_threshold_tokens
        .unwrap_or(DEFAULT_PRUNE_THRESHOLD);
    if tokens_before < threshold {
        return PruneResult {
            messages_pruned: 0,
            tokens_before,
            tokens_after: tokens_before,
        };
    }

    // Walk backward to find protection boundary
    let mut accumulated: u64 = 0;
    let mut protection_boundary: usize = 1;
    if messages.len() > 1 {
        for i in (1..messages.len()).rev() {
            accumulated += estimate_one(&messages[i]);
            if accumulated > protect_window {
                protection_boundary = i + 1;
                break;
            }
        }
    }

    // Prune tool messages in [1, protection_boundary)
    let mut count: u64 = 0;
    for msg in messages.iter_mut().take(protection_boundary).skip(1) {
        if let Message::Tool(tm) = msg {
            if !tm.content.starts_with("[pruned") {
                let original_len = tm.content.len();
                tm.content = format!("[pruned — original: {original_len} chars]");
                count += 1;
            }
        }
    }

    let tokens_after = estimate_tokens(messages);
    PruneResult {
        messages_pruned: count,
        tokens_before,
        tokens_after,
    }
}
