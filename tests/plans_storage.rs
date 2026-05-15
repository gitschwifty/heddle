use heddle::plans::storage::{get_plans_dir, list_plans, load_plan, save_plan, PlanMeta};

mod common;
use common::Sandbox;

#[test]
fn plans_dir_under_project() {
    let _sb = Sandbox::new("plans-dir");
    let p = get_plans_dir(Some("/some/project"));
    assert!(p.to_string_lossy().contains("projects"));
    assert!(p.ends_with("plans"));
}

#[test]
fn save_writes_file_and_returns_path() {
    let _sb = Sandbox::new("plans-save");
    let p = save_plan(
        "my-plan",
        "# My Plan\n\nDo stuff.",
        PlanMeta {
            model: Some("gpt-4"),
            session_id: Some("sess-1"),
        },
        None,
    )
    .unwrap();
    let s = p.to_string_lossy();
    assert!(s.contains("my-plan.md"));
    let raw = std::fs::read_to_string(&p).unwrap();
    assert!(raw.contains("# My Plan"));
    assert!(raw.contains("Do stuff."));
}

#[test]
fn save_writes_frontmatter() {
    let _sb = Sandbox::new("plans-fm");
    let p = save_plan(
        "frontmatter-test",
        "Plan body here.",
        PlanMeta {
            model: Some("claude-3"),
            session_id: Some("sess-fm"),
        },
        None,
    )
    .unwrap();
    let raw = std::fs::read_to_string(&p).unwrap();
    assert!(raw.starts_with("---\n"));
    assert!(raw.contains("model: claude-3"));
    assert!(raw.contains("session_id: sess-fm"));
    assert!(raw.contains("created: "));
}

#[test]
fn save_without_model_omits_field() {
    let _sb = Sandbox::new("plans-nomodel");
    let p = save_plan(
        "no-model",
        "Content.",
        PlanMeta {
            model: None,
            session_id: Some("sess-nm"),
        },
        None,
    )
    .unwrap();
    let raw = std::fs::read_to_string(&p).unwrap();
    assert!(!raw.contains("model:"));
    assert!(raw.contains("session_id: sess-nm"));
}

#[test]
fn load_roundtrips_content_and_metadata() {
    let _sb = Sandbox::new("plans-rt");
    save_plan(
        "roundtrip",
        "Roundtrip body content.",
        PlanMeta {
            model: Some("test-model"),
            session_id: Some("sess-rt"),
        },
        None,
    )
    .unwrap();
    let plan = load_plan("roundtrip", None).unwrap();
    assert_eq!(plan.name, "roundtrip");
    assert!(plan.content.contains("Roundtrip body content."));
    assert_eq!(
        plan.meta.get("model").map(String::as_str),
        Some("test-model")
    );
    assert_eq!(
        plan.meta.get("session_id").map(String::as_str),
        Some("sess-rt")
    );
    assert!(plan.meta.get("created").is_some());
}

#[test]
fn load_returns_none_for_nonexistent() {
    let _sb = Sandbox::new("plans-loadnone");
    assert!(load_plan("does-not-exist", None).is_none());
}

#[test]
fn list_returns_saved_plans_with_previews() {
    let _sb = Sandbox::new("plans-list");
    let project_path = "/test/list-project";
    save_plan(
        "list-alpha",
        "Alpha plan first line.\nSecond line.",
        PlanMeta {
            model: None,
            session_id: Some("sess-la"),
        },
        Some(project_path),
    )
    .unwrap();
    save_plan(
        "list-beta",
        "Beta plan first line.",
        PlanMeta {
            model: Some("m"),
            session_id: Some("sess-lb"),
        },
        Some(project_path),
    )
    .unwrap();
    let plans = list_plans(Some(project_path));
    assert!(plans.len() >= 2);
    let alpha = plans.iter().find(|p| p.name == "list-alpha").unwrap();
    assert!(alpha.preview.contains("Alpha plan first line."));
    let beta = plans.iter().find(|p| p.name == "list-beta").unwrap();
    assert!(beta.created.starts_with("20"));
}

#[test]
fn list_empty_when_no_plans() {
    let _sb = Sandbox::new("plans-listempty");
    assert!(list_plans(Some("/nonexistent/project")).is_empty());
}

#[test]
fn name_sanitized_against_path_traversal() {
    let _sb = Sandbox::new("plans-traversal");
    let p = save_plan(
        "../../../etc/passwd",
        "evil content",
        PlanMeta {
            model: None,
            session_id: Some("sess-evil"),
        },
        None,
    )
    .unwrap();
    let plans_dir = get_plans_dir(None);
    assert!(p.starts_with(&plans_dir));
    assert!(!p.to_string_lossy().contains(".."));
}

#[test]
fn name_with_slashes_sanitized() {
    let _sb = Sandbox::new("plans-slash");
    let p = save_plan(
        "foo/bar/baz",
        "slash content",
        PlanMeta {
            model: None,
            session_id: Some("sess-slash"),
        },
        None,
    )
    .unwrap();
    let plans_dir = get_plans_dir(None);
    assert!(p.starts_with(&plans_dir));
    let filename = p
        .strip_prefix(&plans_dir)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(!filename.contains('/'));
}
