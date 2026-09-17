//! Conservative interactive policy floor. Shell classification is descriptive;
//! every shell command requires approval, including unknown and compound input.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionCategory {
    Shell,
    DestructiveFilesystem,
    VersionControl,
    PackageOrSystem,
    CredentialOrConfig,
    BulkEdit,
    UnknownTool,
    Tool,
}

pub fn classify(tool: &str, args: Option<&Value>) -> ActionCategory {
    if tool == "bash" {
        let command = args
            .and_then(|a| a.get("command"))
            .and_then(Value::as_str)
            .unwrap_or("");
        // These labels never grant permission. Wrappers, substitutions, aliases,
        // scripts and unrecognized syntax still fall back to the shell gate.
        let words: Vec<_> = command
            .split(|c: char| c.is_whitespace() || ";|&()".contains(c))
            .filter(|word| !word.is_empty())
            .collect();
        if words
            .iter()
            .any(|word| matches!(*word, "rm" | "rmdir" | "truncate" | "dd"))
            || command.contains('>')
        {
            return ActionCategory::DestructiveFilesystem;
        }
        if words.contains(&"git") {
            return ActionCategory::VersionControl;
        }
        if words.iter().any(|word| {
            matches!(
                *word,
                "sudo" | "chmod" | "chown" | "brew" | "apt" | "npm" | "pip" | "bun" | "cargo"
            )
        }) {
            return ActionCategory::PackageOrSystem;
        }
        return ActionCategory::Shell;
    }
    if matches!(tool, "write_file" | "edit_file") {
        let path = args
            .and_then(|a| a.get("file_path").or_else(|| a.get("path")))
            .and_then(Value::as_str)
            .unwrap_or("");
        if path.split('/').any(|part| {
            part.starts_with(".env")
                || matches!(
                    part,
                    ".ssh"
                        | ".aws"
                        | ".gnupg"
                        | ".heddle"
                        | ".config"
                        | ".git"
                        | ".npmrc"
                        | ".netrc"
                )
                || part.contains("credentials")
                || part.ends_with(".pem")
                || part.ends_with(".key")
        }) {
            return ActionCategory::CredentialOrConfig;
        }
        if args
            .and_then(|a| a.get("replace_all"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            return ActionCategory::BulkEdit;
        }
    }
    if matches!(
        tool,
        "read_file"
            | "glob"
            | "grep"
            | "ask_user"
            | "write_file"
            | "edit_file"
            | "save_memory"
            | "web_fetch"
    ) {
        ActionCategory::Tool
    } else {
        ActionCategory::UnknownTool
    }
}

pub fn requires_approval(category: ActionCategory) -> bool {
    !matches!(category, ActionCategory::Tool)
}
