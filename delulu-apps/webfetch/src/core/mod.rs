pub mod detect;
pub mod types;

pub use detect::detect_source_type;
pub use types::{ExtractionResult, MarkdownDocument, RedditComment};
