pub mod detect;
pub mod page_status;
pub mod response;
pub mod types;

pub use detect::detect_source_type;
pub use types::{ExtractionResult, MarkdownDocument, RedditComment};
