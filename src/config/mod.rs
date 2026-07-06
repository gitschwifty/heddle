//! Configuration: TOML loading, path resolution, discovery, skills, AGENTS.md.

pub mod agents_md;
pub mod discovery;
pub mod features;
pub mod loader;
pub mod paths;
pub mod skills;
pub mod types;

pub use loader::{load_config, ApprovalMode, HeddleConfig, PermissionsLayer};
