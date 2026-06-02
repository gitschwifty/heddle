//! Export JSON Schemas for Taplo (TOML LSP) autocomplete + validation.
//!
//! Run: `cargo run --bin export-schemas`
//!
//! Mirrors `ts-src/scripts/export-schemas.ts`. Writes `schemas/config.schema.json`
//! and `schemas/hooks.schema.json` from the `JsonSchema`-deriving structs in
//! `src/config/types.rs` and `src/hooks/types.rs`.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use heddle::schema_export::{config_schema, hooks_schema, pretty_schema};

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas")
}

fn write(name: &str, body: serde_json::Value) -> Result<()> {
    let path = schema_dir().join(name);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, pretty_schema(&body))
        .with_context(|| format!("writing {}", path.display()))?;
    println!("Exported {} -> {}", name, path.display());
    Ok(())
}

fn main() -> Result<()> {
    write("config.schema.json", config_schema())?;
    write("hooks.schema.json", hooks_schema())?;
    Ok(())
}
