use std::path::Path;

use heddle::checkpoints::record::{CheckpointRecord, FileChange};
use heddle::checkpoints::restore::{restore_code, restore_code_through, RestoreOutcome};
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
fn restore_code_rolls_modified_file_back_to_pre_turn_content() {
    // Simulates: turn started with file content "pre-turn"; during the
    // turn, backup_file ran (writing v{N+1}.bak with the pre-turn content)
    // and then the file was edited to "post-turn".
    let sb = Sandbox::new("cp-restore-modify");
    let file = sb.project.join("hello.txt");
    write(&file, "ancient");
    backup_file(&file, None).unwrap(); // versions: 1 (v1.bak = "ancient")
    write(&file, "pre-turn");
    backup_file(&file, None).unwrap(); // versions: 2 (v2.bak = "pre-turn")
    write(&file, "post-turn-current-on-disk");

    let mut meta = FileHistoryMeta::new(heddle::config::paths::get_file_history_dir(None));
    let entry = meta.find_by_path(&file).expect("file should be tracked");

    // The "turn" we're rewinding bumped meta from versions=1 to versions=2,
    // writing v2.bak with the pre-turn content.
    let record = CheckpointRecord::new(
        1,
        0,
        "edit hello".into(),
        vec![FileChange {
            file_path: file.to_string_lossy().to_string(),
            uuid: entry.uuid,
            version_after: 2,
        }],
    );

    let outcomes = restore_code(&record, None);
    assert_eq!(outcomes.len(), 1);
    match &outcomes[0] {
        RestoreOutcome::Restored { version, .. } => assert_eq!(*version, 2),
        other => panic!("expected Restored, got {other:?}"),
    }
    assert_eq!(read(&file), "pre-turn");
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
            uuid: String::new(),
            version_after: 0,
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
            uuid: String::new(),
            version_after: 0,
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
                version_after: 1,
            },
            FileChange {
                file_path: b.to_string_lossy().to_string(),
                uuid: String::new(),
                version_after: 0,
            },
        ],
    );

    let outcomes = restore_code(&record, None);
    assert_eq!(outcomes.len(), 2);
    assert_eq!(read(&a), "a-original");
    assert!(!b.exists());
}

#[test]
fn restore_code_through_reverts_later_checkpoints_too() {
    let sb = Sandbox::new("cp-restore-through-later");
    let a = sb.project.join("a.txt");
    let b = sb.project.join("b.txt");
    write(&a, "a-before-turn-1");
    write(&b, "b-before-turn-2");

    backup_file(&a, None).unwrap();
    write(&a, "a-after-turn-1");
    backup_file(&b, None).unwrap();
    write(&b, "b-after-turn-2");

    let mut meta = FileHistoryMeta::new(heddle::config::paths::get_file_history_dir(None));
    let a_uuid = meta.find_by_path(&a).unwrap().uuid;
    let b_uuid = meta.find_by_path(&b).unwrap().uuid;

    let checkpoint_1 = CheckpointRecord::new(
        1,
        0,
        "edit a".into(),
        vec![FileChange {
            file_path: a.to_string_lossy().to_string(),
            uuid: a_uuid,
            version_after: 1,
        }],
    );
    let checkpoint_2 = CheckpointRecord::new(
        2,
        2,
        "edit b".into(),
        vec![FileChange {
            file_path: b.to_string_lossy().to_string(),
            uuid: b_uuid,
            version_after: 1,
        }],
    );

    let outcomes = restore_code_through(&[checkpoint_1, checkpoint_2], None);
    assert_eq!(outcomes.len(), 2);
    assert_eq!(read(&a), "a-before-turn-1");
    assert_eq!(read(&b), "b-before-turn-2");
}

#[test]
fn restore_code_through_applies_same_file_checkpoints_newest_first() {
    let sb = Sandbox::new("cp-restore-through-same-file");
    let file = sb.project.join("same.txt");
    write(&file, "before-turn-1");

    backup_file(&file, None).unwrap();
    write(&file, "after-turn-1-before-turn-2");
    backup_file(&file, None).unwrap();
    write(&file, "after-turn-2");

    let mut meta = FileHistoryMeta::new(heddle::config::paths::get_file_history_dir(None));
    let uuid = meta.find_by_path(&file).unwrap().uuid;

    let checkpoint_1 = CheckpointRecord::new(
        1,
        0,
        "first edit".into(),
        vec![FileChange {
            file_path: file.to_string_lossy().to_string(),
            uuid: uuid.clone(),
            version_after: 1,
        }],
    );
    let checkpoint_2 = CheckpointRecord::new(
        2,
        2,
        "second edit".into(),
        vec![FileChange {
            file_path: file.to_string_lossy().to_string(),
            uuid,
            version_after: 2,
        }],
    );

    let outcomes = restore_code_through(&[checkpoint_1, checkpoint_2], None);
    assert_eq!(outcomes.len(), 2);
    assert_eq!(read(&file), "before-turn-1");
}
