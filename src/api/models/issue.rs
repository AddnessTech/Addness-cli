use serde::{Deserialize, Serialize};
use serde_json::Value;

// Goal Issue (v2 chat) API models.
//
// The v2 goal-issue wire format is snake_case (unlike the camelCase v1 comment
// API), so the field names below map 1:1 without `rename_all`.
// Backend reference: internal/goalissue/handler/endpoints/responses.go.

/// A goal issue (root chat message) or a reply message in an issue thread.
/// Both share the same wire shape; unread fields are only present on root
/// rows and `parent_message_id` only on replies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueMessage {
    pub id: String,
    #[serde(default)]
    pub objective_id: String,
    #[serde(default)]
    pub objective_title: String,
    #[serde(default)]
    pub organization_id: String,
    #[serde(default)]
    pub root_message_id: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub issue_number: i64,
    #[serde(default)]
    pub issue_path: String,
    #[serde(default)]
    pub reply_count: i64,
    #[serde(default)]
    pub last_message_at: String,
    #[serde(default)]
    pub last_activity_at: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub reactions: Vec<IssueReaction>,
    #[serde(default)]
    pub attachments: Vec<IssueAttachment>,
    #[serde(default)]
    pub mentions: Vec<IssueMention>,
    // Root issue rows only:
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unread_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unread_mention_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<String>,
    // Reply rows only:
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueReaction {
    pub emoji: String,
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub reacted_by_me: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueAttachment {
    pub id: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub file_type: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub file_size: i64,
    #[serde(default)]
    pub s3_key: String,
    /// Presigned GET URL; only present when resolved on read paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    // Upload fields are only emitted right after issuance in create responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_values: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueMention {
    pub org_member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// GET /api/v2/objectives/:id/issues
// GET /api/v2/goal-issues
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuesData {
    #[serde(default)]
    pub issues: Vec<IssueMessage>,
}

// GET /api/v2/objectives/:id/issues/:issueId/messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueThread {
    pub issue: IssueMessage,
    #[serde(default)]
    pub messages: Vec<IssueMessage>,
}

// GET /api/v2/goal-issues/search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueSearchData {
    #[serde(default)]
    pub messages: Vec<IssueMessage>,
    /// Matching goals; only returned on the first page.
    #[serde(default)]
    pub goals: Vec<IssueGoalHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueGoalHit {
    pub objective_id: String,
    #[serde(default)]
    pub objective_title: String,
    #[serde(default)]
    pub last_activity_at: String,
}

// GET /api/v2/objectives/:id/issues/:issueId/messages/:messageId/reactions/:emoji/users
// The backend returns member IDs only (display names are resolved client-side
// by the frontend; the CLI prints the IDs as-is).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueReactionUsers {
    #[serde(default)]
    pub member_ids: Vec<String>,
}

// GET /api/v2/goal-sections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSectionPage {
    #[serde(default)]
    pub sections: Vec<GoalSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<GoalSectionCursor>,
}

// GET /api/v2/goal-sections/pinned
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSectionsData {
    #[serde(default)]
    pub sections: Vec<GoalSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSection {
    pub objective_id: String,
    #[serde(default)]
    pub objective_title: String,
    #[serde(default)]
    pub last_activity_at: String,
    #[serde(default)]
    pub issue_count: i64,
    #[serde(default)]
    pub unread_count: i64,
    #[serde(default)]
    pub unread_mention_count: i64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub is_ai_running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_comment: Option<GoalSectionLastComment>,
    /// Owner payload (assignment + avatar resource); kept as raw JSON because
    /// the avatar variant object is display-oriented and not consumed by the CLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSectionLastComment {
    #[serde(default)]
    pub sender_id: String,
    #[serde(default)]
    pub content: String,
}

/// Keyset cursor echoed back for the next goal-sections page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSectionCursor {
    #[serde(default)]
    pub has_unread: bool,
    #[serde(default)]
    pub section_before: String,
    #[serde(default)]
    pub section_before_id: String,
    #[serde(default)]
    pub unread_as_of: String,
}

// GET /api/v2/goal-sections/unread-count
// Note: this endpoint's key is camelCase on the wire (unlike its siblings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnreadCommentCount {
    #[serde(rename = "unreadCommentCount")]
    pub unread_comment_count: i64,
}

// GET /api/v2/goal-sections/unread-mention-count
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnreadMentionCount {
    pub count: i64,
}

// POST /api/v2/objectives/:id/issues
// POST /api/v2/objectives/:id/issues/:issueId/messages
// PATCH (edit) endpoints reuse the same body; only `content` is consumed there.
// File attachments (S3 presigned upload flow) are intentionally not supported
// by the CLI yet, so `files` is never sent.
#[derive(Debug, Serialize)]
pub struct IssueMessageRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentioned_org_member_ids: Vec<String>,
}

// PATCH /api/v2/goal-issues/:issueId/resolution
#[derive(Debug, Serialize)]
pub struct IssueResolutionRequest {
    pub resolved: bool,
}

// PATCH /api/v2/goal-sections/:objectiveId/pinned
#[derive(Debug, Serialize)]
pub struct GoalPinnedRequest {
    pub pinned: bool,
}

// POST /api/v2/goal-issues/messages/preview
#[derive(Debug, Serialize)]
pub struct IssuePreviewRequest {
    pub ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{GoalSectionPage, IssueMessage, IssueThread, UnreadCommentCount};

    #[test]
    fn issue_message_deserializes_root_row() {
        let json = r#"{
            "id": "6a4f0b7e-6f0f-4f7e-9f0a-000000000001",
            "objective_id": "6a4f0b7e-6f0f-4f7e-9f0a-000000000002",
            "objective_title": "goal",
            "organization_id": "6a4f0b7e-6f0f-4f7e-9f0a-000000000003",
            "root_message_id": "6a4f0b7e-6f0f-4f7e-9f0a-000000000001",
            "content": "hello",
            "issue_number": 3,
            "issue_path": "/objectives/6a4f0b7e-6f0f-4f7e-9f0a-000000000002/3",
            "reply_count": 2,
            "last_message_at": "2026-07-15T00:00:00Z",
            "last_activity_at": "2026-07-15T00:00:00Z",
            "created_at": "2026-07-15T00:00:00Z",
            "updated_at": "2026-07-15T00:00:00Z",
            "reactions": [{"emoji": "👍", "count": 1, "reacted_by_me": true}],
            "attachments": [],
            "mentions": [{"org_member_id": "m-1", "name": "Alice"}],
            "unread_count": 1,
            "unread_mention_count": 0
        }"#;
        let msg: IssueMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.issue_number, 3);
        assert_eq!(msg.reply_count, 2);
        assert_eq!(msg.unread_count, Some(1));
        assert_eq!(msg.parent_message_id, None);
        assert_eq!(msg.reactions[0].emoji, "👍");
        assert!(msg.reactions[0].reacted_by_me);
        assert_eq!(msg.mentions[0].name.as_deref(), Some("Alice"));
    }

    #[test]
    fn issue_message_deserializes_reply_row_without_unread_fields() {
        let json = r#"{
            "id": "6a4f0b7e-6f0f-4f7e-9f0a-000000000009",
            "parent_message_id": "6a4f0b7e-6f0f-4f7e-9f0a-000000000001",
            "content": "reply"
        }"#;
        let msg: IssueMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.unread_count, None);
        assert_eq!(
            msg.parent_message_id.as_deref(),
            Some("6a4f0b7e-6f0f-4f7e-9f0a-000000000001")
        );
    }

    #[test]
    fn issue_thread_deserializes_issue_and_messages() {
        let json = r#"{
            "issue": {"id": "i-1", "content": "root"},
            "messages": [{"id": "m-1", "content": "r1", "parent_message_id": "i-1"}]
        }"#;
        let thread: IssueThread = serde_json::from_str(json).unwrap();
        assert_eq!(thread.issue.id, "i-1");
        assert_eq!(thread.messages.len(), 1);
    }

    #[test]
    fn goal_section_page_deserializes_with_and_without_cursor() {
        let with_cursor = r#"{
            "sections": [{
                "objective_id": "o-1",
                "objective_title": "goal",
                "last_activity_at": "2026-07-15T00:00:00Z",
                "issue_count": 4,
                "unread_count": 2,
                "unread_mention_count": 1,
                "status": "IN_PROGRESS",
                "is_ai_running": false,
                "last_comment": {"sender_id": "m-1", "content": "hi"}
            }],
            "next_cursor": {
                "has_unread": true,
                "section_before": "2026-07-15T00:00:00Z",
                "section_before_id": "o-1",
                "unread_as_of": "2026-07-15T00:00:00Z"
            }
        }"#;
        let page: GoalSectionPage = serde_json::from_str(with_cursor).unwrap();
        assert_eq!(page.sections.len(), 1);
        assert_eq!(page.sections[0].issue_count, 4);
        assert!(page.next_cursor.as_ref().unwrap().has_unread);

        let last_page: GoalSectionPage =
            serde_json::from_str(r#"{"sections": [], "next_cursor": null}"#).unwrap();
        assert!(last_page.next_cursor.is_none());
    }

    #[test]
    fn unread_comment_count_uses_camel_case_key() {
        let count: UnreadCommentCount =
            serde_json::from_str(r#"{"unreadCommentCount": 7}"#).unwrap();
        assert_eq!(count.unread_comment_count, 7);
    }
}
