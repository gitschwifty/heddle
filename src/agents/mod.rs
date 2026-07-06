//! Agent definitions (markdown + YAML frontmatter).

pub mod loader;
pub mod types;

pub use loader::{load_agent_definitions, parse_agent_file};
pub use types::AgentDefinition;
