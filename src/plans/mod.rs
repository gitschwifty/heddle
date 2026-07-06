//! Saved-plan markdown storage with YAML frontmatter.

pub mod storage;

pub use storage::{get_plans_dir, list_plans, load_plan, save_plan, PlanSummary};
