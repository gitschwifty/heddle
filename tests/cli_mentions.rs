use heddle::cli::mentions::{build_mention_message, resolve_mentions, InjectedFile};
use std::path::PathBuf;
use tempfile::tempdir;

fn setup() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/index.ts"), "console.log('hello');").unwrap();
    std::fs::write(root.join("config.toml"), "model = \"test\"").unwrap();
    std::fs::write(root.join("a.ts"), "const a = 1;").unwrap();
    std::fs::write(root.join("b.ts"), "const b = 2;").unwrap();
    dir
}

#[tokio::test]
async fn basic_file_mention_injects_content_and_cleans_input() {
    let d = setup();
    let result = resolve_mentions("look at @src/index.ts", d.path()).await;
    assert_eq!(result.cleaned_input, "look at src/index.ts");
    assert_eq!(result.injected_files.len(), 1);
    let f = &result.injected_files[0];
    assert!(f.path.ends_with("src/index.ts"));
    assert_eq!(f.content, "console.log('hello');");
    assert_eq!(f.lines, 1);
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn directory_mention_injects_listing() {
    let d = setup();
    let result = resolve_mentions("check @src/", d.path()).await;
    assert_eq!(result.cleaned_input, "check src/");
    assert_eq!(result.injected_files.len(), 1);
    assert!(result.injected_files[0].content.contains("index.ts"));
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn multiple_mentions_both_injected() {
    let d = setup();
    let result = resolve_mentions("compare @a.ts and @b.ts", d.path()).await;
    assert_eq!(result.cleaned_input, "compare a.ts and b.ts");
    assert_eq!(result.injected_files.len(), 2);
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn duplicate_path_only_injected_once() {
    let d = setup();
    let result = resolve_mentions("@a.ts @a.ts", d.path()).await;
    assert_eq!(result.injected_files.len(), 1);
}

#[tokio::test]
async fn non_existent_path_populates_errors() {
    let d = setup();
    let result = resolve_mentions("@missing.ts", d.path()).await;
    assert!(result.injected_files.is_empty());
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].contains("Not found"));
    assert!(result.errors[0].contains("missing.ts"));
}

#[tokio::test]
async fn no_mentions_returns_input_unchanged() {
    let d = setup();
    let result = resolve_mentions("just regular text", d.path()).await;
    assert_eq!(result.cleaned_input, "just regular text");
    assert!(result.injected_files.is_empty());
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn non_path_at_is_not_treated_as_mention() {
    let d = setup();
    let result = resolve_mentions("hello @username", d.path()).await;
    assert_eq!(result.cleaned_input, "hello @username");
    assert!(result.injected_files.is_empty());
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn path_with_dot_is_treated_as_mention() {
    let d = setup();
    let result = resolve_mentions("@config.toml", d.path()).await;
    assert_eq!(result.injected_files.len(), 1);
    assert!(result.injected_files[0].content.contains("model"));
}

#[tokio::test]
async fn path_with_slash_is_treated_as_mention() {
    let d = setup();
    let result = resolve_mentions("@src/index.ts", d.path()).await;
    assert_eq!(result.injected_files.len(), 1);
}

#[tokio::test]
async fn mixed_valid_and_invalid_paths() {
    let d = setup();
    let result = resolve_mentions("@a.ts @fake.ts", d.path()).await;
    assert_eq!(result.injected_files.len(), 1);
    assert_eq!(result.errors.len(), 1);
}

#[test]
fn build_mention_message_single_file() {
    let result = build_mention_message(
        "look at src/index.ts",
        &[InjectedFile {
            path: PathBuf::from("/tmp/src/index.ts"),
            content: "console.log('hello');".into(),
            lines: 1,
        }],
    );
    assert!(result.contains("look at src/index.ts"));
    assert!(result.contains("---"));
    assert!(result.contains("Referenced files:"));
    assert!(result.contains("`/tmp/src/index.ts`:"));
    assert!(result.contains("```ts"));
    assert!(result.contains("console.log('hello');"));
}

#[test]
fn build_mention_message_multiple_files() {
    let result = build_mention_message(
        "compare",
        &[
            InjectedFile {
                path: PathBuf::from("/tmp/a.ts"),
                content: "const a = 1;".into(),
                lines: 1,
            },
            InjectedFile {
                path: PathBuf::from("/tmp/b.md"),
                content: "# Hello".into(),
                lines: 1,
            },
        ],
    );
    assert!(result.contains("`/tmp/a.ts`:"));
    assert!(result.contains("```ts"));
    assert!(result.contains("`/tmp/b.md`:"));
    assert!(result.contains("```md"));
}

#[test]
fn build_mention_message_file_extension_detection() {
    let result = build_mention_message(
        "test",
        &[InjectedFile {
            path: PathBuf::from("/tmp/app.js"),
            content: "x".into(),
            lines: 1,
        }],
    );
    assert!(result.contains("```js"));
}

#[test]
fn build_mention_message_no_extension_uses_empty_fence() {
    let result = build_mention_message(
        "test",
        &[InjectedFile {
            path: PathBuf::from("/tmp/Makefile"),
            content: "all:".into(),
            lines: 1,
        }],
    );
    assert!(result.contains("```\n"));
}
