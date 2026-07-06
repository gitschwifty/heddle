//! Read MEMORY.md from global and project dirs, concat with headers.

use crate::config::paths::{get_global_memory_dir, get_project_memory_dir};

fn read_memory_file(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub fn load_memory_context(project_path: Option<&str>) -> Option<String> {
    let global_path = get_global_memory_dir().join("MEMORY.md");
    let project_path_buf = get_project_memory_dir(project_path).join("MEMORY.md");

    let global = read_memory_file(&global_path);
    let project = read_memory_file(&project_path_buf);

    if global.is_none() && project.is_none() {
        return None;
    }

    let mut sections = Vec::new();
    if let Some(g) = global {
        sections.push(format!("## Global Memory\n{g}"));
    }
    if let Some(p) = project {
        sections.push(format!("## Project Memory\n{p}"));
    }
    Some(sections.join("\n\n"))
}
