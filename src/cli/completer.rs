//! @-mention path completer for rustyline.

use std::path::PathBuf;

use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

pub struct MentionCompleter {
    cwd: PathBuf,
}

impl Hinter for MentionCompleter {
    type Hint = String;
}
impl Highlighter for MentionCompleter {}
impl Validator for MentionCompleter {}
impl Helper for MentionCompleter {}

impl MentionCompleter {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

impl Completer for MentionCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[..pos];
        let last_word_start = prefix
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        let last_word = &prefix[last_word_start..];
        if !last_word.starts_with('@') {
            return Ok((pos, Vec::new()));
        }
        let partial = &last_word[1..];
        let (dir_path, file_prefix, dir_prefix) = match partial.rfind('/') {
            Some(idx) => {
                let dir_part = &partial[..=idx];
                let file = &partial[idx + 1..];
                (
                    self.cwd.join(dir_part),
                    file.to_string(),
                    dir_part.to_string(),
                )
            }
            None => (self.cwd.clone(), partial.to_string(), String::new()),
        };
        let entries = match std::fs::read_dir(&dir_path) {
            Ok(e) => e,
            Err(_) => return Ok((last_word_start, Vec::new())),
        };
        let mut candidates = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(&file_prefix) {
                continue;
            }
            let suffix = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                "/"
            } else {
                ""
            };
            let display = format!("@{dir_prefix}{name}{suffix}");
            candidates.push(Pair {
                display: display.clone(),
                replacement: display,
            });
        }
        Ok((last_word_start, candidates))
    }
}
