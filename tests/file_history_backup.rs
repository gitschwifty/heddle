use heddle::config::paths::get_file_history_dir;
use heddle::file_history::backup::backup_file;

mod common;
use common::Sandbox;

fn project_arg(sb: &Sandbox) -> String {
    sb.project.to_string_lossy().into_owned()
}

#[test]
fn skips_when_file_missing() {
    let sb = Sandbox::new("fhbackup-missing");
    let p = project_arg(&sb);
    backup_file(&sb.project.join("nonexistent.txt"), Some(&p)).unwrap();
    // file-history dir should not exist (no backups created)
    let dir = get_file_history_dir(Some(&p));
    assert!(!dir.exists() || std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) == 0);
}

#[test]
fn creates_v1_bak_on_first_backup() {
    let sb = Sandbox::new("fhbackup-v1");
    let p = project_arg(&sb);
    let file_path = sb.project.join("source.txt");
    std::fs::write(&file_path, "hello world").unwrap();
    backup_file(&file_path, Some(&p)).unwrap();
    let base = get_file_history_dir(Some(&p));
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(base.join("meta.json")).unwrap()).unwrap();
    let uuids: Vec<&str> = raw
        .as_object()
        .unwrap()
        .keys()
        .map(|s| s.as_str())
        .collect();
    assert_eq!(uuids.len(), 1);
    let uuid = uuids[0];
    let backup = base.join(uuid).join("v1.bak");
    let content = std::fs::read_to_string(&backup).unwrap();
    assert_eq!(content, "hello world");
}

#[test]
fn deduplicates_identical_content() {
    let sb = Sandbox::new("fhbackup-dedup");
    let p = project_arg(&sb);
    let file_path = sb.project.join("dup.txt");
    std::fs::write(&file_path, "same content").unwrap();
    backup_file(&file_path, Some(&p)).unwrap();
    backup_file(&file_path, Some(&p)).unwrap();
    let base = get_file_history_dir(Some(&p));
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(base.join("meta.json")).unwrap()).unwrap();
    let uuid = raw.as_object().unwrap().keys().next().unwrap().clone();
    let files: Vec<String> = std::fs::read_dir(base.join(&uuid))
        .unwrap()
        .filter_map(|e| e.ok().map(|x| x.file_name().to_string_lossy().into_owned()))
        .filter(|n| n.ends_with(".bak"))
        .collect();
    assert_eq!(files.len(), 1);
}

#[test]
fn creates_v2_when_content_changes() {
    let sb = Sandbox::new("fhbackup-change");
    let p = project_arg(&sb);
    let file_path = sb.project.join("changing.txt");
    std::fs::write(&file_path, "version 1").unwrap();
    backup_file(&file_path, Some(&p)).unwrap();
    std::fs::write(&file_path, "version 2").unwrap();
    backup_file(&file_path, Some(&p)).unwrap();
    let base = get_file_history_dir(Some(&p));
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(base.join("meta.json")).unwrap()).unwrap();
    let uuid = raw.as_object().unwrap().keys().next().unwrap().clone();
    let mut files: Vec<String> = std::fs::read_dir(base.join(&uuid))
        .unwrap()
        .filter_map(|e| e.ok().map(|x| x.file_name().to_string_lossy().into_owned()))
        .filter(|n| n.ends_with(".bak"))
        .collect();
    files.sort();
    assert_eq!(files, vec!["v1.bak", "v2.bak"]);
    let v1 = std::fs::read_to_string(base.join(&uuid).join("v1.bak")).unwrap();
    let v2 = std::fs::read_to_string(base.join(&uuid).join("v2.bak")).unwrap();
    assert_eq!(v1, "version 1");
    assert_eq!(v2, "version 2");
}

#[test]
fn nested_file_creates_parent_dirs() {
    let sb = Sandbox::new("fhbackup-nested");
    let p = project_arg(&sb);
    let file_path = sb.project.join("deep/nested/file.txt");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, "nested content").unwrap();
    backup_file(&file_path, Some(&p)).unwrap();
    let base = get_file_history_dir(Some(&p));
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(base.join("meta.json")).unwrap()).unwrap();
    let uuid = raw.as_object().unwrap().keys().next().unwrap().clone();
    assert!(base.join(&uuid).join("v1.bak").exists());
}

#[test]
fn same_file_uses_same_uuid_across_calls() {
    let sb = Sandbox::new("fhbackup-consistent");
    let p = project_arg(&sb);
    let file_path = sb.project.join("consistent.txt");
    std::fs::write(&file_path, "v1").unwrap();
    backup_file(&file_path, Some(&p)).unwrap();
    std::fs::write(&file_path, "v2").unwrap();
    backup_file(&file_path, Some(&p)).unwrap();
    std::fs::write(&file_path, "v3").unwrap();
    backup_file(&file_path, Some(&p)).unwrap();
    let base = get_file_history_dir(Some(&p));
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(base.join("meta.json")).unwrap()).unwrap();
    let map = raw.as_object().unwrap();
    assert_eq!(map.len(), 1);
    let uuid = map.keys().next().unwrap();
    assert_eq!(map[uuid]["versions"], 3);
}
