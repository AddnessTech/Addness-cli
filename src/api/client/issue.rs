use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

use crate::api::{
    ApiClient, ApiResponse, GoalPinnedRequest, GoalSectionPage, GoalSectionsData, IssueMessage,
    IssueMessageRequest, IssuePreviewRequest, IssueReactionUsers, IssueResolutionRequest,
    IssueSearchData, IssueThread, IssuesData, ReactionRequest, UnreadCommentCount,
    UnreadMentionCount,
};

/// Keyset-cursor list parameters shared by the issue list/messages/search
/// endpoints (`limit`, `before` = RFC3339 timestamp, `before_id` = message ID).
#[derive(Default)]
pub struct IssueListParams<'a> {
    pub resolved: Option<bool>,
    pub limit: Option<u16>,
    pub before: Option<&'a str>,
    pub before_id: Option<&'a str>,
}

/// Query parameters for GET /api/v2/goal-sections.
#[derive(Default)]
pub struct GoalSectionListParams<'a> {
    pub resolved: Option<bool>,
    pub limit: Option<u16>,
    pub scope: Option<&'a str>,
    pub has_comments: bool,
    /// Continuation cursor echo values; the backend requires all of
    /// `has_unread` / `section_before` / `section_before_id` / `unread_as_of`
    /// on continuation pages (see next_cursor in the previous response).
    pub has_unread: Option<bool>,
    pub section_before: Option<&'a str>,
    pub section_before_id: Option<&'a str>,
    pub unread_as_of: Option<&'a str>,
}

// Response envelopes used by single-message endpoints.
#[derive(Deserialize)]
struct IssueEnvelope {
    issue: IssueMessage,
}

#[derive(Deserialize)]
struct MessageEnvelope {
    message: IssueMessage,
}

/// Percent-encode a URL path segment (RFC 3986 unreserved characters pass
/// through). Needed for emoji path parameters in reaction endpoints; also
/// keeps `/`, `?`, and `#` in user input from being parsed as URL structure.
pub(super) fn encode_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn issue_list_query_suffix(params: &IssueListParams<'_>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(resolved) = params.resolved {
            query.append_pair("resolved", if resolved { "true" } else { "false" });
        }
        if let Some(limit) = params.limit {
            query.append_pair("limit", &limit.to_string());
        }
        if let Some(before) = params.before {
            query.append_pair("before", before);
        }
        if let Some(before_id) = params.before_id {
            query.append_pair("before_id", before_id);
        }
        query.finish()
    };
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
}

fn goal_section_query_suffix(params: &GoalSectionListParams<'_>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(resolved) = params.resolved {
            query.append_pair("resolved", if resolved { "true" } else { "false" });
        }
        if let Some(limit) = params.limit {
            query.append_pair("limit", &limit.to_string());
        }
        if let Some(scope) = params.scope {
            query.append_pair("scope", scope);
        }
        if params.has_comments {
            query.append_pair("has_comments", "true");
        }
        if let Some(has_unread) = params.has_unread {
            query.append_pair("has_unread", if has_unread { "true" } else { "false" });
        }
        if let Some(section_before) = params.section_before {
            query.append_pair("section_before", section_before);
        }
        if let Some(section_before_id) = params.section_before_id {
            query.append_pair("section_before_id", section_before_id);
        }
        if let Some(unread_as_of) = params.unread_as_of {
            query.append_pair("unread_as_of", unread_as_of);
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
    /// GET /api/v2/objectives/:id/issues
    pub async fn list_objective_issues(
        &self,
        goal_id: &str,
        params: IssueListParams<'_>,
    ) -> Result<IssuesData> {
        let suffix = issue_list_query_suffix(&params);
        let path = format!("/api/v2/objectives/{goal_id}/issues{suffix}");
        let resp: ApiResponse<IssuesData> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/goal-issues (issues across all accessible goals)
    pub async fn list_all_issues(&self, params: IssueListParams<'_>) -> Result<IssuesData> {
        let suffix = issue_list_query_suffix(&params);
        let path = format!("/api/v2/goal-issues{suffix}");
        let resp: ApiResponse<IssuesData> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/objectives/:id/issues
    pub async fn create_issue(
        &self,
        goal_id: &str,
        content: &str,
        mentions: Vec<String>,
    ) -> Result<IssueMessage> {
        let path = format!("/api/v2/objectives/{goal_id}/issues");
        let body = IssueMessageRequest {
            content: content.to_string(),
            mentioned_org_member_ids: mentions,
        };
        let resp: ApiResponse<IssueEnvelope> = self.post(&path, &body).await?;
        Ok(resp.data.issue)
    }

    /// PATCH /api/v2/objectives/:id/issues/:issueId
    pub async fn edit_issue(
        &self,
        goal_id: &str,
        issue_id: &str,
        content: &str,
    ) -> Result<IssueMessage> {
        let path = format!("/api/v2/objectives/{goal_id}/issues/{issue_id}");
        let body = IssueMessageRequest {
            content: content.to_string(),
            mentioned_org_member_ids: Vec::new(),
        };
        let resp: ApiResponse<MessageEnvelope> = self.patch(&path, &body).await?;
        Ok(resp.data.message)
    }

    /// PUT /api/v2/objectives/:id/issues/:issueId/read (204)
    pub async fn mark_issue_read(&self, goal_id: &str, issue_id: &str) -> Result<()> {
        let path = format!("/api/v2/objectives/{goal_id}/issues/{issue_id}/read");
        self.put_empty_no_content(&path).await
    }

    /// GET /api/v2/objectives/:id/issues/:issueId/messages
    /// (`resolved` in params is ignored by this endpoint)
    pub async fn list_issue_messages(
        &self,
        goal_id: &str,
        issue_id: &str,
        params: IssueListParams<'_>,
    ) -> Result<IssueThread> {
        let suffix = issue_list_query_suffix(&params);
        let path = format!("/api/v2/objectives/{goal_id}/issues/{issue_id}/messages{suffix}");
        let resp: ApiResponse<IssueThread> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/objectives/:id/issues/:issueId/messages
    pub async fn post_issue_message(
        &self,
        goal_id: &str,
        issue_id: &str,
        content: &str,
        mentions: Vec<String>,
    ) -> Result<IssueMessage> {
        let path = format!("/api/v2/objectives/{goal_id}/issues/{issue_id}/messages");
        let body = IssueMessageRequest {
            content: content.to_string(),
            mentioned_org_member_ids: mentions,
        };
        let resp: ApiResponse<MessageEnvelope> = self.post(&path, &body).await?;
        Ok(resp.data.message)
    }

    /// PATCH /api/v2/objectives/:id/issues/:issueId/messages/:messageId
    pub async fn edit_issue_message(
        &self,
        goal_id: &str,
        issue_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<IssueMessage> {
        let path = format!("/api/v2/objectives/{goal_id}/issues/{issue_id}/messages/{message_id}");
        let body = IssueMessageRequest {
            content: content.to_string(),
            mentioned_org_member_ids: Vec::new(),
        };
        let resp: ApiResponse<MessageEnvelope> = self.patch(&path, &body).await?;
        Ok(resp.data.message)
    }

    /// POST /api/v2/objectives/:id/issues/:issueId/messages/:messageId/reactions
    /// Returns the updated message (unwrapped on the wire, unlike other endpoints).
    pub async fn add_issue_reaction(
        &self,
        goal_id: &str,
        issue_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<IssueMessage> {
        let path = format!(
            "/api/v2/objectives/{goal_id}/issues/{issue_id}/messages/{message_id}/reactions"
        );
        let body = ReactionRequest {
            emoji: emoji.to_string(),
        };
        let resp: ApiResponse<IssueMessage> = self.post(&path, &body).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v2/objectives/:id/issues/:issueId/messages/:messageId/reactions/:emoji (204)
    pub async fn remove_issue_reaction(
        &self,
        goal_id: &str,
        issue_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<()> {
        let emoji = encode_path_segment(emoji);
        let path = format!(
            "/api/v2/objectives/{goal_id}/issues/{issue_id}/messages/{message_id}/reactions/{emoji}"
        );
        self.delete_no_body(&path).await
    }

    /// GET /api/v2/objectives/:id/issues/:issueId/messages/:messageId/reactions/:emoji/users
    pub async fn list_issue_reaction_users(
        &self,
        goal_id: &str,
        issue_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<IssueReactionUsers> {
        let emoji = encode_path_segment(emoji);
        let path = format!(
            "/api/v2/objectives/{goal_id}/issues/{issue_id}/messages/{message_id}/reactions/{emoji}/users"
        );
        let resp: ApiResponse<IssueReactionUsers> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/goal-issues/search
    pub async fn search_issue_messages(
        &self,
        query_text: &str,
        params: IssueListParams<'_>,
    ) -> Result<IssueSearchData> {
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            query.append_pair("q", query_text);
            if let Some(limit) = params.limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(before) = params.before {
                query.append_pair("before", before);
            }
            if let Some(before_id) = params.before_id {
                query.append_pair("before_id", before_id);
            }
            query.finish()
        };
        let path = format!("/api/v2/goal-issues/search?{query}");
        let resp: ApiResponse<IssueSearchData> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/goal-issues/messages/preview
    /// Response entries are polymorphic (`available: true|false` change the
    /// shape), so they are surfaced as raw JSON.
    pub async fn preview_issue_messages(&self, ids: Vec<String>) -> Result<Value> {
        let body = IssuePreviewRequest { ids };
        let resp: ApiResponse<Value> = self
            .post("/api/v2/goal-issues/messages/preview", &body)
            .await?;
        Ok(resp.data)
    }

    /// PATCH /api/v2/goal-issues/:issueId/resolution
    pub async fn set_issue_resolution(
        &self,
        issue_id: &str,
        resolved: bool,
    ) -> Result<IssueMessage> {
        let path = format!("/api/v2/goal-issues/{issue_id}/resolution");
        let body = IssueResolutionRequest { resolved };
        let resp: ApiResponse<IssueMessage> = self.patch(&path, &body).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/goal-sections
    pub async fn list_goal_sections(
        &self,
        params: GoalSectionListParams<'_>,
    ) -> Result<GoalSectionPage> {
        let suffix = goal_section_query_suffix(&params);
        let path = format!("/api/v2/goal-sections{suffix}");
        let resp: ApiResponse<GoalSectionPage> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/goal-sections/pinned
    pub async fn list_pinned_goal_sections(&self) -> Result<GoalSectionsData> {
        let resp: ApiResponse<GoalSectionsData> = self.get("/api/v2/goal-sections/pinned").await?;
        Ok(resp.data)
    }

    /// GET /api/v2/goal-sections/unread-count
    pub async fn count_unread_issue_comments(&self) -> Result<UnreadCommentCount> {
        let resp: ApiResponse<UnreadCommentCount> =
            self.get("/api/v2/goal-sections/unread-count").await?;
        Ok(resp.data)
    }

    /// GET /api/v2/goal-sections/unread-mention-count
    pub async fn count_unread_issue_mentions(&self) -> Result<UnreadMentionCount> {
        let resp: ApiResponse<UnreadMentionCount> = self
            .get("/api/v2/goal-sections/unread-mention-count")
            .await?;
        Ok(resp.data)
    }

    /// PATCH /api/v2/goal-sections/:objectiveId/pinned (204)
    pub async fn set_goal_section_pinned(&self, goal_id: &str, pinned: bool) -> Result<()> {
        let path = format!("/api/v2/goal-sections/{goal_id}/pinned");
        let body = GoalPinnedRequest { pinned };
        self.patch_no_content(&path, &body).await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GoalSectionListParams, IssueListParams, encode_path_segment, goal_section_query_suffix,
        issue_list_query_suffix,
    };

    #[test]
    fn encode_path_segment_passes_unreserved_chars() {
        assert_eq!(encode_path_segment("abc-XYZ_0.9~"), "abc-XYZ_0.9~");
    }

    #[test]
    fn encode_path_segment_encodes_emoji_and_url_structure() {
        assert_eq!(encode_path_segment("👍"), "%F0%9F%91%8D");
        assert_eq!(encode_path_segment("a/b?c#d"), "a%2Fb%3Fc%23d");
        assert_eq!(encode_path_segment("100%"), "100%25");
    }

    #[test]
    fn issue_list_query_suffix_is_empty_without_params() {
        assert_eq!(issue_list_query_suffix(&IssueListParams::default()), "");
    }

    #[test]
    fn issue_list_query_suffix_encodes_all_params() {
        let suffix = issue_list_query_suffix(&IssueListParams {
            resolved: Some(false),
            limit: Some(20),
            before: Some("2026-07-15T00:00:00Z"),
            before_id: Some("msg-1"),
        });
        assert_eq!(
            suffix,
            "?resolved=false&limit=20&before=2026-07-15T00%3A00%3A00Z&before_id=msg-1"
        );
    }

    #[test]
    fn goal_section_query_suffix_is_empty_without_params() {
        assert_eq!(
            goal_section_query_suffix(&GoalSectionListParams::default()),
            ""
        );
    }

    #[test]
    fn goal_section_query_suffix_encodes_all_params() {
        let suffix = goal_section_query_suffix(&GoalSectionListParams {
            resolved: Some(true),
            limit: Some(30),
            scope: Some("today"),
            has_comments: true,
            has_unread: Some(false),
            section_before: Some("2026-07-15T00:00:00Z"),
            section_before_id: Some("obj-1"),
            unread_as_of: Some("2026-07-15T00:00:00Z"),
        });
        assert_eq!(
            suffix,
            "?resolved=true&limit=30&scope=today&has_comments=true&has_unread=false\
             &section_before=2026-07-15T00%3A00%3A00Z&section_before_id=obj-1\
             &unread_as_of=2026-07-15T00%3A00%3A00Z"
        );
    }
}
