pub mod detect;
pub mod markdown;
pub mod page_status;
pub mod response;
pub mod types;

pub use detect::detect_source_type;
pub(crate) mod yaml;
pub use types::{ExtractionResult, MarkdownDocument, RedditComment};
pub(crate) use yaml::yaml_escape;
