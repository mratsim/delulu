use serde::{Deserialize, Serialize};

use crate::core::types::{DiscoursePost, MarkdownDocument, WebfetchError};

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

// ---------------------------------------------------------------------------
// From<DiscourseData> for MarkdownDocument
// ---------------------------------------------------------------------------

impl From<DiscourseData> for MarkdownDocument {
    fn from(data: DiscourseData) -> Self {
        // Build YAML frontmatter
        let frontmatter = format!(
            "title: \"{}\"\nsource_type: \"discourse\"\ntopic_id: {}",
            data.title, data.topic_id
        );

        // Build body: sequential numbered posts
        let mut body = String::new();

        for post in &data.posts {
            // Post header
            body.push_str(&format!("## Post #{}\n", post.post_number));

            // Metadata line
            body.push_str(&format!(
                "**{}** — {} — Post #{}\n\n",
                post.username, post.created_at, post.post_number,
            ));

            // Use raw Markdown from the API response
            let content_md = post.raw.trim().to_string();
            let trimmed = content_md.trim();
            if !trimmed.is_empty() {
                body.push_str(trimmed);
                body.push('\n');
            }

            body.push('\n');
        }

        MarkdownDocument { frontmatter, body }
    }
}

// ---------------------------------------------------------------------------
// From<DiscourseData> for serde_json::Value
// ---------------------------------------------------------------------------

impl TryFrom<DiscourseData> for serde_json::Value {
    type Error = serde_json::Error;
    fn try_from(data: DiscourseData) -> Result<Self, Self::Error> {
        serde_json::to_value(&data)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../../tests/unit/sources/discourse_test.rs"]
mod tests;
