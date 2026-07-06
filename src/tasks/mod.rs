//! Task tracking — persistent across sessions.

pub mod storage;
pub mod types;

pub use storage::{
    create_task, format_tasks_summary, get_tasks_path, load_tasks, save_tasks, update_task,
    TaskUpdate,
};
pub use types::{Task, TaskStatus};
