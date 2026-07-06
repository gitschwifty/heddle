//! Tests for the test-only `Sandbox` helper (sets HEDDLE_HOME + cwd, cleans up
//! on Drop).

mod common;
use common::Sandbox;

#[test]
fn creates_sandbox_directories() {
    let sb = Sandbox::new("dirs");
    assert!(sb.heddle_home.exists());
    assert!(sb.project.exists());
    drop(sb);
}

#[test]
fn sets_heddle_home_to_sandbox_heddle_home() {
    let sb = Sandbox::new("env");
    let actual = std::env::var("HEDDLE_HOME").unwrap();
    assert_eq!(
        std::path::PathBuf::from(actual).canonicalize().ok(),
        sb.heddle_home.canonicalize().ok()
    );
}

#[test]
fn changes_cwd_to_sandbox_project_dir() {
    let sb = Sandbox::new("cwd");
    let cwd = std::env::current_dir().unwrap();
    assert_eq!(cwd.canonicalize().ok(), sb.project.canonicalize().ok());
}

#[test]
fn drop_restores_env() {
    let orig = std::env::var("HEDDLE_HOME").ok();
    {
        let _sb = Sandbox::new("restore-env");
        assert!(std::env::var("HEDDLE_HOME").is_ok());
    }
    assert_eq!(std::env::var("HEDDLE_HOME").ok(), orig);
}

#[test]
fn drop_restores_cwd() {
    let orig = {
        let sb = Sandbox::new("restore-cwd");
        sb.orig_cwd().clone()
    };
    assert_eq!(std::env::current_dir().unwrap(), orig);
}

#[test]
fn drop_removes_sandbox_files() {
    let root = {
        let sb = Sandbox::new("remove");
        sb.root.clone()
    };
    assert!(!root.exists());
}

#[test]
fn unique_dirs_per_call() {
    let a = Sandbox::new("unique");
    let a_root = a.root.clone();
    drop(a);
    let b = Sandbox::new("unique");
    assert_ne!(a_root, b.root);
}
