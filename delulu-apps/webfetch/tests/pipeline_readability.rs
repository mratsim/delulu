//! Readability test suite — integration tests against Mozilla Readability.js test fixtures.
//!
//! This module discovers all 130+ test cases from `tests/fixtures-readability/`
//! and validates the Rust readability pipeline output against the JS expected output.
//!
//! Module structure:
//! - `helpers`: Fixture loading and HTML normalization
//! - `t_pipeline_readability`: Parametrized test runner across all fixtures
//!
//! # NOTE: #[path] attributes
//!
//! Rust integration test crates resolve `mod foo` relative to the crate root's parent
//! directory (`tests/`), not relative to the crate root file itself. Since we want
//! submodules in `tests/readability/`, we use `#[path]` to point to the correct files.

#[path = "pipeline_readability/helpers.rs"]
mod helpers;

#[path = "pipeline_readability/t_pipeline_readability.rs"]
mod t_pipeline_readability;
