use serde::{Deserialize, Serialize};

// GET /api/v2/comments
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentsResponse {
    pub comments: Vec<Comment>,
    pub total_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub content: String,
    pub commentable_type: String,
    pub commentable_id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub author: CommentAuthor,
    #[serde(default)]
    pub reply_count: i64,
    #[serde(default)]
    pub resolved_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Comment {
    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentDetail {
    #[serde(flatten)]
    pub comment: Comment,
    #[serde(default)]
    pub replies: Vec<Comment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentAuthor {
    pub id: String,
    pub name: String,
    #[serde(default, alias = "isAIAgent")]
    pub is_ai_agent: bool,
}

// POST /api/v2/objectives/:id/comments
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCommentRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<String>,
}

// PUT /api/v2/comments/:id
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommentRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<String>,
}

// POST /api/v2/comments/:id/reactions
#[derive(Debug, Serialize)]
pub struct ReactionRequest {
    pub emoji: String,
}

// GET /api/v2/comments/:id/context
// Surrounding comments for a notification highlight (radius before/after).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentContextResponse {
    #[serde(default)]
    pub comments: Vec<Comment>,
    /// Index of the highlighted target comment within `comments`.
    pub target_index: i64,
    #[serde(default)]
    pub has_above: bool,
    #[serde(default)]
    pub has_below: bool,
    #[serde(default)]
    pub total_count: i64,
    /// Present when the target is a thread reply.
    #[serde(default)]
    pub parent_comment: Option<Comment>,
}
