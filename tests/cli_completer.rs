use heddle::cli::completer::MentionCompleter;
use rustyline::completion::Completer;
use rustyline::history::DefaultHistory;
use rustyline::Context;
use tempfile::tempdir;

fn complete(c: &MentionCompleter, line: &str) -> (usize, Vec<String>) {
    let hist = DefaultHistory::new();
    let ctx = Context::new(&hist);
    let (start, pairs) = c.complete(line, line.len(), &ctx).unwrap();
    let strs: Vec<String> = pairs.into_iter().map(|p| p.display).collect();
    (start, strs)
}

fn setup() -> (tempfile::TempDir, MentionCompleter) {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/cli.ts"), "").unwrap();
    std::fs::write(root.join("src/config.ts"), "").unwrap();
    std::fs::write(root.join("package.json"), "{}").unwrap();
    std::fs::write(root.join("pants.toml"), "").unwrap();
    let c = MentionCompleter::new(root.to_path_buf());
    (dir, c)
}

#[test]
fn at_src_completes_entries_in_src_dir() {
    let (_d, c) = setup();
    let (_, completions) = complete(&c, "@src/");
    assert!(!completions.is_empty());
    for s in &completions {
        assert!(s.starts_with("@src/"), "got {s}");
    }
}

#[test]
fn at_src_cl_filters_to_entries_starting_with_cl() {
    let (_d, c) = setup();
    let (_, completions) = complete(&c, "@src/cl");
    assert!(completions.contains(&"@src/cli.ts".to_string()));
    assert!(!completions.contains(&"@src/config.ts".to_string()));
}

#[test]
fn at_pa_completes_entries_starting_with_pa() {
    let (_d, c) = setup();
    let (_, completions) = complete(&c, "@pa");
    assert!(completions.contains(&"@package.json".to_string()));
    assert!(completions.contains(&"@pants.toml".to_string()));
}

#[test]
fn directories_get_slash_suffix() {
    let (_d, c) = setup();
    let (_, completions) = complete(&c, "@sr");
    assert!(completions.contains(&"@src/".to_string()));
}

#[test]
fn non_at_word_returns_empty_completions() {
    let (_d, c) = setup();
    let (_, completions) = complete(&c, "hello");
    assert!(completions.is_empty());
}

#[test]
fn at_nonexistent_returns_empty_completions() {
    let (_d, c) = setup();
    let (_, completions) = complete(&c, "@nonexistent/");
    assert!(completions.is_empty());
}

#[test]
fn multiple_words_only_last_word_triggers_completion() {
    let (_d, c) = setup();
    let (start, completions) = complete(&c, "look at @src/");
    assert!(!completions.is_empty());
    assert_eq!(start, "look at ".len());
}
