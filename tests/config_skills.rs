use heddle::config::discovery::{DiscoveryLevel, DiscoveryResult, DiscoverySource};
use heddle::config::skills::{load_skills_from_discovery, parse_frontmatter, parse_skill_file};

mod common;
use common::Sandbox;

// ── parse_frontmatter (pure) ──

#[test]
fn parse_valid_yaml_frontmatter() {
    let content = "---\nname: My Skill\ndescription: Does things\n---\n# Body content here";
    let (fm, body) = parse_frontmatter(content);
    assert_eq!(fm.get("name"), Some(&"My Skill".to_string()));
    assert_eq!(fm.get("description"), Some(&"Does things".to_string()));
    assert_eq!(body, "# Body content here");
}

#[test]
fn parse_no_frontmatter_returns_empty() {
    let content = "# Just a markdown file\nWith content";
    let (fm, body) = parse_frontmatter(content);
    assert!(fm.is_empty());
    assert_eq!(body, content);
}

#[test]
fn parse_no_closing_delimiter_treats_as_body() {
    let content = "---\nname: incomplete\nSome body text";
    let (fm, body) = parse_frontmatter(content);
    assert!(fm.is_empty());
    assert_eq!(body, content);
}

#[test]
fn parse_empty_frontmatter_block() {
    let content = "---\n---\nBody only";
    let (fm, body) = parse_frontmatter(content);
    assert!(fm.is_empty());
    assert!(body.contains("Body only"));
}

#[test]
fn parse_trims_body_whitespace() {
    let content = "---\nkey: value\n---\n\n  Body with leading space";
    let (_fm, body) = parse_frontmatter(content);
    assert!(body.starts_with("Body"));
}

// ── parse_skill_file ──

fn level(path: std::path::PathBuf, source: DiscoverySource) -> DiscoveryLevel {
    DiscoveryLevel {
        path,
        source,
        skills: vec![],
        agents: vec![],
        config: None,
    }
}

#[test]
fn parse_skill_file_with_frontmatter() {
    let sb = Sandbox::new("skills-parse");
    let dir = sb.project.join("parse-skill");
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("test.md");
    std::fs::write(
        &file_path,
        "---\ndescription: A test skill\n---\nDo the test thing",
    )
    .unwrap();

    let lvl = level(dir.clone(), DiscoverySource::Heddle);
    let skill = parse_skill_file(&file_path, "", &lvl).unwrap();
    assert_eq!(skill.name, "test");
    assert_eq!(skill.description, "A test skill");
    assert_eq!(skill.content, "Do the test thing");
    assert_eq!(skill.source, dir);
}

#[test]
fn parse_skill_file_nested_path_with_colon() {
    let sb = Sandbox::new("skills-nested");
    let dir = sb.project.join("parse-nested");
    let subdir = dir.join("foo/bar");
    std::fs::create_dir_all(&subdir).unwrap();
    let file_path = subdir.join("baz.md");
    std::fs::write(&file_path, "nested skill content").unwrap();

    let lvl = level(dir.clone(), DiscoverySource::Heddle);
    let skill = parse_skill_file(&file_path, "foo/bar", &lvl).unwrap();
    assert_eq!(skill.name, "foo:bar:baz");
}

#[test]
fn skill_file_uses_filename_as_description_fallback() {
    let sb = Sandbox::new("skills-no-desc");
    let dir = sb.project.join("parse-no-desc");
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("deploy.md");
    std::fs::write(&file_path, "Deploy instructions").unwrap();
    let lvl = level(dir.clone(), DiscoverySource::Heddle);
    let skill = parse_skill_file(&file_path, "", &lvl).unwrap();
    assert_eq!(skill.name, "deploy");
    assert!(skill.description.contains("deploy"));
}

// ── load_skills_from_discovery ──

#[test]
fn load_skills_from_multiple_levels() {
    let sb = Sandbox::new("skills-multi");
    let deep = sb.project.join("multi-level/deep/.heddle");
    let shallow = sb.project.join("multi-level/.heddle");
    std::fs::create_dir_all(deep.join("skills")).unwrap();
    std::fs::create_dir_all(shallow.join("skills")).unwrap();
    std::fs::write(deep.join("skills/deep.md"), "deep skill").unwrap();
    std::fs::write(shallow.join("skills/shallow.md"), "shallow skill").unwrap();

    let discovery = DiscoveryResult {
        levels: vec![
            DiscoveryLevel {
                path: deep,
                source: DiscoverySource::Heddle,
                skills: vec!["deep.md".into()],
                agents: vec![],
                config: None,
            },
            DiscoveryLevel {
                path: shallow,
                source: DiscoverySource::Heddle,
                skills: vec!["shallow.md".into()],
                agents: vec![],
                config: None,
            },
        ],
    };
    let skills = load_skills_from_discovery(&discovery);
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"deep"));
    assert!(names.contains(&"shallow"));
}

#[test]
fn collision_resolution_deeper_wins() {
    let sb = Sandbox::new("skills-collision");
    let deep = sb.project.join("collision/deep/.heddle");
    let shallow = sb.project.join("collision/.heddle");
    std::fs::create_dir_all(deep.join("skills")).unwrap();
    std::fs::create_dir_all(shallow.join("skills")).unwrap();
    std::fs::write(deep.join("skills/deploy.md"), "deep deploy").unwrap();
    std::fs::write(shallow.join("skills/deploy.md"), "shallow deploy").unwrap();
    let discovery = DiscoveryResult {
        levels: vec![
            DiscoveryLevel {
                path: deep,
                source: DiscoverySource::Heddle,
                skills: vec!["deploy.md".into()],
                agents: vec![],
                config: None,
            },
            DiscoveryLevel {
                path: shallow,
                source: DiscoverySource::Heddle,
                skills: vec!["deploy.md".into()],
                agents: vec![],
                config: None,
            },
        ],
    };
    let skills = load_skills_from_discovery(&discovery);
    let deploy = skills.iter().find(|s| s.name == "deploy").unwrap();
    assert_eq!(deploy.content, "deep deploy");
}

#[test]
fn collision_heddle_wins_over_agents() {
    let sb = Sandbox::new("skills-heddle-wins");
    let heddle_dir = sb.project.join("collision2/.heddle");
    let agents_dir = sb.project.join("collision2/.agents/skills");
    std::fs::create_dir_all(heddle_dir.join("skills")).unwrap();
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(heddle_dir.join("skills/review.md"), "heddle review").unwrap();
    std::fs::write(agents_dir.join("review.md"), "agents review").unwrap();
    let discovery = DiscoveryResult {
        levels: vec![
            DiscoveryLevel {
                path: heddle_dir,
                source: DiscoverySource::Heddle,
                skills: vec!["review.md".into()],
                agents: vec![],
                config: None,
            },
            DiscoveryLevel {
                path: agents_dir,
                source: DiscoverySource::Agents,
                skills: vec!["review.md".into()],
                agents: vec![],
                config: None,
            },
        ],
    };
    let skills = load_skills_from_discovery(&discovery);
    let review = skills.iter().find(|s| s.name == "review").unwrap();
    assert_eq!(review.content, "heddle review");
}

#[test]
fn empty_discovery_returns_empty() {
    let _sb = Sandbox::new("skills-empty");
    let discovery = DiscoveryResult { levels: vec![] };
    let skills = load_skills_from_discovery(&discovery);
    assert!(skills.is_empty());
}
