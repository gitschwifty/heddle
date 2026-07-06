use heddle::schema_export::{config_schema, hooks_schema, pretty_schema};

fn assert_schema_current(file_name: &str, generated: serde_json::Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join(file_name);
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read committed schema {}: {e}", path.display()));
    let actual = pretty_schema(&generated);

    assert_eq!(
        actual, expected,
        "{file_name} is stale; run `cargo run --bin export-schemas` and commit the result"
    );
}

#[test]
fn config_schema_is_current() {
    assert_schema_current("config.schema.json", config_schema());
}

#[test]
fn hooks_schema_is_current() {
    assert_schema_current("hooks.schema.json", hooks_schema());
}
