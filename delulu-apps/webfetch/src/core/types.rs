use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// SourceType
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceType {
    Reddit,
    Discourse,
    GenericHtml,
    /// arXiv PDF documents fetched via the arXiv API.
    ArxivPdf,
    /// Generic document (PDF, plain text, etc.) fetched via xberg.
    Document,
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reddit => write!(f, "reddit"),
            Self::Discourse => write!(f, "discourse"),
            Self::GenericHtml => write!(f, "generic_html"),
            Self::ArxivPdf => write!(f, "arxiv_pdf"),
            Self::Document => write!(f, "document"),
        }
    }
}

impl FromStr for SourceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "reddit" => Ok(Self::Reddit),
            "discourse" => Ok(Self::Discourse),
            "generic_html" | "generic" | "html" => Ok(Self::GenericHtml),
            "arxiv_pdf" | "arxiv" => Ok(Self::ArxivPdf),
            "document" | "doc" => Ok(Self::Document),
            _ => Err(format!("unknown source type: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// WebfetchError
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, Error)]
pub enum WebfetchError {
    #[error("HTTP fetch error: {0}")]
    Fetch(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("DOM pass error: {0}")]
    Pass(String),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Request timed out: {0}")]
    Timeout(String),

    #[error("Retry exhausted after {0} attempts")]
    RetryExhausted(u32),

    #[error("Authentication required: {0}")]
    AuthRequired(String),

    /// I/O error from the underlying HTTP transport or filesystem.
    /// Raised when temp file creation, writing, or document size checks fail
    /// during document fetch (xberg pipeline).
    #[error("I/O error: {0}")]
    IoError(String),

    /// Error returned by the xberg document-fetching backend.
    /// Raised when xberg extraction fails or times out (10s timeout exceeded).
    #[error("xberg error: {0}")]
    XbergError(String),
}

// ---------------------------------------------------------------------------
// MarkdownDocument
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkdownDocument {
    pub frontmatter: String,
    pub body: String,
}

// ---------------------------------------------------------------------------
// RedditComment
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedditComment {
    pub author: String,
    pub body: String,
    pub score: i64,
    pub depth: u32,
    pub replies: Vec<RedditComment>,
}

// ---------------------------------------------------------------------------
// DiscoursePost
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoursePost {
    pub post_number: u64,
    pub username: String,
    #[serde(default)]
    pub raw: String,
    pub created_at: String,
    #[serde(default)]
    pub reply_to_post_number: Option<u64>,
}

// ---------------------------------------------------------------------------
// ExtractionResult
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExtractionResult {
    Reddit {
        title: String,
        selftext: String,
        author: String,
        score: i64,
        permalink: String,
        source_url: String,
        comments: Vec<RedditComment>,
        /// Number of comments RETURNED in `comments` — capped at
        /// `RedditExtractor::MAX_COMMENTS` (500). This is NOT the thread's
        /// total comment count on the server; when comments were truncated,
        /// `comments_truncated` is `true`.
        ///
        /// Surfaced (not redundant) in the Reddit frontmatter output
        /// in `src/core/markdown.rs`.
        comment_count: usize,
        /// Whether the returned comments were truncated (capped at
        /// `RedditExtractor::MAX_COMMENTS`, or a `more`/deep-depth marker
        /// was encountered). When `true`, `comment_count` is the capped
        /// returned count, not the server total.
        comments_truncated: bool,
    },
    Discourse {
        title: String,
        topic_id: u64,
        /// The canonical thread URL this topic was fetched from (the original
        /// `url` the caller passed in). Surfaced in the Discourse frontmatter
        /// as `source_url` so the output never hardcodes a misleading `N/A`.
        source_url: String,
        posts: Vec<DiscoursePost>,
        post_count: u64,
        /// Number of posts RETURNED in `posts` (the Discourse JSON API can
        /// return fewer than the topic's `post_count`).
        ///
        /// Surfaced (not redundant) in the Discourse frontmatter output
        /// in `src/core/markdown.rs`.
        posts_returned: usize,
    },
    GenericHtml {
        content_md: MarkdownDocument,
        /// Length in bytes of the raw HTML body (set at every construction site;
        /// surfaced in the `webfetch_raw` JSON report via serde derive).
        raw_html_len: usize,
        /// Length in bytes of the extracted markdown body (post-pipeline;
        /// surfaced in the `webfetch_raw` JSON report via serde derive).
        filtered_html_len: usize,
    },
}

#[cfg(test)]
#[path = "../../tests/unit/core/types_test.rs"]
mod tests;
