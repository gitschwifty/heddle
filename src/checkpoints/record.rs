//! Checkpoint data shapes serialized into session JSONL.

use serde::{Deserialize, Serialize};

/// A single file's backup-version movement across a turn.
///
/// `version_after` is the load-bearing field for `/rewind`: it names the
/// backup file (`v{version_after}.bak`) that was written during the turn
/// and contains the pre-turn content. `version_after == 0` means no
/// backup was written this turn — the file was created from scratch —
/// so the pre-turn state is "file does not exist" and rewinding removes
/// the file.
///
/// `version_before` is informational (shown by `/rewind list` and helps
/// reconstruct history). For files newly created during the turn, both
/// are 0 and the `uuid` field is empty (no `file_history` entry was
/// registered).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub file_path: String,
    pub uuid: String,
    pub version_before: u32,
    pub version_after: u32,
}

/// One record per user turn. Written to the session JSONL via
/// `append_context_marker` with `type: "checkpoint"`.
///
/// `messages_before_turn` is the count of messages already in the session
/// at the instant the user's prompt arrived (before it was appended).
/// Passing it to `fork_session(up_to_message: ...)` restores the
/// conversation to the state immediately before this turn ran.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRecord {
    #[serde(rename = "type")]
    pub kind: String,
    pub turn_index: u64,
    pub messages_before_turn: u64,
    pub user_preview: String,
    pub changes: Vec<FileChange>,
}

impl CheckpointRecord {
    pub fn new(
        turn_index: u64,
        messages_before_turn: u64,
        user_preview: String,
        changes: Vec<FileChange>,
    ) -> Self {
        Self {
            kind: "checkpoint".into(),
            turn_index,
            messages_before_turn,
            user_preview,
            changes,
        }
    }
}
