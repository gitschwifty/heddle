#![allow(dead_code)]
//! Test env loading. Mirrors `bun test`'s auto-load of `.env.test`.
//!
//! Call [`init`] from any integration test that needs env vars from `.env.test`
//! (e.g. `OPENROUTER_API_KEY`, `HEDDLE_INTEGRATION_TESTS`). Safe to call from
//! multiple tests/threads — `OnceLock` ensures the dotenvy load happens once
//! per test binary.

use std::sync::OnceLock;

static INIT: OnceLock<()> = OnceLock::new();

pub fn init() {
    INIT.get_or_init(|| {
        let _ = dotenvy::from_filename(".env.test");
    });
}
