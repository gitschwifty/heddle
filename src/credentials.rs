//! Resolve non-secret credential references immediately before provider setup.
//!
//! References are safe to keep in configuration and diagnostics; the resolved
//! value is intentionally returned only to the caller that constructs a client.

use anyhow::{anyhow, Result};

const KEYCHAIN_REFERENCE_HELP: &str = "credential must use keychain:<service>/<account>";

/// The local OpenRouter credential Heddle checks when configuration does not
/// select a different backend or keychain item.
pub const DEFAULT_OPENROUTER_CREDENTIAL: &str = "keychain:heddle/openrouter";
pub const DEFAULT_STRAITLY_CREDENTIAL: &str = "keychain:heddle/straitly";

#[derive(Debug, Clone, PartialEq, Eq)]
enum CredentialReference {
    Keychain { service: String, account: String },
}

impl CredentialReference {
    fn parse(reference: &str) -> Result<Self> {
        let path = reference
            .strip_prefix("keychain:")
            .ok_or_else(|| anyhow!(KEYCHAIN_REFERENCE_HELP))?;
        let (service, account) = path
            .split_once('/')
            .ok_or_else(|| anyhow!(KEYCHAIN_REFERENCE_HELP))?;
        if service.is_empty() || account.is_empty() || account.contains('/') {
            return Err(anyhow!(KEYCHAIN_REFERENCE_HELP));
        }
        Ok(Self::Keychain {
            service: service.to_string(),
            account: account.to_string(),
        })
    }
}

/// Resolve a configured credential reference without exposing it to the agent.
pub fn resolve_credential(reference: &str) -> Result<String> {
    match CredentialReference::parse(reference)? {
        CredentialReference::Keychain { service, account } => {
            resolve_keychain_credential(&service, &account)
        }
    }
}

#[cfg(target_os = "macos")]
fn resolve_keychain_credential(service: &str, account: &str) -> Result<String> {
    use security_framework::passwords::{generic_password, PasswordOptions};

    let bytes = generic_password(PasswordOptions::new_generic_password(service, account))
        .map_err(|_| {
            anyhow!(
                "could not read credential from macOS Keychain for {service}/{account}; add it with: security add-generic-password -U -s {service} -a {account} -w"
            )
        })?;
    let credential = String::from_utf8(bytes).map_err(|_| {
        anyhow!("credential from macOS Keychain for {service}/{account} is not UTF-8")
    })?;
    if credential.is_empty() {
        return Err(anyhow!(
            "credential from macOS Keychain for {service}/{account} is empty"
        ));
    }
    Ok(credential)
}

#[cfg(not(target_os = "macos"))]
fn resolve_keychain_credential(service: &str, account: &str) -> Result<String> {
    Err(anyhow!(
        "macOS Keychain credential references are unavailable on this platform ({service}/{account}); set the provider environment variable instead"
    ))
}

#[cfg(test)]
mod tests {
    use super::{CredentialReference, KEYCHAIN_REFERENCE_HELP};

    #[test]
    fn parses_keychain_reference() {
        assert_eq!(
            CredentialReference::parse("keychain:heddle/openrouter").unwrap(),
            CredentialReference::Keychain {
                service: "heddle".into(),
                account: "openrouter".into(),
            }
        );
    }

    #[test]
    fn rejects_invalid_references() {
        for reference in [
            "env:OPENROUTER_API_KEY",
            "keychain:",
            "keychain:heddle",
            "keychain:/openrouter",
            "keychain:heddle/",
            "keychain:heddle/openrouter/extra",
        ] {
            assert_eq!(
                CredentialReference::parse(reference)
                    .unwrap_err()
                    .to_string(),
                KEYCHAIN_REFERENCE_HELP
            );
        }
    }
}
