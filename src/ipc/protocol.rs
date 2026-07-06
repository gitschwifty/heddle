//! Protocol version compatibility check.

use once_cell::sync::Lazy;

pub static PROTOCOL_VERSION: Lazy<String> = Lazy::new(|| {
    if let Ok(v) = std::env::var("HEDDLE_PROTOCOL_VERSION") {
        return v.trim().to_string();
    }
    // Build-time include from the repo root.
    let raw = include_str!("../../PROTOCOL_VERSION");
    raw.trim().to_string()
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatLevel {
    Exact,
    Patch,
    Minor,
    Major,
    Incompatible,
}

#[derive(Debug, Clone)]
pub struct CompatResult {
    pub compatible: bool,
    pub level: CompatLevel,
    pub warn: Option<String>,
}

pub fn parse_semver(version: &str) -> (u32, u32, u32) {
    let mut parts = version.split('.').map(|s| s.parse::<u32>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

pub fn check_compatibility(client_version: &str) -> CompatResult {
    let server_str: String = PROTOCOL_VERSION.clone();
    let server = parse_semver(&server_str);
    let client = parse_semver(client_version);

    if server.0 != client.0 {
        return CompatResult {
            compatible: false,
            level: CompatLevel::Incompatible,
            warn: Some(format!(
                "Major version mismatch: client={client_version}, server={server_str}"
            )),
        };
    }
    if server.1 != client.1 {
        return CompatResult {
            compatible: true,
            level: CompatLevel::Minor,
            warn: Some(format!(
                "Minor version mismatch: client={client_version}, server={server_str}"
            )),
        };
    }
    if server.2 != client.2 {
        return CompatResult {
            compatible: true,
            level: CompatLevel::Patch,
            warn: None,
        };
    }
    CompatResult {
        compatible: true,
        level: CompatLevel::Exact,
        warn: None,
    }
}
