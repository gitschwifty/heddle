//! Bounded, range-aware `read_file` tool.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::types::{ExecOptions, HeddleTool};

const DEFAULT_MAX_RETURNED_LINES: usize = 200;

pub struct ReadTool;

pub fn create_read_tool() -> Arc<dyn HeddleTool> {
    Arc::new(ReadTool)
}

fn line_param(params: &Value, name: &str) -> Result<Option<usize>, String> {
    let Some(value) = params.get(name) else {
        return Ok(None);
    };
    let Some(value) = value.as_u64() else {
        return Err(format!("Error: {name} must be a positive integer"));
    };
    let value = usize::try_from(value)
        .map_err(|_| format!("Error: {name} is too large for this system"))?;
    if value == 0 {
        return Err(format!("Error: {name} must be at least 1"));
    }
    Ok(Some(value))
}

fn format_range_result(
    content: &str,
    start_line: usize,
    end_line: usize,
) -> Result<String, String> {
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    if total_lines == 0 {
        return Err("Error: cannot request a line range from an empty file".into());
    }
    if start_line > total_lines {
        return Err(format!(
            "Error: start_line {start_line} is beyond the file's last line ({total_lines})"
        ));
    }
    if end_line < start_line {
        return Err(format!(
            "Error: end_line {end_line} must be greater than or equal to start_line {start_line}"
        ));
    }

    if end_line > total_lines {
        return Err(format!(
            "Error: end_line {end_line} is beyond the file's last line ({total_lines})"
        ));
    }

    let returned_end = end_line.min(start_line + DEFAULT_MAX_RETURNED_LINES - 1);
    let truncated = returned_end < end_line;
    let returned = lines[start_line - 1..returned_end].join("\n");
    let requested = format!("{start_line}-{end_line}");
    let returned_span = format!("{start_line}-{returned_end}");
    let mut metadata = format!(
        "[read_file metadata: total_lines={total_lines}; total_bytes={}; requested_lines={requested}; returned_lines={returned_span}; truncated={truncated}",
        content.len()
    );
    if truncated {
        metadata.push_str(&format!(
            "; next_start_line={}; continue with read_file using start_line={}",
            returned_end + 1,
            returned_end + 1
        ));
    }
    metadata.push(']');
    Ok(format!("{metadata}\n{returned}"))
}

#[async_trait]
impl HeddleTool for ReadTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a file. For large files, request a 1-based inclusive line range; default reads return at most 200 lines with continuation metadata. Set full_file=true to deliberately return the entire file."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Path to the file (absolute or relative to cwd)" },
                "start_line": { "type": "integer", "minimum": 1, "description": "First line to return, inclusive and 1-based. Defaults to line 1." },
                "end_line": { "type": "integer", "minimum": 1, "description": "Last line to return, inclusive. Defaults to the last line." },
                "full_file": { "type": "boolean", "description": "Set true to deliberately return the entire file. Cannot be combined with line ranges." }
            },
            "required": ["file_path"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let file_path = match params
            .get("file_path")
            .or_else(|| params.get("path"))
            .and_then(Value::as_str)
        {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => return "Error: missing file_path".to_string(),
        };
        let full_file = match params.get("full_file") {
            Some(value) => match value.as_bool() {
                Some(value) => value,
                None => return "Error: full_file must be a boolean".to_string(),
            },
            None => false,
        };
        let start_line = match line_param(&params, "start_line") {
            Ok(value) => value,
            Err(error) => return error,
        };
        let end_line = match line_param(&params, "end_line") {
            Ok(value) => value,
            Err(error) => return error,
        };
        if full_file && (start_line.is_some() || end_line.is_some()) {
            return "Error: full_file cannot be combined with start_line or end_line".to_string();
        }

        let content = match tokio::fs::read_to_string(&file_path).await {
            Ok(content) => content,
            Err(_) => return format!("Error: Could not read file: {file_path}"),
        };
        if full_file {
            return content;
        }

        let lines: Vec<&str> = content.lines().collect();
        let has_explicit_range = start_line.is_some() || end_line.is_some();
        if !has_explicit_range && lines.len() <= DEFAULT_MAX_RETURNED_LINES {
            return content;
        }
        let start_line = start_line.unwrap_or(1);
        let end_line = end_line.unwrap_or(lines.len());
        format_range_result(&content, start_line, end_line).unwrap_or_else(|error| error)
    }
}
