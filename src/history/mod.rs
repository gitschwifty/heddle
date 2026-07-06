//! Cross-session message history (the user-typed input log).

pub mod reader;
pub mod writer;

pub use reader::{load_history, LoadHistoryOptions};
pub use writer::{append_history_entry, HistoryEntry};
