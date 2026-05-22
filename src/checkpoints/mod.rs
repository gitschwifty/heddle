//! Prompt-scoped file checkpoints.
//!
//! On each user turn the REPL snapshots `FileHistoryMeta`, runs the agent,
//! then diffs the snapshot to determine which files received new backup
//! versions during the turn. The diff is recorded as a `CheckpointRecord`
//! line in the session JSONL, alongside the conversation. `/rewind` reads
//! those records and uses the existing `file_history::restore` machinery
//! to roll files back to their pre-turn content, optionally also forking
//! the session at the turn boundary so the conversation can be replayed from
//! the same point with code in its pre-turn state.
//!
//! Bash-driven file modifications are not tracked — `backup_file` is only
//! called from the edit/write tools, mirroring Claude Code's behavior.

pub mod diff;
pub mod io;
pub mod record;
pub mod restore;

pub use diff::{compute_changes, compute_changes_with_touched, snapshot_meta, MetaSnapshot};
pub use io::{load_checkpoints, write_checkpoint};
pub use record::{CheckpointRecord, FileChange};
pub use restore::restore_code;
