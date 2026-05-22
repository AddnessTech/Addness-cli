use std::collections::HashMap;

use crate::api::{
    ApiClient, ApiResponse, Comment, CommentsResponse, CreateCommentRequest, ReactionRequest,
    UpdateCommentRequest,
};
use anyhow::Result;

impl ApiClient {
    pub async fn list_comments(&self, goal_id: &str) -> Result<CommentsResponse> {
        let path = format!("/api/v2/objectives/{goal_id}/comments");
        let resp: ApiResponse<CommentsResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn create_comment(&self, goal_id: &str, body: &str) -> Result<Comment> {
        let req = CreateCommentRequest {
            commentable_type: "objective".to_string(),
            commentable_id: goal_id.to_string(),
            content: body.to_string(),
            parent_id: None,
        };
        let resp: ApiResponse<Comment> = self.post("/api/v1/team/comments", &req).await?;
        Ok(resp.data)
    }

    pub async fn update_comment(
        &self,
        comment_id: &str,
        content: &str,
        mentions: Vec<String>,
    ) -> Result<Comment> {
        let path = format!("/api/v1/team/comments/{comment_id}");
        let body = UpdateCommentRequest {
            content: content.to_string(),
            mentions,
        };
        let resp: ApiResponse<Comment> = self.put(&path, &body).await?;
        Ok(resp.data)
    }

    pub async fn delete_comment(&self, comment_id: &str) -> Result<()> {
        let path = format!("/api/v1/team/comments/{comment_id}");
        self.delete_no_body(&path).await
    }

    pub async fn resolve_comment(&self, comment_id: &str) -> Result<Comment> {
        let path = format!("/api/v1/team/comments/{comment_id}/resolve");
        let resp: ApiResponse<Comment> = self.patch_empty(&path).await?;
        Ok(resp.data)
    }

    pub async fn unresolve_comment(&self, comment_id: &str) -> Result<Comment> {
        let path = format!("/api/v1/team/comments/{comment_id}/unresolve");
        let resp: ApiResponse<Comment> = self.patch_empty(&path).await?;
        Ok(resp.data)
    }

    pub async fn add_reaction(&self, comment_id: &str, emoji: &str) -> Result<()> {
        let path = format!("/api/v1/team/comments/{comment_id}/reactions");
        let body = ReactionRequest {
            emoji: emoji.to_string(),
        };
        self.post_no_content(&path, &body).await
    }

    pub async fn delete_comment_attachment(
        &self,
        comment_id: &str,
        attachment_id: &str,
    ) -> Result<()> {
        let path = format!("/api/v1/team/comments/{comment_id}/attachments/{attachment_id}");
        self.delete_no_body(&path).await
    }

    /// 各ゴールのコメントを並行取得してマップで返す
    pub async fn get_comments_map(&self, goal_ids: &[&str]) -> HashMap<String, Vec<Comment>> {
        let futures: Vec<_> = goal_ids.iter().map(|g| self.list_comments(g)).collect();
        let results = futures::future::join_all(futures).await;

        let mut map = HashMap::new();
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(resp) => {
                    map.insert(goal_ids[i].to_string(), resp.comments);
                }
                Err(e) => {
                    eprintln!("Warning: failed to fetch comments for {}: {e}", goal_ids[i]);
                }
            }
        }

        map
    }
}
