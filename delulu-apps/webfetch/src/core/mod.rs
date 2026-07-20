pub mod detect;
pub mod http_client;
pub mod types;

pub use detect::detect_source_type;
pub use http_client::WebbfetchClient;
pub use types::{
    ExtractionResult, MarkdownDocument, RedditComment,
};
