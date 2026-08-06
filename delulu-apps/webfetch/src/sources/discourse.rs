use serde::{Deserialize, Serialize};

use crate::core::types::{DiscoursePost, WebfetchError};

// ---------------------------------------------------------------------------
// DiscourseData
// ---------------------------------------------------------------------------

/// Represents a parsed Discourse topic with its metadata and posts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscourseData {
    pub title: String,
    pub topic_id: u64,
    pub slug: String,
    pub posts: Vec<DiscoursePost>,
    pub post_count: u64,
}

// ---------------------------------------------------------------------------
// Internal API response types
// ---------------------------------------------------------------------------

/// Top-level JSON structure returned by Discourse `/t/slug/id.json`.
#[derive(Debug, Deserialize)]
struct DiscourseApiResponse {
    title: String,
    id: u64,
    slug: String,
    posts_count: u64,
    post_stream: PostStream,
}

/// Wrapper for the `post_stream` object.
#[derive(Debug, Deserialize)]
struct PostStream {
    posts: Vec<DiscoursePost>,
}

// ---------------------------------------------------------------------------
// DiscourseExtractor
// ---------------------------------------------------------------------------

/// Extracts structured data from a Discourse topic JSON response.
pub struct DiscourseExtractor;

impl DiscourseExtractor {
    /// Parse a Discourse `/t/slug/id.json` response body into [`DiscourseData`].
    ///
    /// # Errors
    ///
    /// Returns [`WebfetchError::Parse`] when the JSON is malformed or missing
    /// required fields.
    pub fn extract(json_str: &str) -> Result<DiscourseData, WebfetchError> {
        let api_response: DiscourseApiResponse = serde_json::from_str(json_str)
            .map_err(|e| WebfetchError::Parse(format!("Failed to parse Discourse JSON: {e}")))?;

        let mut posts = api_response.post_stream.posts;
        let post_count = api_response.posts_count;

        if (posts.len() as u64) < post_count {
            // TODO side-effect to push to main: tracing::* logging in lib
            tracing::warn!(
                "Discourse topic '{}' (id={}) has {} posts on server but only {} in this response (possibly paginated)",
                api_response.title,
                api_response.id,
                post_count,
                posts.len(),
            );
        }

        if posts.len() > 200 {
            tracing::warn!("Discourse: truncating {} posts to 200", posts.len());
            posts.truncate(200);
        }

        Ok(DiscourseData {
            title: api_response.title,
            topic_id: api_response.id,
            slug: api_response.slug,
            posts,
            post_count,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../../tests/unit/sources/discourse_test.rs"]
mod tests;
