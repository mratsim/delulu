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
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reddit => write!(f, "reddit"),
            Self::Discourse => write!(f, "discourse"),
            Self::GenericHtml => write!(f, "generic_html"),
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
            _ => Err(format!("unknown source type: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// WebbfetchError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
pub enum WebbfetchError {
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
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

// ---------------------------------------------------------------------------
// HttpClient trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn get(&self, url: &str) -> Result<Response, WebbfetchError>;
}

// ---------------------------------------------------------------------------
// FetchConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchConfig {
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Queries per second rate limit.
    pub qps: u64,
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

// ---------------------------------------------------------------------------
// UrlInfo
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlInfo {
    pub url: String,
    pub source_type: SourceType,
    pub domain: String,
}

// ---------------------------------------------------------------------------
// FetchResult
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResult {
    pub url: UrlInfo,
    pub content: ExtractionResult,
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
