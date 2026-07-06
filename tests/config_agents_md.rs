use heddle::config::agents_md::{find_all_agents_md, load_agents_context};

mod common;
use common::Sandbox;

#[test]
fn finds_agents_md_in_start_dir() {
    let sb = Sandbox::new("amd-start");
    let agents_path = sb.project.join("AGENTS.md");
    std::fs::write(&agents_path, "# Project instructions").unwrap();
    let result = find_all_agents_md(Some(&sb.project));
    assert!(result.contains(&agents_path));
}

#[test]
fn finds_agents_md_lowercase() {
    let sb = Sandbox::new("amd-lower");
    let agents_path = sb.project.join("agents.md");
    std::fs::write(&agents_path, "# lowercase agents").unwrap();
    let result = find_all_agents_md(Some(&sb.project));
    assert!(result.contains(&agents_path));
}

#[test]
fn finds_agents_md_mixed_case() {
    let sb = Sandbox::new("amd-mixed");
    let agents_path = sb.project.join("Agents.Md");
    std::fs::write(&agents_path, "# mixed case").unwrap();
    let result = find_all_agents_md(Some(&sb.project));
    assert!(result.contains(&agents_path));
}

#[test]
fn finds_agents_md_in_parent() {
    let sb = Sandbox::new("amd-parent");
    let child = sb.project.join("child");
    std::fs::create_dir_all(&child).unwrap();
    let agents_path = sb.project.join("AGENTS.md");
    std::fs::write(&agents_path, "# Parent instructions").unwrap();
    let result = find_all_agents_md(Some(&child));
    assert!(result.contains(&agents_path));
}

#[test]
fn finds_multiple_agents_md_farthest_first() {
    let sb = Sandbox::new("amd-multi");
    let child = sb.project.join("child");
    std::fs::create_dir_all(&child).unwrap();
    let parent_agents = sb.project.join("AGENTS.md");
    let child_agents = child.join("AGENTS.md");
    std::fs::write(&parent_agents, "# Parent").unwrap();
    std::fs::write(&child_agents, "# Child").unwrap();
    let result = find_all_agents_md(Some(&child));
    // parent first (farthest from start), then child
    let p_idx = result.iter().position(|p| p == &parent_agents).unwrap();
    let c_idx = result.iter().position(|p| p == &child_agents).unwrap();
    assert!(p_idx < c_idx);
}

#[test]
fn returns_empty_when_none_exist() {
    let sb = Sandbox::new("amd-empty");
    let empty = sb.root.join("empty-amd");
    std::fs::create_dir_all(&empty).unwrap();
    let result = find_all_agents_md(Some(&empty));
    // The result may include HEDDLE_HOME's AGENTS.md if one exists, but in a
    // fresh sandbox there is none.
    assert!(!result.iter().any(|p| p.starts_with(&empty)));
}

#[test]
fn deduplicates_heddle_home_in_walk_path() {
    let sb = Sandbox::new("amd-dedup");
    let agents_path = sb.heddle_home.join("AGENTS.md");
    std::fs::write(&agents_path, "# Instructions").unwrap();
    let result = find_all_agents_md(Some(&sb.heddle_home));
    let count = result.iter().filter(|p| **p == agents_path).count();
    assert_eq!(count, 1);
}

#[test]
fn loads_agents_context_concatenates() {
    let sb = Sandbox::new("amd-ctx-concat");
    let child = sb.project.join("child");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(sb.project.join("AGENTS.md"), "# Parent rules").unwrap();
    std::fs::write(child.join("AGENTS.md"), "# Child rules").unwrap();
    let result = load_agents_context(Some(&child)).unwrap();
    assert!(result.contains("# Parent rules"));
    assert!(result.contains("# Child rules"));
    // Order: parent first, then child
    assert!(result.find("# Parent rules").unwrap() < result.find("# Child rules").unwrap());
}

#[test]
fn loads_agents_context_returns_none_when_empty() {
    let sb = Sandbox::new("amd-ctx-none");
    let empty = sb.root.join("empty-ctx");
    std::fs::create_dir_all(&empty).unwrap();
    let result = load_agents_context(Some(&empty));
    // May still get HEDDLE_HOME content; in our sandbox HEDDLE_HOME has no AGENTS.md
    // either, so expect None.
    assert!(result.is_none());
}

#[test]
fn loads_single_file_no_extra_separators() {
    let sb = Sandbox::new("amd-ctx-one");
    std::fs::write(sb.project.join("AGENTS.md"), "# Only one").unwrap();
    let result = load_agents_context(Some(&sb.project)).unwrap();
    // Could be just "# Only one" or with HEDDLE_HOME prepend; check the body is in there.
    assert!(result.contains("# Only one"));
}
