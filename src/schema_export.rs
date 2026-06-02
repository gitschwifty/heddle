//! Shared JSON Schema generation for the `export-schemas` bin and drift tests.

use schemars::schema_for;

use crate::config::types::HeddleConfigSchema;
use crate::hooks::types::HooksConfig;

pub fn config_schema() -> serde_json::Value {
    serde_json::to_value(schema_for!(HeddleConfigSchema)).expect("config schema serializes")
}

pub fn hooks_schema() -> serde_json::Value {
    serde_json::to_value(schema_for!(HooksConfig)).expect("hooks schema serializes")
}

pub fn pretty_schema(body: &serde_json::Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(body).expect("schema pretty-prints")
    )
}
