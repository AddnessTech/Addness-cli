use std::{collections::HashMap, sync::OnceLock};

use crate::api::{
    ApiClient, ApiResponse, Comment, CommentContextResponse, CommentDetail, CommentMutationResult,
    CommentsResponse, CreateCommentRequest, ReactionRequest, RelatedFetchError,
    UpdateCommentRequest,
};
use anyhow::{Result, bail};
use regex::Regex;
use serde_json::Value;

/// Goal-issue v2 deliberately uses a smaller chat-message limit than the
/// legacy comment API (4,000 vs 10,000 Unicode scalar values).
const V2_COMMENT_CONTENT_LIMIT: usize = 4_000;

fn inline_mention_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)@([a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12})")
            .expect("inline mention regex must compile")
    })
}

fn inline_mention_ids(content: &str) -> Vec<String> {
    inline_mention_regex()
        .captures_iter(content)
        .filter_map(|captures| captures.get(1))
        .map(|id| id.as_str().to_ascii_lowercase())
        .collect()
}

/// Whether v2 create/reply can represent a legacy comment request without
/// changing content or mention semantics. Goal-issue v2 strips inline UUID
/// mentions not included in `mentioned_org_member_ids` and prepends missing
/// `@UUID`s, so account for both rules before selecting the route.
fn v2_create_can_represent(content: &str, mentions: &[String]) -> bool {
    let normalized_mentions: Vec<String> = mentions
        .iter()
        .map(|id| id.trim().to_ascii_lowercase())
        .collect();
    if inline_mention_ids(content)
        .iter()
        .any(|id| !normalized_mentions.contains(id))
    {
        return false;
    }

    let trimmed = content.trim();
    let normalized_content = trimmed.to_ascii_lowercase();
    let missing: Vec<&str> = normalized_mentions
        .iter()
        .map(String::as_str)
        .filter(|id| !normalized_content.contains(&format!("@{id}")))
        .collect();
    let prefix_chars: usize = missing.iter().map(|id| 1 + id.chars().count()).sum();
    let separator_chars = if missing.is_empty() {
        0
    } else {
        missing.len().saturating_sub(1) + usize::from(!trimmed.is_empty())
    };

    trimmed.chars().count() + prefix_chars + separator_chars <= V2_COMMENT_CONTENT_LIMIT
}

/// Goal-issue v2 edits content only; it intentionally does not rewrite the
/// mention rows. Use it only when neither the existing nor requested message
/// has mentions, so the legacy update contract remains intact.
fn v2_edit_can_represent(
    content: &str,
    requested_mentions: &[String],
    current_mentions: &[String],
) -> bool {
    requested_mentions.is_empty()
        && current_mentions.is_empty()
        && inline_mention_ids(content).is_empty()
        && content.trim().chars().count() <= V2_COMMENT_CONTENT_LIMIT
}

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

/// Filters for the global comment feed (GET /api/v1/team/comments).
/// Unlike `ListCommentsParams` the goal is optional; when set, the backend
/// requires `commentableType=objective` alongside `commentableId`.
#[derive(Default)]
pub struct ListAllCommentsParams<'a> {
    pub goal_id: Option<&'a str>,
    pub author_id: Option<&'a str>,
    pub parent_id: Option<&'a str>,
    pub resolved: Option<bool>,
    pub limit: Option<u16>,
    pub offset: Option<u64>,
    pub sort: Option<&'a str>,
    pub include_replies: bool,
}

fn list_all_comments_query_suffix(params: &ListAllCommentsParams<'_>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(goal_id) = params.goal_id {
            query.append_pair("commentableType", "objective");
            query.append_pair("commentableId", goal_id);
        }
        if let Some(author_id) = params.author_id {
            query.append_pair("author_id", author_id);
        }
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
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
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

    /// GET /api/v1/team/comments (global comment feed with filters)
    pub async fn list_all_comments(
        &self,
        params: ListAllCommentsParams<'_>,
    ) -> Result<CommentsResponse> {
        let suffix = list_all_comments_query_suffix(&params);
        let path = format!("/api/v1/team/comments{suffix}");
        let resp: ApiResponse<CommentsResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn get_comment(&self, comment_id: &str) -> Result<CommentDetail> {
        let path = format!("/api/v1/team/comments/{comment_id}");
        let resp: ApiResponse<CommentDetail> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/comments/:id/context
    /// Surrounding comments around a target comment (notification highlight).
    pub async fn get_comment_context(
        &self,
        comment_id: &str,
        radius: Option<u8>,
        resolved: Option<bool>,
    ) -> Result<CommentContextResponse> {
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            if let Some(radius) = radius {
                query.append_pair("radius", &radius.to_string());
            }
            if let Some(resolved) = resolved {
                query.append_pair("resolved", if resolved { "true" } else { "false" });
            }
            query.finish()
        };
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let path = format!("/api/v1/team/comments/{comment_id}/context{suffix}");
        let resp: ApiResponse<CommentContextResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// Read reaction users through Goal Issue v2 when the message can be
    /// resolved there. V2 returns `{member_ids: [...]}`; the V1-only fallback
    /// for other commentable types returns full member resources (or `null`),
    /// so the compatibility surface remains raw JSON.
    pub async fn get_comment_reaction_users(&self, comment_id: &str, emoji: &str) -> Result<Value> {
        if let Some(scope) = self.find_issue_message_scope(comment_id).await? {
            let users = self
                .list_issue_reaction_users(
                    &scope.objective_id,
                    &scope.issue_id,
                    &scope.message_id,
                    emoji,
                )
                .await?;
            return Ok(serde_json::to_value(users)?);
        }

        // V1-only fallback for comments outside the objective goal-issue model.
        let emoji = super::issue::encode_path_segment(emoji);
        let path = format!("/api/v1/team/comments/{comment_id}/reactions/{emoji}/users");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// Create a root objective comment through Goal Issue v2 when its content
    /// contract is compatible, otherwise use the documented V1-only fallback.
    pub async fn create_comment(&self, goal_id: &str, body: &str) -> Result<CommentMutationResult> {
        self.create_comment_with_options(goal_id, body, None, Vec::new())
            .await
    }

    pub async fn create_comment_with_options(
        &self,
        goal_id: &str,
        body: &str,
        parent_id: Option<String>,
        mentions: Vec<String>,
    ) -> Result<CommentMutationResult> {
        if v2_create_can_represent(body, &mentions) {
            if let Some(parent_id) = parent_id.as_deref() {
                if let Some(scope) = self.find_issue_message_scope(parent_id).await? {
                    if !scope.objective_id.eq_ignore_ascii_case(goal_id) {
                        bail!(
                            "parent comment {parent_id} belongs to goal {}, not {goal_id}",
                            scope.objective_id
                        );
                    }
                    let message = self
                        .post_issue_message(goal_id, &scope.issue_id, body, mentions.clone())
                        .await?;
                    return Ok(CommentMutationResult::V2(Box::new(message)));
                }
            } else {
                let message = self.create_issue(goal_id, body, mentions.clone()).await?;
                return Ok(CommentMutationResult::V2(Box::new(message)));
            }
        }

        // V1-only compatibility: long content, inline mentions that v2 would
        // strip, or a non-objective parent that goal-issue preview cannot map.
        let req = CreateCommentRequest {
            commentable_type: "objective".to_string(),
            commentable_id: goal_id.to_string(),
            content: body.to_string(),
            parent_id,
            mentions,
        };
        let resp: ApiResponse<Comment> = self.post("/api/v1/team/comments", &req).await?;
        Ok(CommentMutationResult::V1(Box::new(resp.data)))
    }

    /// Edit an objective comment through Goal Issue v2 only when mention rows
    /// remain empty; legacy update remains authoritative for mention changes.
    pub async fn update_comment(
        &self,
        comment_id: &str,
        content: &str,
        mentions: Vec<String>,
    ) -> Result<CommentMutationResult> {
        if let Some(scope) = self.find_issue_message_scope(comment_id).await?
            && v2_edit_can_represent(content, &mentions, &scope.mentioned_member_ids)
        {
            let message = if scope.is_root {
                self.edit_issue(&scope.objective_id, &scope.issue_id, content)
                    .await?
            } else {
                self.edit_issue_message(
                    &scope.objective_id,
                    &scope.issue_id,
                    &scope.message_id,
                    content,
                )
                .await?
            };
            return Ok(CommentMutationResult::V2(Box::new(message)));
        }

        // V1-only compatibility: legacy update owns mention-row replacement
        // and accepts content between 4,001 and 10,000 characters.
        let path = format!("/api/v1/team/comments/{comment_id}");
        let body = UpdateCommentRequest {
            content: content.to_string(),
            mentions,
        };
        let resp: ApiResponse<Comment> = self.put(&path, &body).await?;
        Ok(CommentMutationResult::V1(Box::new(resp.data)))
    }

    pub async fn delete_comment(&self, comment_id: &str) -> Result<()> {
        if let Some(scope) = self.find_issue_message_scope(comment_id).await? {
            return if scope.is_root {
                self.delete_issue(&scope.objective_id, &scope.issue_id)
                    .await
            } else {
                self.delete_issue_message(&scope.objective_id, &scope.issue_id, &scope.message_id)
                    .await
            };
        }

        // V1-only fallback for non-objective comments.
        let path = format!("/api/v1/team/comments/{comment_id}");
        self.delete_no_body(&path).await
    }

    pub async fn resolve_comment(&self, comment_id: &str) -> Result<CommentMutationResult> {
        if let Some(scope) = self.find_issue_message_scope(comment_id).await?
            && scope.is_root
        {
            let message = self.set_issue_resolution(&scope.issue_id, true).await?;
            return Ok(CommentMutationResult::V2(Box::new(message)));
        }

        // V1-only fallback for non-objective comments and reply IDs (v2
        // resolution deliberately accepts roots only).
        let path = format!("/api/v1/team/comments/{comment_id}/resolve");
        let resp: ApiResponse<Comment> = self.patch_empty(&path).await?;
        Ok(CommentMutationResult::V1(Box::new(resp.data)))
    }

    pub async fn unresolve_comment(&self, comment_id: &str) -> Result<CommentMutationResult> {
        if let Some(scope) = self.find_issue_message_scope(comment_id).await?
            && scope.is_root
        {
            let message = self.set_issue_resolution(&scope.issue_id, false).await?;
            return Ok(CommentMutationResult::V2(Box::new(message)));
        }

        let path = format!("/api/v1/team/comments/{comment_id}/unresolve");
        let resp: ApiResponse<Comment> = self.patch_empty(&path).await?;
        Ok(CommentMutationResult::V1(Box::new(resp.data)))
    }

    pub async fn add_reaction(&self, comment_id: &str, emoji: &str) -> Result<()> {
        if let Some(scope) = self.find_issue_message_scope(comment_id).await? {
            self.add_issue_reaction(
                &scope.objective_id,
                &scope.issue_id,
                &scope.message_id,
                emoji,
            )
            .await?;
            return Ok(());
        }

        // V1-only fallback for non-objective comments.
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

#[cfg(test)]
mod tests {
    use super::{
        ListAllCommentsParams, list_all_comments_query_suffix, v2_create_can_represent,
        v2_edit_can_represent,
    };

    #[test]
    fn list_all_comments_query_suffix_is_empty_without_params() {
        assert_eq!(
            list_all_comments_query_suffix(&ListAllCommentsParams::default()),
            ""
        );
    }

    #[test]
    fn list_all_comments_query_suffix_pairs_goal_with_commentable_type() {
        assert_eq!(
            list_all_comments_query_suffix(&ListAllCommentsParams {
                goal_id: Some("goal-1"),
                ..Default::default()
            }),
            "?commentableType=objective&commentableId=goal-1"
        );
    }

    #[test]
    fn list_all_comments_query_suffix_encodes_all_params() {
        let suffix = list_all_comments_query_suffix(&ListAllCommentsParams {
            goal_id: Some("goal-1"),
            author_id: Some("author-1"),
            parent_id: Some("parent-1"),
            resolved: Some(false),
            limit: Some(50),
            offset: Some(10),
            sort: Some("desc"),
            include_replies: true,
        });
        assert_eq!(
            suffix,
            "?commentableType=objective&commentableId=goal-1&author_id=author-1\
             &parentId=parent-1&resolved=false&limit=50&offset=10&sort=desc\
             &include_replies=true"
        );
    }

    #[test]
    fn v2_create_supports_regular_and_explicit_mention_content() {
        let member = "550e8400-e29b-41d4-a716-446655440000".to_string();
        assert!(v2_create_can_represent("hello", &[]));
        assert!(v2_create_can_represent(
            "hello",
            std::slice::from_ref(&member)
        ));
        assert!(v2_create_can_represent(
            &format!("hello @{member}"),
            std::slice::from_ref(&member)
        ));
    }

    #[test]
    fn v2_create_rejects_unlisted_inline_mentions_and_oversized_prepared_content() {
        let member = "550e8400-e29b-41d4-a716-446655440000".to_string();
        assert!(!v2_create_can_represent(&format!("hello @{member}"), &[]));
        assert!(!v2_create_can_represent(&"a".repeat(4_001), &[]));
        assert!(!v2_create_can_represent(
            &"a".repeat(3_970),
            std::slice::from_ref(&member)
        ));
    }

    #[test]
    fn v2_edit_requires_mention_free_content_and_state() {
        let member = "550e8400-e29b-41d4-a716-446655440000".to_string();
        assert!(v2_edit_can_represent("hello", &[], &[]));
        assert!(!v2_edit_can_represent(
            "hello",
            std::slice::from_ref(&member),
            &[]
        ));
        assert!(!v2_edit_can_represent(
            "hello",
            &[],
            std::slice::from_ref(&member)
        ));
        assert!(!v2_edit_can_represent(
            &format!("hello @{member}"),
            &[],
            &[]
        ));
        assert!(!v2_edit_can_represent(&"a".repeat(4_001), &[], &[]));
    }
}
