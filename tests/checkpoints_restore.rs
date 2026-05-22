use std::path::Path;

use heddle::checkpoints::record::{CheckpointRecord, FileChange};
use heddle::checkpoints::restore::{restore_code, RestoreOutcome};
use heddle::file_history::backup::backup_file;
use heddle::file_history::meta::FileHistoryMeta;

mod common;
use common::Sandbox;

fn write(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
}
fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn restore_code_rolls_modified_file_back_to_version_before() {
    let sb = Sandbox::new("cp-restore-modify");
    let file = sb.project.join("hello.txt");
    write(&file, "v1");
    backup_file(&file, None).unwrap(); // versions: 1 (snapshot of v1)
    write(&file, "v2");
    backup_file(&file, None).unwrap(); // versions: 2 (snapshot of v2)
    write(&file, "v3-current-on-disk");

    // Locate the uuid the file-history layer assigned.
    let mut meta = FileHistoryMeta::new(heddle::config::paths::get_file_history_dir(None));
    let entry = meta.find_by_path(&file).expect("file should be tracked");

    let record = CheckpointRecord::new(
        1,
        0,
        "edit hello".into(),
        vec![FileChange {
            file_path: file.to_string_lossy().to_string(),
            uuid: entry.uuid,
            version_before: 1,
            version_after: 2,
        }],
    );

    let outcomes = restore_code(&record, None);
    assert_eq!(outcomes.len(), 1);
    match &outcomes[0] {
        RestoreOutcome::Restored { version, .. } => assert_eq!(*version, 1),
        other => panic!("expected Restored, got {other:?}"),
    }
    assert_eq!(read(&file), "v1");
}

#[test]
fn restore_code_removes_file_created_during_turn() {
    let sb = Sandbox::new("cp-restore-new");
    let file = sb.project.join("created-this-turn.txt");
    write(&file, "freshly added");

    let record = CheckpointRecord::new(
        2,
        0,
        "create".into(),
        vec![FileChange {
            file_path: file.to_string_lossy().to_string(),
            uuid: "fake-uuid".into(),
            version_before: 0,
            version_after: 1,
        }],
    );

    let outcomes = restore_code(&record, None);
    match &outcomes[0] {
        RestoreOutcome::Removed { file_path } => {
            assert_eq!(file_path, &file.to_string_lossy().to_string())
        }
        other => panic!("expected Removed, got {other:?}"),
    }
    assert!(!file.exists());
}

#[test]
fn restore_code_idempotent_when_new_file_already_gone() {
    let sb = Sandbox::new("cp-restore-idem");
    let file = sb.project.join("never-there.txt");
    // Don't create the file — simulates a second /rewind to the same checkpoint.

    let record = CheckpointRecord::new(
        2,
        0,
        "create".into(),
        vec![FileChange {
            file_path: file.to_string_lossy().to_string(),
            uuid: "fake-uuid".into(),
            version_before: 0,
            version_after: 1,
        }],
    );

    let outcomes = restore_code(&record, None);
    assert!(matches!(outcomes[0], RestoreOutcome::Removed { .. }));
}

#[test]
fn restore_code_reports_failure_for_missing_backup() {
    let sb = Sandbox::new("cp-restore-missing");
    let file = sb.project.join("orphan.txt");
    write(&file, "current");

    let record = CheckpointRecord::new(
        3,
        0,
        "edit".into(),
        vec![FileChange {
            file_path: file.to_string_lossy().to_string(),
            uuid: "uuid-with-no-backups".into(),
            version_before: 1,
            version_after: 2,
        }],
    );

    let outcomes = restore_code(&record, None);
    assert!(matches!(outcomes[0], RestoreOutcome::Failed { .. }));
}

#[test]
fn restore_code_handles_multiple_changes() {
    let sb = Sandbox::new("cp-restore-multi");
    let a = sb.project.join("a.txt");
    let b = sb.project.join("b.txt");
    write(&a, "a-original");
    backup_file(&a, None).unwrap();
    write(&a, "a-edited");
    write(&b, "b-fresh-this-turn");

    let mut meta = FileHistoryMeta::new(heddle::config::paths::get_file_history_dir(None));
    let a_uuid = meta.find_by_path(&a).unwrap().uuid;

    let record = CheckpointRecord::new(
        4,
        0,
        "two-file edit".into(),
        vec![
            FileChange {
                file_path: a.to_string_lossy().to_string(),
                uuid: a_uuid,
                version_before: 1,
                version_after: 1,
            },
            FileChange {
                file_path: b.to_string_lossy().to_string(),
                uuid: "new-uuid".into(),
                version_before: 0,
                version_after: 1,
            },
        ],
    );

    let outcomes = restore_code(&record, None);
    assert_eq!(outcomes.len(), 2);
    assert_eq!(read(&a), "a-original");
    assert!(!b.exists());
}
