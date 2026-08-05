use crate::core::types::{RedditComment, WebfetchError};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Intermediate serde types for Reddit JSON API
// ---------------------------------------------------------------------------

/// Top-level Reddit API response: array of two Listings (post + comments).
#[derive(Deserialize)]
struct RedditApiResponse(Vec<RedditListing>);

#[derive(Deserialize)]
struct RedditListing {
    // Reserved — consumed by serde during deserialization to match the Reddit API response shape.
    // Kept for future use (e.g. distinguishing Listing vs. t1/t3 kinds without looping children).
    #[allow(dead_code)]
    #[serde(default)]
    kind: String,
    #[serde(default)]
    data: RedditListingData,
}

#[derive(Deserialize, Default)]
struct RedditListingData {
    #[serde(default)]
    children: Vec<RedditChild>,
}

#[derive(Deserialize)]
struct RedditChild {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// RedditData
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedditData {
    pub title: String,
    pub author: String,
    pub score: i64,
    pub selftext: String,
    pub created_utc: f64,
    pub permalink: String,
    pub comments: Vec<RedditComment>,
    pub comments_truncated: bool,
}

// ---------------------------------------------------------------------------
// RedditExtractor
// ---------------------------------------------------------------------------

pub struct RedditExtractor;

impl RedditExtractor {
    const MAX_DEPTH: u32 = 10;
    const MAX_COMMENTS: usize = 500;

    /// Extract structured Reddit data from Reddit's JSON API response.
    ///
    /// The Reddit API returns a top-level array:
    ///   `[post_listing, comments_listing]`
    ///
    /// - `post_listing["data"]["children"][0]["data"]` contains post metadata.
    /// - `comments_listing["data"]["children"]` is the comments array.
    pub fn extract(json_str: &str) -> Result<RedditData, WebfetchError> {
        // Pre-check for known error shapes before full parse.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str)
            && let Some(err) = val.get("error").and_then(|e| e.as_i64())
            && err == 403
        {
            return Err(WebfetchError::AuthRequired(
                "Reddit API returned HTTP 403 — content may be private, removed, or quarantined"
                    .into(),
            ));
        }

        let response: RedditApiResponse = serde_json::from_str(json_str).map_err(|e| {
            WebfetchError::Parse(format!("Failed to parse Reddit JSON response: {e}"))
        })?;

        // --- Extract post info ---
        let post_listing = response.0.first().ok_or_else(|| {
            WebfetchError::Parse("Reddit response: missing post listing (index 0)".into())
        })?;
        let post_child = post_listing.data.children.first().ok_or_else(|| {
            WebfetchError::Parse("Reddit response: missing post child in listing".into())
        })?;

        let post_data = &post_child.data;
        let title = post_data
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let author = post_data
            .get("author")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let score = post_data.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
        let selftext = post_data
            .get("selftext")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let created_utc = post_data
            .get("created_utc")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let permalink = post_data
            .get("permalink")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // --- Extract comments ---
        let mut current_count = 0usize;
        let mut truncated = false;

        let comments = if let Some(comments_listing) = response.0.get(1) {
            Self::parse_children(
                &comments_listing.data.children,
                Self::MAX_DEPTH,
                Self::MAX_COMMENTS,
                &mut current_count,
                &mut truncated,
            )
        } else {
            Vec::new()
        };

        Ok(RedditData {
            title,
            author,
            score,
            selftext,
            created_utc,
            permalink,
            comments,
            comments_truncated: truncated,
        })
    }

    /// Recursively parse a slice of `RedditChild` items into `RedditComment`s.
    fn parse_children(
        children: &[RedditChild],
        max_depth: u32,
        max_total: usize,
        current_count: &mut usize,
        truncated: &mut bool,
    ) -> Vec<RedditComment> {
        let mut comments = Vec::new();

        for child in children {
            if *current_count >= max_total {
                *truncated = true;
                break;
            }

            match child.kind.as_str() {
                "more" => {
                    let count = child
                        .data
                        .get("count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    if count > 0 {
                        // TODO side-effect to push to main: tracing::* logging in lib
                        tracing::warn!("Reddit: skipping {} more comments", count);
                    }
                    *truncated = true;
                }
                "t1" => {
                    let depth = child
                        .data
                        .get("depth")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    if depth >= max_depth {
                        // Comment is deeper than max depth — skip entirely.
                        *truncated = true;
                        continue;
                    }

                    let comment = RedditComment {
                        author: child
                            .data
                            .get("author")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        body: child
                            .data
                            .get("body")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        score: child
                            .data
                            .get("score")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0),
                        depth,
                        replies: Self::parse_replies(
                            &child.data,
                            max_depth,
                            max_total,
                            current_count,
                            truncated,
                        ),
                    };
                    *current_count += 1;
                    comments.push(comment);
                }
                _ => {
                    // Unknown kind (e.g. "t3" for posts in cross-post listings) — skip.
                }
            }
        }

        comments
    }

    /// Parse the `replies` field of a comment, which can be a Listing object
    /// or an empty string.
    fn parse_replies(
        data: &serde_json::Value,
        max_depth: u32,
        max_total: usize,
        current_count: &mut usize,
        truncated: &mut bool,
    ) -> Vec<RedditComment> {
        if let Some(replies) = data.get("replies")
            && replies.is_object()
            && let Ok(listing) = serde_json::from_value::<RedditListing>(replies.clone())
        {
            return Self::parse_children(
                &listing.data.children,
                max_depth,
                max_total,
                current_count,
                truncated,
            );
        }
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../../tests/unit/sources/reddit_test.rs"]
mod tests;
