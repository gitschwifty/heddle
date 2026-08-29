#![allow(dead_code)]
//! Test env loading for the repository dotenv files.
//!
//! Call [`init`] from any integration test that needs env vars such as
//! `OPENROUTER_API_KEY`. Existing process environment wins; otherwise the
//! normal local/test/default dotenv precedence is used. Safe to call from
//! multiple tests/threads — `OnceLock` ensures the dotenvy load happens once
//! per test binary.

use std::sync::OnceLock;

static INIT: OnceLock<()> = OnceLock::new();

pub fn init() {
    INIT.get_or_init(|| {
        let _ = dotenvy::from_filename(".env.local");
        let _ = dotenvy::from_filename(".env.test");
        let _ = dotenvy::dotenv();
    });
}
