//! Default deny rules shipped with the global config on first run.

pub const DEFAULT_DENY_RULES: &[&str] = &[
    "Write(.env*)",
    "Write(*.pem)",
    "Write(*.key)",
    "Write(*credentials*)",
    "Bash(rm *)",
    "Bash(rm)",
    "Bash(sudo *)",
    "Bash(chmod *)",
    "Write(.heddle/config.toml)",
];

pub fn generate_default_permissions_toml() -> String {
    let entries: Vec<String> = DEFAULT_DENY_RULES
        .iter()
        .map(|r| format!("  \"{r}\","))
        .collect();
    format!("[permissions]\ndeny = [\n{}\n]\n", entries.join("\n"))
}
