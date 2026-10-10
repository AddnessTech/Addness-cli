use crate::api::{
    ApiClient, ApiResponse, Comment, CommentContextResponse, CommentDetail, CommentsResponse,
    CreateCommentRequest, ReactionRequest, RelatedFetchError, UpdateCommentRequest,
};
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

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

/// 投稿者別のコメント一覧。ゴールの絞り込みには ListCommentsParams を使う。
pub struct ListAllCommentsParams<'a> {
    pub author_id: &'a str,
    pub resolved: Option<bool>,
    pub limit: Option<u16>,
    pub offset: Option<u64>,
}

fn list_all_comments_query_suffix(params: &ListAllCommentsParams<'_>) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    query.append_pair("author_id", params.author_id);
    if let Some(value) = params.resolved {
        query.append_pair("resolved", &value.to_string());
    }
    if let Some(value) = params.limit {
        query.append_pair("limit", &value.to_string());
    }
    if let Some(value) = params.offset {
        query.append_pair("offset", &value.to_string());
    }
    format!("?{}", query.finish())
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

    pub async fn list_all_comments(
        &self,
        params: ListAllCommentsParams<'_>,
    ) -> Result<CommentsResponse> {
        let suffix = list_all_comments_query_suffix(&params);
        self.get(&format!("/api/v2/comments{suffix}")).await
    }

    pub async fn get_comment(&self, comment_id: &str) -> Result<CommentDetail> {
        self.get(&format!("/api/v2/comments/{comment_id}")).await
    }

    pub async fn get_comment_context(
        &self,
        comment_id: &str,
        radius: Option<u8>,
        resolved: Option<bool>,
    ) -> Result<CommentContextResponse> {
        let suffix = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            if let Some(radius) = radius {
                query.append_pair("radius", &radius.to_string());
            }
            if let Some(resolved) = resolved {
                query.append_pair("resolved", &resolved.to_string());
            }
            query.finish()
        };
        self.get(&format!("/api/v2/comments/{comment_id}/context?{suffix}"))
            .await
    }

    pub async fn get_comment_reaction_users(&self, comment_id: &str, emoji: &str) -> Result<Value> {
        let emoji = super::issue::encode_path_segment(emoji);
        self.get(&format!(
            "/api/v2/comments/{comment_id}/reactions/{emoji}/users"
        ))
        .await
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
            content: body.to_string(),
            parent_id,
            mentions,
        };
        self.post(&format!("/api/v2/objectives/{goal_id}/comments"), &req)
            .await
    }

    pub async fn update_comment(
        &self,
        comment_id: &str,
        content: &str,
        mentions: Vec<String>,
    ) -> Result<Comment> {
        self.put(
            &format!("/api/v2/comments/{comment_id}"),
            &UpdateCommentRequest {
                content: content.to_string(),
                mentions,
            },
        )
        .await
    }

    pub async fn delete_comment(&self, comment_id: &str) -> Result<()> {
        self.delete_no_body(&format!("/api/v2/comments/{comment_id}"))
            .await
    }

    pub async fn resolve_comment(&self, comment_id: &str) -> Result<Comment> {
        self.patch_empty(&format!("/api/v2/comments/{comment_id}/resolve"))
            .await
    }

    pub async fn unresolve_comment(&self, comment_id: &str) -> Result<Comment> {
        self.patch_empty(&format!("/api/v2/comments/{comment_id}/unresolve"))
            .await
    }

    pub async fn add_reaction(&self, comment_id: &str, emoji: &str) -> Result<()> {
        self.post_no_content(
            &format!("/api/v2/comments/{comment_id}/reactions"),
            &ReactionRequest {
                emoji: emoji.to_string(),
            },
        )
        .await
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
