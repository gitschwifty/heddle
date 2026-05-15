//! `@path` mention resolver. Reads file/dir contents to inject into the prompt.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone, Default)]
pub struct InjectedFile {
    pub path: PathBuf,
    pub content: String,
    pub lines: usize,
}

#[derive(Debug, Clone, Default)]
pub struct MentionResult {
    pub cleaned_input: String,
    pub injected_files: Vec<InjectedFile>,
    pub errors: Vec<String>,
}

static MENTION_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"@(\S+)").unwrap());

fn looks_like_path(token: &str) -> bool {
    token.contains('/') || token.contains('.')
}

pub async fn resolve_mentions(input: &str, cwd: &Path) -> MentionResult {
    let mut injected_files = Vec::new();
    let mut errors = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut cleaned = input.to_string();

    let matches: Vec<String> = MENTION_RE
        .captures_iter(input)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .filter(|t| looks_like_path(t))
        .collect();

    for token in matches {
        cleaned = cleaned.replace(&format!("@{token}"), &token);
        let resolved = cwd.join(&token);
        let canonical = resolved.canonicalize().unwrap_or(resolved.clone());
        if !seen.insert(canonical.clone()) {
            continue;
        }
        match tokio::fs::metadata(&canonical).await {
            Ok(md) => {
                if md.is_dir() {
                    match tokio::fs::read_dir(&canonical).await {
                        Ok(mut rd) => {
                            let mut names = Vec::new();
                            while let Ok(Some(entry)) = rd.next_entry().await {
                                names.push(entry.file_name().to_string_lossy().into_owned());
                            }
                            let lines = names.len();
                            injected_files.push(InjectedFile {
                                path: canonical,
                                content: names.join("\n"),
                                lines,
                            });
                        }
                        Err(e) => errors.push(format!("{canonical:?}: {e}")),
                    }
                } else {
                    match tokio::fs::read_to_string(&canonical).await {
                        Ok(content) => {
                            let lines = content.split('\n').count();
                            injected_files.push(InjectedFile {
                                path: canonical,
                                content,
                                lines,
                            });
                        }
                        Err(e) => errors.push(format!("{canonical:?}: {e}")),
                    }
                }
            }
            Err(_) => errors.push(format!("Not found: {}", canonical.display())),
        }
    }

    MentionResult {
        cleaned_input: cleaned,
        injected_files,
        errors,
    }
}

pub fn build_mention_message(input: &str, files: &[InjectedFile]) -> String {
    let blocks: Vec<String> = files
        .iter()
        .map(|f| {
            let ext = f
                .path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();
            let fence = if ext.is_empty() {
                "```".to_string()
            } else {
                format!("```{ext}")
            };
            format!("`{}`:\n{fence}\n{}\n```", f.path.display(), f.content)
        })
        .collect();
    format!(
        "{input}\n\n---\nReferenced files:\n\n{}",
        blocks.join("\n\n")
    )
}
