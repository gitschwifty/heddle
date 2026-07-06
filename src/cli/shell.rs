//! Shell-out helpers for the `!` and `!!` REPL prefixes.

use std::io::Write;
use std::process::Stdio;

use tokio::process::Command;

use crate::types::{Message, UserMessage};

#[derive(Debug, Clone)]
pub struct ShellResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub async fn run_shell(command: &str) -> ShellResult {
    let output = Command::new("bash")
        .args(["-c", command])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;
    match output {
        Ok(o) => ShellResult {
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            exit_code: o.status.code().unwrap_or(-1),
        },
        Err(e) => ShellResult {
            stdout: String::new(),
            stderr: e.to_string(),
            exit_code: -1,
        },
    }
}

pub fn print_shell_result(result: &ShellResult) {
    if !result.stdout.is_empty() {
        let _ = std::io::stdout().write_all(result.stdout.as_bytes());
    }
    if !result.stderr.is_empty() {
        let _ = std::io::stderr().write_all(result.stderr.as_bytes());
    }
    if result.exit_code != 0 {
        eprintln!("Exit code: {}", result.exit_code);
    }
}

pub fn format_shell_for_context(command: &str, result: &ShellResult) -> Message {
    let mut content = format!("Shell output from `{command}`:\n```\n{}```", result.stdout);
    if !result.stderr.is_empty() {
        content.push_str(&format!("\nstderr:\n```\n{}```", result.stderr));
    }
    Message::User(UserMessage { content })
}
