//! Slash-command framework + built-in commands + custom command loader.

pub mod builtins;
pub mod loader;
pub mod registry;
pub mod types;

pub use builtins::create_builtin_commands;
pub use loader::load_custom_commands;
pub use registry::CommandRegistry;
pub use types::{CommandContext, SlashCommand};
