use std::fmt;

use crate::core::types::WebfetchError;

/// Recoverable errors in the pipeline.
/// Logic bugs (div-by-zero, missing deps, serialization) remain panics.
#[derive(Debug)]
pub enum PipelineError {
    /// External input could not be parsed as valid HTML/JSON.
    ParseError(String),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::ParseError(msg) => write!(f, "Parse error: {msg}"),
        }
    }
}

impl std::error::Error for PipelineError {}

impl From<PipelineError> for WebfetchError {
    fn from(e: PipelineError) -> Self {
        WebfetchError::Parse(e.to_string())
    }
}
