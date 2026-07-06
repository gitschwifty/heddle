use heddle::config::discovery::{resolve_discovery, DiscoverySource};

mod common;
use common::Sandbox;

#[test]
fn finds_heddle_in_cwd() {
    let sb = Sandbox::new("discovery-cwd");
    let heddle_dir = sb.project.join(".heddle");
    std::fs::create_dir_all(&heddle_dir).unwrap();
    let result = resolve_discovery(Some(&sb.project), Some(&sb.home));
    let heddle: Vec<_> = result
        .levels
        .iter()
        .filter(|l| matches!(l.source, DiscoverySource::Heddle))
        .collect();
    assert!(!heddle.is_empty());
    assert!(heddle.iter().any(|l| l.path == heddle_dir));
}

#[test]
fn walks_up_finds_multiple_levels() {
    let sb = Sandbox::new("discovery-walk");
    let deep = sb.project.join("a/b/c");
    std::fs::create_dir_all(&deep).unwrap();
    let project_heddle = sb.project.join(".heddle");
    std::fs::create_dir_all(&project_heddle).unwrap();
    let mid_heddle = sb.project.join("a/.heddle");
    std::fs::create_dir_all(&mid_heddle).unwrap();
    let result = resolve_discovery(Some(&deep), Some(&sb.home));
    let heddle_paths: Vec<_> = result
        .levels
        .iter()
        .filter(|l| matches!(l.source, DiscoverySource::Heddle))
        .map(|l| l.path.clone())
        .collect();
    let mid_idx = heddle_paths.iter().position(|p| p == &mid_heddle).unwrap();
    let proj_idx = heddle_paths
        .iter()
        .position(|p| p == &project_heddle)
        .unwrap();
    assert!(mid_idx < proj_idx, "mid should appear before project");
}

#[test]
fn finds_dot_agents_skills_at_repo_root() {
    let sb = Sandbox::new("discovery-agents");
    std::fs::create_dir_all(sb.project.join(".git")).unwrap();
    let agents_skills = sb.project.join(".agents/skills");
    std::fs::create_dir_all(&agents_skills).unwrap();
    std::fs::write(agents_skills.join("test.md"), "# Test skill").unwrap();
    let result = resolve_discovery(Some(&sb.project), Some(&sb.home));
    let agents_levels: Vec<_> = result
        .levels
        .iter()
        .filter(|l| matches!(l.source, DiscoverySource::Agents))
        .collect();
    assert_eq!(agents_levels.len(), 1);
    assert_eq!(agents_levels[0].path, agents_skills);
}

#[test]
fn does_not_throw_on_missing_dirs() {
    let sb = Sandbox::new("discovery-missing");
    let result = resolve_discovery(Some(&sb.project), Some(&sb.home));
    let _ = result.levels;
}

#[test]
fn deepest_heddle_first() {
    let sb = Sandbox::new("discovery-prio");
    let deep = sb.project.join("deep-priority");
    std::fs::create_dir_all(deep.join(".heddle/skills")).unwrap();
    std::fs::create_dir_all(sb.project.join(".heddle/skills")).unwrap();
    std::fs::write(deep.join(".heddle/skills/deep.md"), "deep skill").unwrap();
    std::fs::write(
        sb.project.join(".heddle/skills/shallow.md"),
        "shallow skill",
    )
    .unwrap();
    let result = resolve_discovery(Some(&deep), Some(&sb.home));
    let first_heddle = result
        .levels
        .iter()
        .find(|l| matches!(l.source, DiscoverySource::Heddle))
        .unwrap();
    assert_eq!(first_heddle.path, deep.join(".heddle"));
}

#[test]
fn includes_heddle_home_level() {
    let sb = Sandbox::new("discovery-hh");
    std::fs::create_dir_all(sb.heddle_home.join("skills")).unwrap();
    std::fs::write(sb.heddle_home.join("skills/global.md"), "global").unwrap();
    let result = resolve_discovery(Some(&sb.project), Some(&sb.home));
    let heddle_levels: Vec<_> = result
        .levels
        .iter()
        .filter(|l| matches!(l.source, DiscoverySource::Heddle))
        .collect();
    assert!(heddle_levels.iter().any(|l| l.path == sb.heddle_home));
}

#[test]
fn finds_config_toml_in_levels() {
    let sb = Sandbox::new("discovery-config");
    let dir = sb.project.join("config-test");
    let heddle_dir = dir.join(".heddle");
    std::fs::create_dir_all(&heddle_dir).unwrap();
    std::fs::write(heddle_dir.join("config.toml"), r#"model = "test""#).unwrap();
    let result = resolve_discovery(Some(&dir), Some(&sb.home));
    let level = result.levels.iter().find(|l| l.path == heddle_dir).unwrap();
    assert_eq!(
        level.config.as_deref(),
        Some(heddle_dir.join("config.toml").as_path())
    );
}

#[test]
fn handles_dot_git_file_worktree() {
    let sb = Sandbox::new("discovery-worktree");
    let worktree = sb.root.join("worktree-project");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join(".git"), "gitdir: /some/path").unwrap();
    let agents_skills = worktree.join(".agents/skills");
    std::fs::create_dir_all(&agents_skills).unwrap();
    std::fs::write(agents_skills.join("wt.md"), "worktree skill").unwrap();
    let result = resolve_discovery(Some(&worktree), Some(&sb.home));
    let agents_levels: Vec<_> = result
        .levels
        .iter()
        .filter(|l| matches!(l.source, DiscoverySource::Agents))
        .collect();
    assert_eq!(agents_levels.len(), 1);
}

#[test]
fn nonexistent_start_dir_returns_levels() {
    let sb = Sandbox::new("discovery-nonexistent");
    let result = resolve_discovery(Some(&sb.root.join("nonexistent")), Some(&sb.home));
    let _ = result.levels;
}
