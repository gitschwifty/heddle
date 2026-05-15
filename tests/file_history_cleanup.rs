use heddle::config::paths::get_file_history_dir;
use heddle::file_history::cleanup::{run_file_history_cleanup, CleanupConfig};

mod common;
use common::Sandbox;

fn project_arg(sb: &Sandbox) -> String {
    sb.project.to_string_lossy().into_owned()
}

#[test]
fn no_removal_when_under_max_size() {
    let sb = Sandbox::new("fhcleanup-under");
    let p = project_arg(&sb);
    let base = get_file_history_dir(Some(&p));
    let uuid_dir = base.join("fake-uuid-1");
    std::fs::create_dir_all(&uuid_dir).unwrap();
    for i in 1..=5 {
        std::fs::write(uuid_dir.join(format!("v{i}.bak")), format!("content {i}")).unwrap();
    }
    let stats = run_file_history_cleanup(CleanupConfig {
        max_size: 100 * 1024 * 1024,
        project_path: Some(p),
    });
    assert_eq!(stats.files_removed, 0);
}

#[test]
fn respects_max_size_oldest_first() {
    let sb = Sandbox::new("fhcleanup-maxsize");
    let p = project_arg(&sb);
    let base = get_file_history_dir(Some(&p));
    let uuid_dir = base.join("fake-uuid-2");
    std::fs::create_dir_all(&uuid_dir).unwrap();
    let big = "x".repeat(1000);
    std::fs::write(uuid_dir.join("v1.bak"), &big).unwrap();
    std::fs::write(uuid_dir.join("v2.bak"), &big).unwrap();
    std::fs::write(uuid_dir.join("v3.bak"), &big).unwrap();
    let stats = run_file_history_cleanup(CleanupConfig {
        max_size: 2500,
        project_path: Some(p),
    });
    assert!(stats.files_removed >= 1);
    assert!(stats.bytes_freed >= 1000);
    // v3 (newest) should survive
    assert!(uuid_dir.join("v3.bak").exists());
}

#[test]
fn empty_dir_returns_zero_stats() {
    let sb = Sandbox::new("fhcleanup-empty");
    let p = project_arg(&sb);
    let stats = run_file_history_cleanup(CleanupConfig {
        max_size: 100 * 1024 * 1024,
        project_path: Some(p),
    });
    assert_eq!(stats.files_removed, 0);
    assert_eq!(stats.bytes_freed, 0);
}

#[test]
fn no_op_when_base_dir_missing() {
    let sb = Sandbox::new("fhcleanup-nodir");
    let p = sb.root.join("nonexistent").to_string_lossy().into_owned();
    let stats = run_file_history_cleanup(CleanupConfig {
        max_size: 100 * 1024 * 1024,
        project_path: Some(p),
    });
    assert_eq!(stats.files_removed, 0);
}
