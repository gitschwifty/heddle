use heddle::tools::save_memory::create_save_memory_tool;
use heddle::tools::types::ExecOptions;
use serde_json::json;
use tempfile::tempdir;

mod common;
use common::Sandbox;

#[tokio::test]
async fn creates_memory_md_if_missing() {
    let _sb = Sandbox::new("savemem-create");
    let dir = tempdir().unwrap();
    let tool = create_save_memory_tool(dir.path().to_path_buf());

    let result = tool
        .execute(
            json!({ "content": "Remember this" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("Saved"), "got: {result}");

    let content = std::fs::read_to_string(dir.path().join("MEMORY.md")).unwrap();
    assert!(content.contains("Remember this"), "got: {content}");
}

#[tokio::test]
async fn appends_timestamped_section_to_existing() {
    let _sb = Sandbox::new("savemem-append");
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("MEMORY.md"), "# Existing\n\nOld content\n").unwrap();
    let tool = create_save_memory_tool(dir.path().to_path_buf());

    tool.execute(json!({ "content": "New memory" }), ExecOptions::default())
        .await;

    let content = std::fs::read_to_string(dir.path().join("MEMORY.md")).unwrap();
    assert!(content.contains("Old content"));
    assert!(content.contains("New memory"));
    // ISO timestamp header e.g. "## 2026-05-17T..."
    let ts_re = regex::Regex::new(r"## \d{4}-\d{2}-\d{2}T").unwrap();
    assert!(
        ts_re.is_match(&content),
        "no timestamp header in: {content}"
    );
}

#[tokio::test]
async fn respects_global_vs_project_scope() {
    let _sb = Sandbox::new("savemem-scope");
    let proj_dir = tempdir().unwrap();
    let global_dir = tempdir().unwrap();
    let global_mem_dir = global_dir.path().join("memory");
    std::fs::create_dir_all(&global_mem_dir).unwrap();
    std::env::set_var("HEDDLE_HOME", global_dir.path());

    let tool = create_save_memory_tool(proj_dir.path().to_path_buf());
    tool.execute(
        json!({ "content": "Project note", "scope": "project" }),
        ExecOptions::default(),
    )
    .await;
    tool.execute(
        json!({ "content": "Global note", "scope": "global" }),
        ExecOptions::default(),
    )
    .await;

    let proj_content = std::fs::read_to_string(proj_dir.path().join("MEMORY.md")).unwrap();
    assert!(proj_content.contains("Project note"));
    assert!(!proj_content.contains("Global note"));

    let global_content = std::fs::read_to_string(global_mem_dir.join("MEMORY.md")).unwrap();
    assert!(global_content.contains("Global note"));
    assert!(!global_content.contains("Project note"));
}

#[tokio::test]
async fn default_scope_is_project() {
    let _sb = Sandbox::new("savemem-default");
    let dir = tempdir().unwrap();
    let tool = create_save_memory_tool(dir.path().to_path_buf());

    tool.execute(
        json!({ "content": "Default scope note" }),
        ExecOptions::default(),
    )
    .await;

    let content = std::fs::read_to_string(dir.path().join("MEMORY.md")).unwrap();
    assert!(content.contains("Default scope note"));
}
