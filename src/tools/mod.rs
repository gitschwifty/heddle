//! Tools available to the agent. Each tool is a `HeddleTool` (name, description,
//! JSON-schema parameters, `execute()`).

pub mod ask_user;
pub mod bash;
pub mod edit;
pub mod fuzzy_match;
pub mod glob;
pub mod grep;
pub mod read;
pub mod registry;
pub mod save_memory;
pub mod save_plan;
pub mod string_distance;
pub mod subagent;
pub mod task_tools;
pub mod types;
pub mod web_fetch;
pub mod write;

pub use ask_user::create_ask_user_tool;
pub use bash::create_bash_tool;
pub use edit::create_edit_tool;
pub use glob::create_glob_tool;
pub use grep::create_grep_tool;
pub use read::create_read_tool;
pub use registry::ToolRegistry;
pub use save_memory::create_save_memory_tool;
pub use save_plan::create_save_plan_tool;
pub use subagent::{create_subagent_tool, SubagentOptions};
pub use task_tools::{create_create_task_tool, create_list_tasks_tool, create_update_task_tool};
pub use types::{ExecOptions, HeddleTool};
pub use web_fetch::create_web_fetch_tool;
pub use write::create_write_tool;
