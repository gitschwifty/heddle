use std::path::Path;

use heddle::file_history::meta::FileHistoryMeta;

mod common;
use common::Sandbox;

#[test]
fn get_or_create_returns_new_uuid_for_unknown_path() {
    let sb = Sandbox::new("fhmeta-new");
    let mut meta = FileHistoryMeta::new(&sb.root);
    let entry = meta
        .get_or_create(Path::new("/some/file.ts"), None)
        .unwrap();
    assert_eq!(entry.uuid.len(), 36);
    assert_eq!(entry.path, "/some/file.ts");
    assert_eq!(entry.versions, 0);
}

#[test]
fn get_or_create_returns_same_uuid_for_same_path() {
    let sb = Sandbox::new("fhmeta-same");
    let mut meta = FileHistoryMeta::new(&sb.root);
    let first = meta
        .get_or_create(Path::new("/some/file.ts"), None)
        .unwrap();
    let second = meta
        .get_or_create(Path::new("/some/file.ts"), None)
        .unwrap();
    assert_eq!(first.uuid, second.uuid);
}

#[test]
fn increment_version_bumps_count() {
    let sb = Sandbox::new("fhmeta-inc");
    let mut meta = FileHistoryMeta::new(&sb.root);
    let entry = meta
        .get_or_create(Path::new("/some/file.ts"), None)
        .unwrap();
    assert_eq!(entry.versions, 0);
    meta.increment_version(&entry.uuid).unwrap();
    let updated = meta
        .get_or_create(Path::new("/some/file.ts"), None)
        .unwrap();
    assert_eq!(updated.versions, 1);
    meta.increment_version(&entry.uuid).unwrap();
    let again = meta
        .get_or_create(Path::new("/some/file.ts"), None)
        .unwrap();
    assert_eq!(again.versions, 2);
}

#[test]
fn find_by_path_returns_none_for_unknown() {
    let sb = Sandbox::new("fhmeta-find");
    let mut meta = FileHistoryMeta::new(&sb.root);
    let r = meta.find_by_path(Path::new("/nonexistent.ts"));
    assert!(r.is_none());
}

#[test]
fn persists_to_meta_json() {
    let sb = Sandbox::new("fhmeta-persist");
    let mut meta = FileHistoryMeta::new(&sb.root);
    let entry = meta
        .get_or_create(Path::new("/persisted.ts"), None)
        .unwrap();
    meta.increment_version(&entry.uuid).unwrap();
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(sb.root.join("meta.json")).unwrap()).unwrap();
    assert!(raw.get(&entry.uuid).is_some());
    assert_eq!(raw[&entry.uuid]["path"], "/persisted.ts");
    assert_eq!(raw[&entry.uuid]["versions"], 1);
}

#[test]
fn reloads_existing_meta() {
    let sb = Sandbox::new("fhmeta-reload");
    let mut m1 = FileHistoryMeta::new(&sb.root);
    let entry = m1.get_or_create(Path::new("/reload.ts"), None).unwrap();
    m1.increment_version(&entry.uuid).unwrap();
    let mut m2 = FileHistoryMeta::new(&sb.root);
    let found = m2.find_by_path(Path::new("/reload.ts")).unwrap();
    assert_eq!(found.uuid, entry.uuid);
    assert_eq!(found.versions, 1);
}

#[test]
fn tracks_previous_paths() {
    let sb = Sandbox::new("fhmeta-prev");
    let mut meta = FileHistoryMeta::new(&sb.root);
    let old = meta.get_or_create(Path::new("/old/path.ts"), None).unwrap();
    meta.increment_version(&old.uuid).unwrap();
    let moved = meta
        .get_or_create(Path::new("/new/path.ts"), Some(&old.uuid))
        .unwrap();
    assert_ne!(moved.uuid, old.uuid);
    assert!(moved
        .previous_paths
        .as_ref()
        .map(|p| p.iter().any(|s| s == "/old/path.ts"))
        .unwrap_or(false));
}

#[test]
fn multiple_files_distinct_uuids() {
    let sb = Sandbox::new("fhmeta-distinct");
    let mut meta = FileHistoryMeta::new(&sb.root);
    let a = meta.get_or_create(Path::new("/a.ts"), None).unwrap();
    let b = meta.get_or_create(Path::new("/b.ts"), None).unwrap();
    assert_ne!(a.uuid, b.uuid);
}
