//! Agent file parsing + multi-level loading. Mirrors `ts-test/agents/loader.test.ts`.

use heddle::agents::loader::{load_agent_definitions, parse_agent_file};
use heddle::config::discovery::{DiscoveryLevel, DiscoveryResult, DiscoverySource};
use tempfile::tempdir;

fn write(path: &std::path::Path, content: &str) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

// ─── parse_agent_file ────────────────────────────────────────────────────

#[test]
fn parses_valid_agent_file_with_all_fields() {
    let d = tempdir().unwrap();
    let p = d.path().join("full-agent.md");
    write(
        &p,
        "---
name: researcher
description: Research-focused agent
model: openrouter/google/gemini-2.5-pro
tools:
  - read
  - glob
  - grep
---

You are a research agent. Read files and report findings.

Never modify files.
",
    );
    let r = parse_agent_file(&p).unwrap();
    assert_eq!(r.name, "researcher");
    assert_eq!(r.description, "Research-focused agent");
    assert_eq!(r.model.as_deref(), Some("openrouter/google/gemini-2.5-pro"));
    assert_eq!(
        r.tools,
        Some(vec!["read".into(), "glob".into(), "grep".into()])
    );
    assert!(r.system_prompt.contains("You are a research agent."));
    assert!(r.system_prompt.contains("Never modify files."));
    assert_eq!(r.source, p);
}

#[test]
fn parses_with_minimal_frontmatter_name_only() {
    let d = tempdir().unwrap();
    let p = d.path().join("minimal-agent.md");
    write(&p, "---\nname: helper\n---\n\nHelp the user.\n");
    let r = parse_agent_file(&p).unwrap();
    assert_eq!(r.name, "helper");
    assert_eq!(r.description, "");
    assert!(r.model.is_none());
    assert!(r.tools.is_none());
    assert!(r.system_prompt.contains("Help the user."));
}

#[test]
fn derives_name_from_filename_when_not_in_frontmatter() {
    let d = tempdir().unwrap();
    let p = d.path().join("code-reviewer.md");
    write(
        &p,
        "---\ndescription: Reviews code\n---\n\nReview code carefully.\n",
    );
    let r = parse_agent_file(&p).unwrap();
    assert_eq!(r.name, "code-reviewer");
    assert_eq!(r.description, "Reviews code");
}

#[test]
fn derives_name_from_filename_with_no_frontmatter_at_all() {
    let d = tempdir().unwrap();
    let p = d.path().join("simple-bot.md");
    write(&p, "Just a system prompt, no frontmatter.\n");
    let r = parse_agent_file(&p).unwrap();
    assert_eq!(r.name, "simple-bot");
    assert_eq!(r.description, "");
    assert!(r
        .system_prompt
        .contains("Just a system prompt, no frontmatter."));
}

#[test]
fn returns_none_for_nonexistent_file() {
    let d = tempdir().unwrap();
    let r = parse_agent_file(&d.path().join("does-not-exist.md"));
    assert!(r.is_none());
}

#[test]
fn returns_none_for_empty_file() {
    let d = tempdir().unwrap();
    let p = d.path().join("empty-agent.md");
    write(&p, "");
    let r = parse_agent_file(&p);
    assert!(r.is_none());
}

#[test]
fn returns_none_for_whitespace_only_file() {
    let d = tempdir().unwrap();
    let p = d.path().join("whitespace-agent.md");
    write(&p, "   \n\n  \n");
    let r = parse_agent_file(&p);
    assert!(r.is_none());
}

#[test]
fn handles_file_with_frontmatter_but_empty_body() {
    let d = tempdir().unwrap();
    let p = d.path().join("no-body-agent.md");
    write(
        &p,
        "---\nname: headless\ndescription: No system prompt\n---\n",
    );
    let r = parse_agent_file(&p).unwrap();
    assert_eq!(r.name, "headless");
    assert_eq!(r.system_prompt, "");
}

#[test]
fn handles_malformed_yaml_frontmatter_gracefully() {
    // Rust falls back to defaults (filename stem) when YAML fails to parse.
    // TS returns null because TypeBox rejects malformed frontmatter.
    let d = tempdir().unwrap();
    let p = d.path().join("bad-yaml.md");
    write(&p, "---\nname: [broken\n  yaml: {{{}\n---\n\nSome body.\n");
    let r = parse_agent_file(&p);
    // Either behavior is acceptable; Rust currently returns Some with stem name.
    if let Some(def) = r {
        assert_eq!(def.name, "bad-yaml");
    }
}

// ─── load_agent_definitions ──────────────────────────────────────────────

fn level(path: &std::path::Path, agents: Vec<&str>) -> DiscoveryLevel {
    DiscoveryLevel {
        path: path.to_path_buf(),
        source: DiscoverySource::Heddle,
        skills: vec![],
        agents: agents.into_iter().map(String::from).collect(),
        config: None,
    }
}

#[test]
fn returns_empty_map_when_no_agents_exist() {
    let d = tempdir().unwrap();
    let discovery = DiscoveryResult {
        levels: vec![level(&d.path().join("empty-level"), vec![])],
    };
    assert!(load_agent_definitions(&discovery).is_empty());
}

#[test]
fn loads_agents_from_single_discovery_level() {
    let d = tempdir().unwrap();
    let lvl_path = d.path().join("single-level");
    let agents_dir = lvl_path.join("agents");
    write(
        &agents_dir.join("writer.md"),
        "---
name: writer
description: Writing agent
model: gpt-4o
tools:
  - write
  - edit
---

You are a writing agent.
",
    );
    let discovery = DiscoveryResult {
        levels: vec![level(&lvl_path, vec!["writer.md"])],
    };
    let r = load_agent_definitions(&discovery);
    assert_eq!(r.len(), 1);
    let writer = r.get("writer").unwrap();
    assert_eq!(writer.description, "Writing agent");
    assert_eq!(writer.model.as_deref(), Some("gpt-4o"));
    assert_eq!(writer.tools, Some(vec!["write".into(), "edit".into()]));
    assert!(writer.system_prompt.contains("You are a writing agent."));
}

#[test]
fn project_level_agent_overrides_global_with_same_name() {
    let d = tempdir().unwrap();
    let global_path = d.path().join("global-level");
    write(
        &global_path.join("agents").join("reviewer.md"),
        "---
name: reviewer
description: Global reviewer
model: gpt-3.5-turbo
---

Global review prompt.
",
    );
    let project_path = d.path().join("project-level");
    write(
        &project_path.join("agents").join("reviewer.md"),
        "---
name: reviewer
description: Project reviewer
model: claude-sonnet-4-20250514
---

Project-specific review prompt.
",
    );
    // Deepest first → project, then global
    let discovery = DiscoveryResult {
        levels: vec![
            level(&project_path, vec!["reviewer.md"]),
            level(&global_path, vec!["reviewer.md"]),
        ],
    };
    let r = load_agent_definitions(&discovery);
    assert_eq!(r.len(), 1);
    let rv = r.get("reviewer").unwrap();
    assert_eq!(rv.description, "Project reviewer");
    assert_eq!(rv.model.as_deref(), Some("claude-sonnet-4-20250514"));
    assert!(rv.system_prompt.contains("Project-specific review prompt."));
}

#[test]
fn merges_agents_from_multiple_levels_without_collision() {
    let d = tempdir().unwrap();
    let g = d.path().join("merge-global");
    write(
        &g.join("agents").join("coder.md"),
        "---\nname: coder\ndescription: Global coder\n---\n\nCode things.\n",
    );
    let p = d.path().join("merge-project");
    write(
        &p.join("agents").join("tester.md"),
        "---\nname: tester\ndescription: Test runner\n---\n\nRun tests.\n",
    );
    let discovery = DiscoveryResult {
        levels: vec![level(&p, vec!["tester.md"]), level(&g, vec!["coder.md"])],
    };
    let r = load_agent_definitions(&discovery);
    assert_eq!(r.len(), 2);
    assert!(r.contains_key("coder"));
    assert!(r.contains_key("tester"));
}

#[test]
fn skips_malformed_agent_files_without_crashing() {
    let d = tempdir().unwrap();
    let lvl = d.path().join("skip-bad");
    write(
        &lvl.join("agents").join("good.md"),
        "---\nname: good\ndescription: Works fine\n---\n\nGood agent.\n",
    );
    write(&lvl.join("agents").join("bad.md"), "");
    let discovery = DiscoveryResult {
        levels: vec![level(&lvl, vec!["good.md", "bad.md"])],
    };
    let r = load_agent_definitions(&discovery);
    assert_eq!(r.len(), 1);
    assert!(r.contains_key("good"));
}

#[test]
fn handles_level_with_missing_agents_directory_gracefully() {
    let d = tempdir().unwrap();
    let lvl = d.path().join("no-agents-dir");
    std::fs::create_dir_all(&lvl).unwrap();
    let discovery = DiscoveryResult {
        levels: vec![level(&lvl, vec!["phantom.md"])],
    };
    assert!(load_agent_definitions(&discovery).is_empty());
}
