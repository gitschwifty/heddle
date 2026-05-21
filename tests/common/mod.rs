//! Shared test helpers: sandbox, mock provider, mock streams.
//!
//! Each integration test binary is its own crate, so unused helpers here trip
//! `dead_code` in the targets that don't use them. The `#![allow]` on each
//! submodule below scopes the suppression to test-helper code only.

pub mod env;
pub mod headless;
pub mod mocks;
pub mod sandbox;

#[allow(unused_imports)]
pub use mocks::*;
#[allow(unused_imports)]
pub use sandbox::Sandbox;
