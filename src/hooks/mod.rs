//! Lifecycle hooks: shell commands fired on session/tool/turn events.

pub mod loader;
pub mod matcher;
pub mod runner;
pub mod types;

pub use runner::HooksRunner;
pub use types::{HookContext, HookEvent, HookResult, ResolvedHookDefinition, ResolvedHooksConfig};
