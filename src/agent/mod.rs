//! Agent loop: streaming and non-streaming variants, two-phase architect.

pub mod architect;
pub mod loop_;
pub mod types;

pub use architect::{run_architect_pipeline, ArchitectOptions};
pub use loop_::{run_agent_loop, run_agent_loop_streaming, AgentLoopOptions, PermissionResolver};
pub use types::AgentEvent;
