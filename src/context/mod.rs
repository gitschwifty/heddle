//! Context management: token-budget pruning, weak-model compaction, paste cache.

pub mod compaction;
pub mod paste_cache;
pub mod pruning;

pub use compaction::{
    build_compaction_prompt, compact_context, get_compactable_messages, should_compact,
    CompactionConfig, CompactionStats,
};
pub use paste_cache::{CachedPaste, PasteCache};
pub use pruning::{estimate_tokens, prune_tool_results, PruneResult, PruningOptions};
