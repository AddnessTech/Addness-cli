use serde::{Deserialize, Serialize};

use super::issue::IssueMessage;

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

/// Result of a legacy `comment` command mutation.
///
/// Objective comments are written through the v2 goal-issue API whenever its
/// contract can represent the request. The v1 variant is retained only for
/// legacy-only cases such as content over 4,000 characters, mention-changing
/// edits, or non-objective comments. `untagged` keeps `--json` output equal to
/// the backend resource rather than adding a CLI-only discriminator.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum CommentMutationResult {
    V2(Box<IssueMessage>),
    V1(Box<Comment>),
}

impl CommentMutationResult {
    pub fn id(&self) -> &str {
        match self {
            Self::V2(message) => &message.id,
            Self::V1(comment) => &comment.id,
        }
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

// GET /v1/team/comments/:id/context
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

#[cfg(test)]
mod tests {
    use super::{Comment, CommentAuthor, CommentMutationResult};
    use crate::api::IssueMessage;
    use serde_json::json;

    #[test]
    fn mutation_result_serializes_backend_resource_without_variant_wrapper() {
        let legacy = CommentMutationResult::V1(Box::new(Comment {
            id: "legacy-1".to_string(),
            content: "legacy".to_string(),
            commentable_type: "objective".to_string(),
            commentable_id: "goal-1".to_string(),
            parent_id: None,
            author: CommentAuthor {
                id: "member-1".to_string(),
                name: "Member".to_string(),
                is_ai_agent: false,
            },
            reply_count: 0,
            resolved_at: None,
            created_at: "2026-08-28T00:00:00Z".to_string(),
            updated_at: "2026-08-28T00:00:00Z".to_string(),
        }));
        let legacy_json = serde_json::to_value(&legacy).unwrap();
        assert_eq!(legacy.id(), "legacy-1");
        assert_eq!(legacy_json["id"], "legacy-1");
        assert!(legacy_json.get("V1").is_none());

        let issue: IssueMessage = serde_json::from_value(json!({"id": "issue-1"})).unwrap();
        let v2 = CommentMutationResult::V2(Box::new(issue));
        let v2_json = serde_json::to_value(&v2).unwrap();
        assert_eq!(v2.id(), "issue-1");
        assert_eq!(v2_json["id"], "issue-1");
        assert!(v2_json.get("V2").is_none());
    }
}
