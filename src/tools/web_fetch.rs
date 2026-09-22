//! web_fetch tool — converts HTML to readable text via html2text, 10s timeout, 50KB cap.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};
use url::{Host, Url};

use super::types::{ExecOptions, HeddleTool};

pub struct WebFetchTool {
    options: WebFetchOptions,
    network_allowed: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WebFetchOptions {
    pub allow_private_addresses: bool,
}

const MAX_LENGTH: usize = 50_000;
const RENDER_WIDTH: usize = 80;

pub fn create_web_fetch_tool() -> Arc<dyn HeddleTool> {
    Arc::new(WebFetchTool {
        options: WebFetchOptions {
            allow_private_addresses: allow_private_addresses_from_env(),
        },
        network_allowed: true,
    })
}

pub fn create_web_fetch_tool_with_options(options: WebFetchOptions) -> Arc<dyn HeddleTool> {
    create_web_fetch_tool_with_profile(options, super::bash::SandboxProfile::Developer)
}

/// Enforce the session's network boundary even when permission checks are bypassed.
pub fn create_web_fetch_tool_with_profile(
    options: WebFetchOptions,
    profile: super::bash::SandboxProfile,
) -> Arc<dyn HeddleTool> {
    Arc::new(WebFetchTool {
        options,
        network_allowed: profile == super::bash::SandboxProfile::Developer,
    })
}

fn allow_private_addresses_from_env() -> bool {
    std::env::var("HEDDLE_WEB_FETCH_ALLOW_PRIVATE_ADDRESSES")
        .ok()
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

fn is_restricted_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(ip)) => is_restricted_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_restricted_ip(IpAddr::V6(ip)),
        Some(Host::Domain(host)) => {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            host == "localhost" || host.ends_with(".localhost")
        }
        None => true,
    }
}

fn is_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            if let Some(ipv4) = ip.to_ipv4_mapped() {
                return is_restricted_ip(IpAddr::V4(ipv4));
            }
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
    }
}

#[async_trait]
impl HeddleTool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        if self.network_allowed {
            "Fetch the contents of a URL. HTML pages are converted to readable text."
        } else {
            "Web fetching is denied by the strict network capability policy."
        }
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
        // Deny before URL processing, client construction, DNS, or proxy discovery.
        if !self.network_allowed {
            return "Error: network capability denied (strict profile; web_fetch)".to_string();
        }
        let raw_url = match params.get("url").and_then(Value::as_str) {
            Some(u) => u,
            None => return "Error: missing url".to_string(),
        };
        let url = match Url::parse(raw_url) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => u,
            _ => return "Error: URL must be a valid http:// or https:// URL".to_string(),
        };
        if !url.username().is_empty() || url.password().is_some() {
            return "Error: URL credentials are not allowed".to_string();
        }
        if url.host_str().is_none() {
            return "Error: URL must include a host".to_string();
        }
        if !self.options.allow_private_addresses && is_restricted_host(&url) {
            return "Error: URL host resolves to a loopback/private address. Set web_fetch_allow_private_addresses = true only for trusted local workflows.".to_string();
        }

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build();
        let client = match client {
            Ok(client) => client,
            Err(_) => return "Error: could not construct web_fetch client".to_string(),
        };
        let response = match client.get(url).send().await {
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
        let content_type_lc = content_type.to_ascii_lowercase();
        if !content_type_lc.contains("text")
            && !content_type_lc.contains("json")
            && !content_type_lc.contains("xml")
        {
            return format!("Error: Non-text content type: {content_type}");
        }

        let mut body = Vec::with_capacity(MAX_LENGTH.min(8192));
        let mut stream = response.bytes_stream();
        while body.len() < MAX_LENGTH {
            let Some(chunk) = stream.next().await else {
                break;
            };
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => return format!("Error: {e}"),
            };
            let remaining = MAX_LENGTH - body.len();
            if chunk.len() > remaining {
                body.extend_from_slice(&chunk[..remaining]);
                break;
            }
            body.extend_from_slice(&chunk);
        }
        let text = String::from_utf8_lossy(&body).into_owned();

        let rendered = if content_type_lc.contains("html") {
            html2text::from_read(text.as_bytes(), RENDER_WIDTH)
        } else {
            text
        };
        rendered
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn mapped_ipv4_uses_ipv4_policy() {
        for addr in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "0.0.0.0",
            "8.8.8.8",
        ] {
            let ipv4: std::net::Ipv4Addr = addr.parse().unwrap();
            assert_eq!(
                is_restricted_ip(IpAddr::V6(ipv4.to_ipv6_mapped())),
                is_restricted_ip(IpAddr::V4(ipv4)),
                "mapped policy mismatch for {addr}"
            );
        }
    }

    #[tokio::test]
    async fn strict_denies_before_processing_arguments() {
        let tool = create_web_fetch_tool_with_profile(
            WebFetchOptions {
                allow_private_addresses: true,
            },
            super::super::bash::SandboxProfile::Strict,
        );
        for args in [json!({}), json!({"url": "http://127.0.0.1/"})] {
            assert_eq!(
                tool.execute(args, ExecOptions::default()).await,
                "Error: network capability denied (strict profile; web_fetch)"
            );
        }
    }

    #[tokio::test]
    async fn developer_still_validates_urls_without_a_request() {
        let tool = create_web_fetch_tool_with_options(WebFetchOptions::default());
        assert_eq!(
            tool.execute(json!({}), ExecOptions::default()).await,
            "Error: missing url"
        );
    }
}
