//! Conservative tool-I/O policy. Dotenv examples and PEM certificates are
//! deliberately protected too; filenames cannot establish that they are safe.
//! This is not a streaming boundary or a detector for encoded credentials.

use std::path::Path;
use std::sync::LazyLock;

use parking_lot::RwLock;
use regex::Regex;

/// Shared Rust/Seatbelt-compatible expression, matching whole path components.
pub const PROTECTED_PATH_PATTERN: &str = r"(^|/)([.]env([.][^/]*)?|[.]netrc|[.]npmrc|[.]pypirc|[.]ssh|[.]aws|[.]gnupg|id_rsa|id_dsa|id_ecdsa|id_ed25519|[^/]*[.](pem|key|p12|pfx)|credentials[.]json|application_default_credentials[.]json)(/|$)";

pub const PROTECTED_PATH_DENIAL: &str =
    "Error: protected credential path denied by secret-safe tool I/O policy";

pub fn is_protected_path(path: &Path) -> bool {
    static PATTERN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(PROTECTED_PATH_PATTERN).expect("static path pattern"));
    PATTERN.is_match(&path.to_string_lossy())
        || path
            .canonicalize()
            .is_ok_and(|physical| PATTERN.is_match(&physical.to_string_lossy()))
}

// Never derive Debug or serialize this store. Retaining rotated credentials for
// the process lifetime also protects results from in-flight older clients.
static KNOWN_CREDENTIALS: LazyLock<RwLock<Vec<String>>> = LazyLock::new(|| RwLock::new(Vec::new()));

pub fn register_credential(value: &str) {
    if value.is_empty() {
        return;
    }
    let mut known = KNOWN_CREDENTIALS.write();
    if !known.iter().any(|existing| existing == value) {
        known.push(value.to_owned());
        known.sort_by_key(|value| std::cmp::Reverse(value.len()));
    }
}

/// Consume a complete result before it reaches events, hooks, or persistence.
/// Assignment detection intentionally requires uppercase credential names and
/// a literal value; mere identifier mentions are preserved. Literal examples
/// can be false positives. No original values are included in diagnostics.
pub fn redact(mut text: String) -> String {
    // Collect spans against the original text, then merge overlaps. Replacing
    // sequentially could redact a marker or partially hide a larger secret.
    let mut spans = Vec::new();
    for value in KNOWN_CREDENTIALS.read().iter() {
        spans.extend(
            text.match_indices(value)
                .map(|(start, matched)| (start, start + matched.len())),
        );
    }
    static DETECTORS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
        [
            r"(?s)-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----.*?(?:-----END (?:[A-Z0-9]+ )*PRIVATE KEY-----|$)",
            r"\b(?:sk-(?:or-v1-|ant-api03-)?[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|xox[baprs]-[A-Za-z0-9-]{20,})\b",
            r#"[a-zA-Z][a-zA-Z0-9+.-]*://([^\s/@]+@)"#,
            r#"\b(?:PASSWORD|TOKEN|SECRET|API_KEY|[A-Z][A-Z0-9_]*_(?:PASSWORD|TOKEN|SECRET|API_KEY|ACCESS_KEY))\s*=\s*("[^"\r\n]+"|'[^'\r\n]+'|[^\s;"']+)"#,
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("static redaction pattern"))
        .collect()
    });
    for detector in DETECTORS.iter() {
        for captures in detector.captures_iter(&text) {
            let matched = captures.get(1).or_else(|| captures.get(0)).expect("match");
            if matched.as_str().starts_with("[REDACTED:") {
                continue;
            }
            spans.push((matched.start(), matched.end()));
        }
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        if let Some(last) = merged.last_mut() {
            if start < last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    for (start, end) in merged.into_iter().rev() {
        text.replace_range(start..end, "[REDACTED: credential]");
    }
    text
}
