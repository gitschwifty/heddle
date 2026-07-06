use heddle::config::paths::get_file_history_dir;
use heddle::file_history::meta::FileHistoryMeta;
use heddle::file_history::restore::{list_backups, restore_backup};

mod common;
use common::Sandbox;

fn project_arg(sb: &Sandbox) -> String {
    sb.project.to_string_lossy().into_owned()
}

#[test]
fn lists_backups_newest_first() {
    let sb = Sandbox::new("fhrestore-list");
    let p = project_arg(&sb);
    let file_path = sb.project.join("test.txt");
    let base = get_file_history_dir(Some(&p));
    std::fs::create_dir_all(&base).unwrap();
    let mut meta = FileHistoryMeta::new(&base);
    let entry = meta.get_or_create(&file_path, None).unwrap();
    let uuid_dir = base.join(&entry.uuid);
    std::fs::create_dir_all(&uuid_dir).unwrap();
    std::fs::write(uuid_dir.join("v1.bak"), "old").unwrap();
    meta.increment_version(&entry.uuid).unwrap();
    std::fs::write(uuid_dir.join("v2.bak"), "middle").unwrap();
    meta.increment_version(&entry.uuid).unwrap();
    std::fs::write(uuid_dir.join("v3.bak"), "newest").unwrap();
    meta.increment_version(&entry.uuid).unwrap();

    let backups = list_backups(&file_path, Some(&p));
    assert_eq!(backups.len(), 3);
    assert_eq!(backups[0].version, 3);
    assert_eq!(backups[1].version, 2);
    assert_eq!(backups[2].version, 1);
}

#[test]
fn empty_for_nonexistent_file() {
    let sb = Sandbox::new("fhrestore-empty");
    let p = project_arg(&sb);
    let backups = list_backups(&sb.project.join("nope.txt"), Some(&p));
    assert!(backups.is_empty());
}

#[test]
fn backup_entries_include_size() {
    let sb = Sandbox::new("fhrestore-size");
    let p = project_arg(&sb);
    let file_path = sb.project.join("sized.txt");
    let base = get_file_history_dir(Some(&p));
    std::fs::create_dir_all(&base).unwrap();
    let mut meta = FileHistoryMeta::new(&base);
    let entry = meta.get_or_create(&file_path, None).unwrap();
    let uuid_dir = base.join(&entry.uuid);
    std::fs::create_dir_all(&uuid_dir).unwrap();
    std::fs::write(uuid_dir.join("v1.bak"), "twelve chars").unwrap();
    meta.increment_version(&entry.uuid).unwrap();
    let backups = list_backups(&file_path, Some(&p));
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].size, 12);
}

#[test]
fn restores_backup_to_original_path() {
    let sb = Sandbox::new("fhrestore-do");
    let p = project_arg(&sb);
    let file_path = sb.project.join("restore-me.txt");
    let base = get_file_history_dir(Some(&p));
    std::fs::create_dir_all(&base).unwrap();
    let mut meta = FileHistoryMeta::new(&base);
    let entry = meta.get_or_create(&file_path, None).unwrap();
    let uuid_dir = base.join(&entry.uuid);
    std::fs::create_dir_all(&uuid_dir).unwrap();
    std::fs::write(uuid_dir.join("v1.bak"), "restored content").unwrap();
    meta.increment_version(&entry.uuid).unwrap();
    std::fs::write(&file_path, "current content").unwrap();
    let result = restore_backup(&file_path, 1, Some(&p));
    assert!(result.contains("Restored"));
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "restored content");
}

#[test]
fn error_for_nonexistent_version() {
    let sb = Sandbox::new("fhrestore-novers");
    let p = project_arg(&sb);
    let result = restore_backup(&sb.project.join("missing.txt"), 99, Some(&p));
    let lc = result.to_lowercase();
    assert!(lc.contains("not found") || lc.contains("error"));
}
