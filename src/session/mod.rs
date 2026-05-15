//! Session lifecycle: setup, JSONL persistence, list/find, fork.

pub mod fork;
pub mod jsonl;
pub mod list;
pub mod setup;

pub use fork::{fork_session, ForkResult};
pub use jsonl::{
    append_context_marker, append_message, load_all_session_metas, load_session, load_session_meta,
    write_session_meta, SessionMeta,
};
pub use list::{find_session, list_sessions, SessionInfo};
pub use setup::{create_session, SessionContext, SessionOptions};
