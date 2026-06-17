use serde::{Deserialize, Serialize};

// GET /v1/team/comments
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
    #[serde(default)]
    pub is_ai_agent: bool,
}

// POST /v1/team/comments
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCommentRequest {
    pub commentable_type: String,
    pub commentable_id: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<String>,
}

// PUT /v1/team/comments/:id
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommentRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<String>,
}

// POST /v1/team/comments/:id/reactions
#[derive(Debug, Serialize)]
pub struct ReactionRequest {
    pub emoji: String,
}
