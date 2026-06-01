//! edit_file tool — exact unique match with fuzzy fallback.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::fuzzy_match::{cascading_match, find_closest_match};
use super::types::{ExecOptions, HeddleTool};
use crate::file_history::backup::backup_file;

pub struct EditTool;

pub fn create_edit_tool() -> Arc<dyn HeddleTool> {
    Arc::new(EditTool)
}

#[async_trait]
impl HeddleTool for EditTool {
    fn name(&self) -> &str {
        "edit_file"
    }
    fn description(&self) -> &str {
        "Replace occurrences of old_string with new_string in a file. By default, old_string must appear exactly once (unique match). Set replace_all to true to replace every occurrence."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path":   { "type": "string", "description": "Path to the file (absolute or relative to cwd)" },
                "old_string":  { "type": "string", "description": "The text to find" },
                "new_string":  { "type": "string", "description": "The replacement text" },
                "replace_all": { "type": "boolean", "description": "Replace all occurrences" }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let file_path = match params.get("file_path").and_then(Value::as_str) {
            Some(p) => p.to_string(),
            None => return "Error: missing file_path".to_string(),
        };
        let old_string = match params.get("old_string").and_then(Value::as_str) {
            Some(s) => s.to_string(),
            None => return "Error: missing old_string".to_string(),
        };
        let new_string = match params.get("new_string").and_then(Value::as_str) {
            Some(s) => s.to_string(),
            None => return "Error: missing new_string".to_string(),
        };
        let replace_all = params
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let content = match tokio::fs::read_to_string(&file_path).await {
            Ok(c) => c,
            Err(_) => return format!("Error: File not found: {file_path}"),
        };

        if content.contains(&old_string) {
            if replace_all {
                let count = content.matches(&old_string).count();
                let updated = content.replace(&old_string, &new_string);
                if let Err(e) = backup_file(Path::new(&file_path), None) {
                    return format!("Error: backup failed: {e}");
                }
                if let Err(e) = tokio::fs::write(&file_path, updated).await {
                    return format!("Error: Could not write file: {e}");
                }
                return format!("Replaced {count} occurrences in {file_path}");
            }
            // Unique-match check
            let first_idx = content.find(&old_string).unwrap();
            let second_idx = content[first_idx + 1..].find(&old_string);
            if second_idx.is_some() {
                return format!(
                    "Error: old_string is not unique in {file_path} (found multiple matches). Use replace_all: true to replace all, or provide more context."
                );
            }
            let updated = content.replacen(&old_string, &new_string, 1);
            if let Err(e) = backup_file(Path::new(&file_path), None) {
                return format!("Error: backup failed: {e}");
            }
            if let Err(e) = tokio::fs::write(&file_path, updated).await {
                return format!("Error: Could not write file: {e}");
            }
            return format!("Applied edit to {file_path}");
        }

        // Exact match failed — try fuzzy
        if let Some(fuzzy) = cascading_match(&content, &old_string) {
            if fuzzy.level > 0 {
                let level_names = [
                    "exact",
                    "whitespace-normalized",
                    "indent-flexible",
                    "line-fuzzy",
                ];
                let mut updated = String::new();
                updated.push_str(&content[..fuzzy.start_index]);
                updated.push_str(&new_string);
                updated.push_str(&content[fuzzy.start_index + fuzzy.matched_text.len()..]);
                if let Err(e) = backup_file(Path::new(&file_path), None) {
                    return format!("Error: backup failed: {e}");
                }
                if let Err(e) = tokio::fs::write(&file_path, updated).await {
                    return format!("Error: Could not write file: {e}");
                }
                return format!(
                    "Applied edit to {file_path} ({} match)",
                    level_names[fuzzy.level as usize]
                );
            }
        }

        if let Some(closest) = find_closest_match(&content, &old_string) {
            return format!(
                "Error: old_string not found in {file_path}. Closest match near line {}:\n{}",
                closest.line, closest.snippet
            );
        }
        format!("Error: old_string not found in {file_path}")
    }
}
