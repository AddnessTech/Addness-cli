use std::collections::HashMap;

use crate::api::{ApiClient, ApiResponse, Comment, CommentsResponse, CreateCommentRequest};
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
