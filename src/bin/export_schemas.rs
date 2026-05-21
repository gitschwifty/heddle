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
use heddle::config::types::HeddleConfigSchema;
use heddle::hooks::types::HooksConfig;
use schemars::schema_for;

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas")
}

fn write(name: &str, body: serde_json::Value) -> Result<()> {
    let path = schema_dir().join(name);
    fs::create_dir_all(path.parent().unwrap())?;
    let pretty = serde_json::to_string_pretty(&body)?;
    fs::write(&path, format!("{pretty}\n"))
        .with_context(|| format!("writing {}", path.display()))?;
    println!("Exported {} -> {}", name, path.display());
    Ok(())
}

fn main() -> Result<()> {
    write(
        "config.schema.json",
        serde_json::to_value(schema_for!(HeddleConfigSchema))?,
    )?;
    write(
        "hooks.schema.json",
        serde_json::to_value(schema_for!(HooksConfig))?,
    )?;
    Ok(())
}
