use std::collections::HashMap;
use std::path::Path;

use heddle::checkpoints::diff::{compute_changes, snapshot_from_meta, snapshot_meta};
use heddle::checkpoints::record::FileChange;
use heddle::file_history::meta::FileHistoryMeta;

mod common;
use common::Sandbox;

#[test]
fn compute_changes_empty_when_snapshots_match() {
    let mut before = HashMap::new();
    before.insert("u1".into(), ("/a.rs".into(), 2u32));
    let after = before.clone();
    assert!(compute_changes(&before, &after).is_empty());
}

#[test]
fn compute_changes_detects_version_bump() {
    let mut before = HashMap::new();
    before.insert("u1".into(), ("/a.rs".into(), 2u32));
    let mut after = HashMap::new();
    after.insert("u1".into(), ("/a.rs".into(), 3u32));

    let changes = compute_changes(&before, &after);
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0],
        FileChange {
            file_path: "/a.rs".into(),
            uuid: "u1".into(),
            version_after: 3,
        }
    );
}

#[test]
fn compute_changes_records_new_uuid_with_full_version_after() {
    let before = HashMap::new();
    let mut after = HashMap::new();
    after.insert("u-new".into(), ("/freshly-created.rs".into(), 1u32));

    let changes = compute_changes(&before, &after);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].version_after, 1);
    assert_eq!(changes[0].uuid, "u-new");
}

#[test]
fn compute_changes_ignores_unchanged_uuids() {
    let mut before = HashMap::new();
    before.insert("untouched".into(), ("/a.rs".into(), 5u32));
    before.insert("changed".into(), ("/b.rs".into(), 1u32));
    let mut after = before.clone();
    after.insert("changed".into(), ("/b.rs".into(), 2u32));

    let changes = compute_changes(&before, &after);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].uuid, "changed");
}

#[test]
fn compute_changes_sorts_by_file_path() {
    let before = HashMap::new();
    let mut after = HashMap::new();
    after.insert("u1".into(), ("/z.rs".into(), 1u32));
    after.insert("u2".into(), ("/a.rs".into(), 1u32));
    after.insert("u3".into(), ("/m.rs".into(), 1u32));

    let changes = compute_changes(&before, &after);
    let paths: Vec<&str> = changes.iter().map(|c| c.file_path.as_str()).collect();
    assert_eq!(paths, vec!["/a.rs", "/m.rs", "/z.rs"]);
}

#[test]
fn snapshot_from_meta_roundtrips_known_entries() {
    let sb = Sandbox::new("cp-diff-snap");
    let mut meta = FileHistoryMeta::new(&sb.root);
    let a = meta.get_or_create(Path::new("/a.rs"), None).unwrap();
    meta.increment_version(&a.uuid).unwrap();
    let b = meta.get_or_create(Path::new("/b.rs"), None).unwrap();
    meta.increment_version(&b.uuid).unwrap();
    meta.increment_version(&b.uuid).unwrap();

    let snap = snapshot_from_meta(&mut meta);
    assert_eq!(snap.get(&a.uuid).map(|(_, v)| *v), Some(1));
    assert_eq!(snap.get(&b.uuid).map(|(_, v)| *v), Some(2));
}

#[test]
fn snapshot_meta_returns_empty_when_no_history() {
    // Sandbox swaps HEDDLE_HOME, so snapshot_meta reads an empty file-history dir.
    let _sb = Sandbox::new("cp-diff-empty");
    let snap = snapshot_meta(None);
    assert!(snap.is_empty());
}
