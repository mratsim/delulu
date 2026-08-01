//! Pure JSON response builder shared by the `webfetch_raw` MCP tool.
//!
//! [`webfetch_raw_response`] serializes an `ExtractionResult` at the **top
//! level** of a JSON object and inserts `page_status` as a **sibling** key.
//! It performs no I/O and is fully unit-testable.

use crate::core::page_status::PageStatus;
use crate::core::types::ExtractionResult;

/// Build the JSON string for the `webfetch_raw` MCP tool's success response.
///
/// The `ExtractionResult` is serialized at top level (externally tagged enum,
/// so it becomes a JSON object whose single key is the variant name, e.g.
/// `GenericHtml`), and `page_status` is inserted as a **sibling** key — never
/// nested under a `result` wrapper.
///
/// # Returns
///
/// Infallible `String`. `ExtractionResult` and all its fields, `PageStatus`,
/// and `BlockedBy` are plain `#[derive(Serialize)]` types with no floats, no
/// non-string keys, and no custom `Serialize`, so `serde_json::to_value` /
/// `to_string` cannot fail.
///
/// # Panics
///
/// Panics (via `expect`) if `result` does not serialize to a JSON object.
/// `ExtractionResult` is an externally tagged enum, so this is structurally
/// guaranteed; the `expect` documents the invariant instead of silently
/// dropping data with `unwrap_or_default()`.
pub fn webfetch_raw_response(result: &ExtractionResult, status: &PageStatus) -> String {
    let mut value = serde_json::to_value(result).expect(
        "ExtractionResult and all fields are plain Serialize types with no \
         floats or non-string keys, so serialization cannot fail",
    );
    debug_assert!(value.is_object(), "ExtractionResult must serialize to a JSON object");
    let obj = value.as_object_mut().expect(
        "ExtractionResult is an externally tagged enum and always serializes \
         to a JSON object",
    );
    obj.insert(
        "page_status".to_string(),
        serde_json::to_value(status).expect("PageStatus is a plain Serialize enum"),
    );
    serde_json::to_string(&value).expect("serializing a serde_json::Value cannot fail")
}

#[cfg(test)]
#[path = "../../tests/unit/core/response_test.rs"]
mod tests;
