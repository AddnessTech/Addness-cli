use std::collections::HashMap;

use crate::api::{
    ApiClient, ApiResponse, Comment, CommentDetail, CommentsResponse, CreateCommentRequest,
    ReactionRequest, RelatedFetchError, UpdateCommentRequest,
};
use anyhow::Result;

#[derive(Default)]
pub struct ListCommentsParams<'a> {
    pub goal_id: &'a str,
    pub parent_id: Option<&'a str>,
    pub resolved: Option<bool>,
    pub limit: Option<u16>,
    pub offset: Option<u64>,
    pub sort: Option<&'a str>,
    pub include_replies: bool,
}

impl ApiClient {
    pub async fn list_comments(&self, goal_id: &str) -> Result<CommentsResponse> {
        self.list_comments_with_params(ListCommentsParams {
            goal_id,
            ..Default::default()
        })
        .await
    }

    pub async fn list_comments_with_params(
        &self,
        params: ListCommentsParams<'_>,
    ) -> Result<CommentsResponse> {
        // Serializer は非Sendなので、ブロック内で文字列に確定させて drop し、
        // await をまたいで生存しないようにする（spawn 用に future を Send に保つ）。
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            if let Some(parent_id) = params.parent_id {
                query.append_pair("parentId", parent_id);
            }
            if let Some(resolved) = params.resolved {
                query.append_pair("resolved", if resolved { "true" } else { "false" });
            }
            if let Some(limit) = params.limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(offset) = params.offset {
                query.append_pair("offset", &offset.to_string());
            }
            if let Some(sort) = params.sort {
                query.append_pair("sort", sort);
            }
            if params.include_replies {
                query.append_pair("include_replies", "true");
            }
            query.finish()
        };
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let path = format!("/api/v2/objectives/{}/comments{suffix}", params.goal_id);
        let resp: ApiResponse<CommentsResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn get_comment(&self, comment_id: &str) -> Result<CommentDetail> {
        let path = format!("/api/v1/team/comments/{comment_id}");
        let resp: ApiResponse<CommentDetail> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn create_comment(&self, goal_id: &str, body: &str) -> Result<Comment> {
        self.create_comment_with_options(goal_id, body, None, Vec::new())
            .await
    }

    pub async fn create_comment_with_options(
        &self,
        goal_id: &str,
        body: &str,
        parent_id: Option<String>,
        mentions: Vec<String>,
    ) -> Result<Comment> {
        let req = CreateCommentRequest {
            commentable_type: "objective".to_string(),
            commentable_id: goal_id.to_string(),
            content: body.to_string(),
            parent_id,
            mentions,
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
        let (map, errors) = self.get_comments_map_with_errors(goal_ids).await;
        for error in errors {
            eprintln!(
                "Warning: failed to fetch {} for {}: {}",
                error.kind, error.goal_id, error.message
            );
        }

        map
    }

    /// 各ゴールのコメントを並行取得し、部分失敗を呼び出し側で扱える形で返す。
    pub async fn get_comments_map_with_errors(
        &self,
        goal_ids: &[&str],
    ) -> (HashMap<String, Vec<Comment>>, Vec<RelatedFetchError>) {
        let futures: Vec<_> = goal_ids.iter().map(|g| self.list_comments(g)).collect();
        let results = futures::future::join_all(futures).await;

        let mut map = HashMap::new();
        let mut errors = Vec::new();
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(resp) => {
                    map.insert(goal_ids[i].to_string(), resp.comments);
                }
                Err(e) => {
                    errors.push(RelatedFetchError {
                        kind: "comments",
                        goal_id: goal_ids[i].to_string(),
                        message: e.to_string(),
                    });
                }
            }
        }

        (map, errors)
    }
}
