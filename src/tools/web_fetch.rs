//! web_fetch tool — converts HTML to readable text via html2text, 10s timeout, 50KB cap.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::types::{ExecOptions, HeddleTool};

pub struct WebFetchTool;

const MAX_LENGTH: usize = 50_000;
const RENDER_WIDTH: usize = 80;

pub fn create_web_fetch_tool() -> Arc<dyn HeddleTool> {
    Arc::new(WebFetchTool)
}

#[async_trait]
impl HeddleTool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Fetch the contents of a URL. HTML pages are converted to readable text."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to fetch" }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, params: Value, _options: ExecOptions) -> String {
        let url = match params.get("url").and_then(Value::as_str) {
            Some(u) => u.to_string(),
            None => return "Error: missing url".to_string(),
        };
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return "Error: URL must start with http:// or https://".to_string();
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let response = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return "Error: Request timed out after 10s".to_string(),
            Err(e) => return format!("Error: {e}"),
        };
        let status = response.status();
        if !status.is_success() {
            return format!(
                "Error: HTTP {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("")
            );
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();
        if !content_type.contains("text")
            && !content_type.contains("json")
            && !content_type.contains("xml")
        {
            return format!("Error: Non-text content type: {content_type}");
        }
        let text = match response.text().await {
            Ok(t) => t,
            Err(e) => return format!("Error: {e}"),
        };

        let mut rendered = if content_type.contains("html") {
            html2text::from_read(text.as_bytes(), RENDER_WIDTH)
        } else {
            text
        };
        if rendered.len() > MAX_LENGTH {
            rendered.truncate(MAX_LENGTH);
        }
        rendered
    }
}
