use heddle::memory::loader::load_memory_context;

mod common;
use common::Sandbox;

#[test]
fn returns_none_when_no_memory_files() {
    let sb = Sandbox::new("memloader-none");
    let result = load_memory_context(Some(&sb.project.to_string_lossy()));
    assert!(result.is_none());
}

#[test]
fn loads_global_memory_only() {
    let sb = Sandbox::new("memloader-global");
    let global_dir = sb.heddle_home.join("memory");
    std::fs::create_dir_all(&global_dir).unwrap();
    std::fs::write(global_dir.join("MEMORY.md"), "Global notes here").unwrap();

    let result = load_memory_context(Some(&sb.project.to_string_lossy())).unwrap();
    assert!(result.contains("## Global Memory"));
    assert!(result.contains("Global notes here"));
    assert!(!result.contains("## Project Memory"));
}

#[test]
fn loads_project_memory_only() {
    let sb = Sandbox::new("memloader-project");
    let project_path = "/test-project";
    let proj_mem_dir = sb
        .heddle_home
        .join("projects")
        .join("-test-project")
        .join("memory");
    std::fs::create_dir_all(&proj_mem_dir).unwrap();
    std::fs::write(proj_mem_dir.join("MEMORY.md"), "Project notes here").unwrap();

    let result = load_memory_context(Some(project_path)).unwrap();
    assert!(result.contains("## Project Memory"));
    assert!(result.contains("Project notes here"));
    assert!(!result.contains("## Global Memory"));
}

#[test]
fn loads_both_concatenated_global_first() {
    let sb = Sandbox::new("memloader-both");
    let global_dir = sb.heddle_home.join("memory");
    std::fs::create_dir_all(&global_dir).unwrap();
    std::fs::write(global_dir.join("MEMORY.md"), "Global stuff").unwrap();

    let project_path = "/test-project";
    let proj_mem_dir = sb
        .heddle_home
        .join("projects")
        .join("-test-project")
        .join("memory");
    std::fs::create_dir_all(&proj_mem_dir).unwrap();
    std::fs::write(proj_mem_dir.join("MEMORY.md"), "Project stuff").unwrap();

    let result = load_memory_context(Some(project_path)).unwrap();
    let g = result.find("## Global Memory").unwrap();
    let p = result.find("## Project Memory").unwrap();
    assert!(g < p);
    assert!(result.contains("Global stuff"));
    assert!(result.contains("Project stuff"));
}

#[test]
fn empty_memory_files_treated_as_none() {
    let sb = Sandbox::new("memloader-empty");
    let global_dir = sb.heddle_home.join("memory");
    std::fs::create_dir_all(&global_dir).unwrap();
    std::fs::write(global_dir.join("MEMORY.md"), "").unwrap();
    let result = load_memory_context(Some(&sb.project.to_string_lossy()));
    assert!(result.is_none());
}
