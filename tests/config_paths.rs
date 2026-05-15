//! Tests for config::paths. Many of these set/unset HEDDLE_HOME or chdir, so
//! all tests share a global env lock via the `Sandbox` helper.

use std::path::Path;

use heddle::config::paths::{
    encode_path, find_repo_root, get_agents_dir, get_heddle_home, get_local_heddle_dir,
    get_project_dir, get_project_sessions_dir, get_skills_dir, get_system_heddle_dir,
    walk_up_heddle_dirs,
};

mod common;
use common::Sandbox;

// ── encode_path ── (pure function — no env)

#[test]
fn encode_path_with_dashes() {
    assert_eq!(
        encode_path("/home/user/repos/heddle"),
        "-home-user-repos-heddle"
    );
}

#[test]
fn encode_path_strips_trailing_slash() {
    assert_eq!(
        encode_path("/home/user/repos/heddle/"),
        "-home-user-repos-heddle"
    );
}

#[test]
fn encode_path_single_segment() {
    assert_eq!(encode_path("/tmp"), "-tmp");
}

// ── system dir ── (pure)

#[test]
fn system_dir_is_etc_heddle() {
    assert_eq!(get_system_heddle_dir(), Path::new("/etc/heddle"));
}

// ── get_heddle_home / get_local_heddle_dir / project dirs ── (need env)

#[test]
fn heddle_home_respects_env_var() {
    let sb = Sandbox::new("paths-env");
    assert_eq!(get_heddle_home(), sb.heddle_home);
}

#[test]
fn heddle_home_resolves_relative_to_cwd() {
    let sb = Sandbox::new("paths-relative");
    std::env::set_var("HEDDLE_HOME", ".heddle-dev");
    let result = get_heddle_home();
    assert_eq!(result, sb.project.join(".heddle-dev"));
    std::env::set_var("HEDDLE_HOME", &sb.heddle_home); // restore for drop()
}

#[test]
fn local_heddle_dir_in_cwd() {
    let sb = Sandbox::new("paths-local");
    assert_eq!(get_local_heddle_dir(), sb.project.join(".heddle"));
}

#[test]
fn project_dir_under_heddle_home() {
    let sb = Sandbox::new("paths-projdir");
    let encoded = encode_path(&sb.project.to_string_lossy());
    let expected = sb.heddle_home.join("projects").join(encoded);
    assert_eq!(get_project_dir(None), expected);
}

#[test]
fn project_dir_with_explicit_path() {
    let sb = Sandbox::new("paths-projdir-arg");
    let result = get_project_dir(Some("/foo/bar"));
    assert_eq!(result, sb.heddle_home.join("projects").join("-foo-bar"));
}

#[test]
fn sessions_dir_under_project() {
    let sb = Sandbox::new("paths-sessions");
    let encoded = encode_path(&sb.project.to_string_lossy());
    let expected = sb
        .heddle_home
        .join("projects")
        .join(encoded)
        .join("sessions");
    assert_eq!(get_project_sessions_dir(None), expected);
}

#[test]
fn agents_dir_under_heddle_home() {
    let sb = Sandbox::new("paths-agents");
    assert_eq!(get_agents_dir(), sb.heddle_home.join("agents"));
}

#[test]
fn skills_dir_under_heddle_home() {
    let sb = Sandbox::new("paths-skills");
    assert_eq!(get_skills_dir(), sb.heddle_home.join("skills"));
}

// ── walk_up_heddle_dirs ──

#[test]
fn walk_up_finds_dirs_deepest_first() {
    let sb = Sandbox::new("walk-up");
    let deep = sb.home.join("a/b/c");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::create_dir_all(sb.home.join("a/.heddle")).unwrap();
    std::fs::create_dir_all(sb.home.join("a/b/.heddle")).unwrap();

    let result = walk_up_heddle_dirs(Some(&deep), Some(&sb.home));
    assert!(result[0].ends_with("a/b/.heddle"));
    assert!(result[1].ends_with("a/.heddle"));
}

#[test]
fn walk_up_includes_dir_at_home() {
    let sb = Sandbox::new("walk-athome");
    let project = sb.home.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let home_heddle = sb.home.join(".heddle");
    std::fs::create_dir_all(&home_heddle).unwrap();
    let result = walk_up_heddle_dirs(Some(&project), Some(&sb.home));
    assert!(result.iter().any(|p| p == &home_heddle));
}

#[test]
fn walk_up_includes_heddle_home_when_different() {
    let sb = Sandbox::new("walk-hh");
    // Sandbox::new already creates `heddle_home`; verify it's in the walk.
    let result = walk_up_heddle_dirs(Some(&sb.project), Some(&sb.home));
    assert!(result.iter().any(|p| p == &sb.heddle_home));
}

// ── find_repo_root ──

#[test]
fn find_repo_root_with_dot_git_dir() {
    let sb = Sandbox::new("repo-dir");
    let repo = sb.root.join("repo-dir");
    let sub = repo.join("src/lib");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let result = find_repo_root(Some(&sub));
    assert_eq!(result.as_deref(), Some(repo.as_path()));
}

#[test]
fn find_repo_root_with_dot_git_file_worktree() {
    let sb = Sandbox::new("repo-file");
    let repo = sb.root.join("repo-file");
    let sub = repo.join("src");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        repo.join(".git"),
        "gitdir: /some/path/.git/worktrees/branch",
    )
    .unwrap();
    let result = find_repo_root(Some(&sub));
    assert_eq!(result.as_deref(), Some(repo.as_path()));
}

#[test]
fn find_repo_root_returns_none_when_not_found() {
    let sb = Sandbox::new("no-repo");
    let result = find_repo_root(Some(&sb.root));
    assert!(result.is_none(), "expected None, got {result:?}");
}

// ── ensure_heddle_dirs ──

#[test]
fn ensure_heddle_dirs_creates_structure() {
    let sb = Sandbox::new("ensure");
    heddle::config::paths::ensure_heddle_dirs();
    assert!(sb.heddle_home.is_dir());
    assert!(sb.heddle_home.join("agents").is_dir());
    assert!(sb.heddle_home.join("skills").is_dir());
    let encoded = encode_path(&sb.project.to_string_lossy());
    let proj = sb.heddle_home.join("projects").join(encoded);
    assert!(proj.join("sessions").is_dir());
}

#[test]
fn ensure_heddle_dirs_is_idempotent() {
    let _sb = Sandbox::new("ensure-idem");
    heddle::config::paths::ensure_heddle_dirs();
    heddle::config::paths::ensure_heddle_dirs();
    // Just no-panic.
}
