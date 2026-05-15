use heddle::ipc::protocol::{check_compatibility, parse_semver, CompatLevel, PROTOCOL_VERSION};

mod common;

#[test]
fn parse_semver_basic() {
    assert_eq!(parse_semver("1.2.3"), (1, 2, 3));
    assert_eq!(parse_semver("0.0.0"), (0, 0, 0));
}

#[test]
fn parse_semver_partial() {
    assert_eq!(parse_semver("1"), (1, 0, 0));
    assert_eq!(parse_semver("1.2"), (1, 2, 0));
}

#[test]
fn parse_semver_garbage() {
    assert_eq!(parse_semver("abc"), (0, 0, 0));
}

#[test]
fn protocol_version_loads() {
    assert!(!PROTOCOL_VERSION.is_empty());
    let parts: Vec<&str> = PROTOCOL_VERSION.split('.').collect();
    assert_eq!(parts.len(), 3, "PROTOCOL_VERSION must be semver");
}

#[test]
fn check_compatibility_exact() {
    let r = check_compatibility(&PROTOCOL_VERSION);
    assert!(r.compatible);
    assert!(matches!(r.level, CompatLevel::Exact));
}

#[test]
fn check_compatibility_major_mismatch() {
    let server = parse_semver(&PROTOCOL_VERSION);
    let client = format!("{}.{}.{}", server.0 + 99, server.1, server.2);
    let r = check_compatibility(&client);
    assert!(!r.compatible);
    assert!(matches!(r.level, CompatLevel::Incompatible));
    assert!(r.warn.is_some());
}

#[test]
fn check_compatibility_minor_mismatch_compatible_with_warn() {
    let server = parse_semver(&PROTOCOL_VERSION);
    let client = format!("{}.{}.{}", server.0, server.1 + 1, server.2);
    let r = check_compatibility(&client);
    assert!(r.compatible);
    assert!(matches!(r.level, CompatLevel::Minor));
    assert!(r.warn.is_some());
}

#[test]
fn check_compatibility_patch_mismatch_compatible() {
    let server = parse_semver(&PROTOCOL_VERSION);
    let client = format!("{}.{}.{}", server.0, server.1, server.2 + 1);
    let r = check_compatibility(&client);
    assert!(r.compatible);
    assert!(matches!(r.level, CompatLevel::Patch));
}
