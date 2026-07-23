use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
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
    /// Raised when xberg extraction fails or times out (120s timeout exceeded).
    #[error("xberg error: {0}")]
    XbergError(String),
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub body: String,
    /// MIME type / content-type header value from the HTTP response.
    pub content_type: Option<String>,
}

// ---------------------------------------------------------------------------
// HttpClient trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn get(&self, url: &str) -> Result<Response, WebfetchError>;

    /// Fetch a URL and return the raw bytes of the response body.
    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, WebfetchError>;
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
        comments: Vec<RedditComment>,
    },
    Discourse {
        title: String,
        topic_id: u64,
        posts: Vec<DiscoursePost>,
    },
    GenericHtml {
        content_md: MarkdownDocument,
    },
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
