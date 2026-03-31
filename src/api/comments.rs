use crate::api::{ApiClient, Comment, CommentsResponse, CreateCommentRequest};
use anyhow::Result;

impl ApiClient {
    pub async fn list_comments(&self, goal_id: &str) -> Result<CommentsResponse> {
        let path = format!("/api/v2/objectives/{goal_id}/comments");
        self.get(&path).await
    }

    pub async fn create_comment(&self, goal_id: &str, body: &str) -> Result<Comment> {
        let req = CreateCommentRequest {
            commentable_type: "objective".to_string(),
            commentable_id: goal_id.to_string(),
            content: body.to_string(),
            parent_id: None,
        };
        self.post("/api/v1/team/comments", &req).await
    }
}
