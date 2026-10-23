use serde::{Deserialize, Serialize};

use crate::api::{IssueAttachment, IssueReaction};

// Organization chat (Org Chat, `/api/v2/chat/*`) API models.
//
// The wire format is snake_case (same convention as goal-issue v2), except for
// the pending-invitation payload which is camelCase (see `ChatPendingInvitation`
// below). Backend reference: internal/orgchat/handler/endpoints/responses.go.

/// A DM or group chat room.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRoom {
    pub id: String,
    #[serde(default)]
    pub organization_id: String,
    #[serde(default)]
    pub room_type: String,
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub is_member: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub unread_count: i64,
    #[serde(default)]
    pub unread_mention_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dm_pair_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message: Option<ChatLastMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatLastMessage {
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<String>,
}

/// A chat-room membership row (distinct from the organization member roster).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRoomMember {
    pub id: String,
    #[serde(default)]
    pub chat_room_id: String,
    #[serde(default)]
    pub organization_member_id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub joined_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_at: Option<String>,
}

/// A pending group invitation addressed to the caller. Unlike the rest of the
/// org-chat wire format, this payload's keys are camelCase (see
/// pendingInvitationsJSON in the backend).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatPendingInvitation {
    pub id: String,
    #[serde(rename = "roomId")]
    pub room_id: String,
    #[serde(default, rename = "roomName")]
    pub room_name: String,
    #[serde(default, rename = "inviterName")]
    pub inviter_name: String,
    #[serde(default, rename = "createdAt")]
    pub created_at: String,
    #[serde(default, rename = "expiresAt")]
    pub expires_at: String,
}

/// A message posted to a room (root or reply; org-chat threads are single
/// level so `parent_message_id` marks a reply-to reference, not a thread root).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    #[serde(default)]
    pub chat_room_id: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub reactions: Vec<IssueReaction>,
    #[serde(default)]
    pub attachments: Vec<IssueAttachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

// GET /api/v2/chat/rooms
// GET /api/v2/chat/rooms/public
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRoomsData {
    #[serde(default)]
    pub rooms: Vec<ChatRoom>,
}

// GET /api/v2/chat/rooms/:roomId/members
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRoomMembersData {
    #[serde(default)]
    pub members: Vec<ChatRoomMember>,
}

// GET /api/v2/chat/invitations/pending
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatPendingInvitationsData {
    #[serde(default)]
    pub invitations: Vec<ChatPendingInvitation>,
}

// GET /api/v2/chat/rooms/:roomId/messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessagesData {
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
}

// GET /api/v2/chat/search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSearchData {
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub rooms: Vec<ChatRoom>,
}

// GET /api/v2/chat/rooms/unread-count
// Note: this endpoint's key is camelCase on the wire (unlike its siblings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatUnreadCount {
    #[serde(rename = "unreadCount")]
    pub unread_count: i64,
}

// POST /api/v2/chat/dms
#[derive(Debug, Serialize)]
pub struct CreateDmRequest {
    pub partner_id: String,
}

// POST /api/v2/chat/groups
#[derive(Debug, Serialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub visibility: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub member_ids: Vec<String>,
}

// PATCH /api/v2/chat/rooms/:roomId
#[derive(Debug, Serialize)]
pub struct RenameRoomRequest {
    pub name: String,
}

// POST /api/v2/chat/rooms/:roomId/invitations
#[derive(Debug, Serialize)]
pub struct ChatInviteMembersRequest {
    pub invited_member_ids: Vec<String>,
}

// PATCH /api/v2/chat/rooms/:roomId/hidden
#[derive(Debug, Serialize)]
pub struct ChatHideRoomRequest {
    pub hidden: bool,
}

// PUT /api/v2/chat/rooms/:roomId/read
#[derive(Debug, Serialize)]
pub struct ChatMarkRoomReadRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

// POST /api/v2/chat/rooms/:roomId/messages
// PATCH /api/v2/chat/rooms/:roomId/messages/:messageId (only `content` is read)
// File attachments (S3 presigned upload flow, `files[]`) are intentionally not
// supported by the CLI yet, matching the goal-issue message precedent.
#[derive(Debug, Serialize)]
pub struct ChatMessageRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentioned_org_member_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{ChatMessage, ChatPendingInvitation, ChatRoom, ChatUnreadCount};

    #[test]
    fn chat_room_deserializes_group_room_with_last_message() {
        let json = r#"{
            "id": "6a4f0b7e-6f0f-4f7e-9f0a-000000000001",
            "organization_id": "6a4f0b7e-6f0f-4f7e-9f0a-000000000002",
            "room_type": "group",
            "visibility": "public",
            "is_member": true,
            "created_at": "2026-07-15T00:00:00Z",
            "updated_at": "2026-07-15T00:00:00Z",
            "unread_count": 3,
            "unread_mention_count": 1,
            "name": "general",
            "icon_url": "https://example.com/icon.png",
            "last_message": {"content": "hi", "sender_id": "m-1"}
        }"#;
        let room: ChatRoom = serde_json::from_str(json).unwrap();
        assert_eq!(room.room_type, "group");
        assert_eq!(room.name.as_deref(), Some("general"));
        assert_eq!(room.last_message.unwrap().content, "hi");
    }

    #[test]
    fn chat_room_deserializes_dm_room_without_optional_fields() {
        let json = r#"{
            "id": "r-1",
            "room_type": "dm",
            "visibility": "private",
            "is_member": true,
            "dm_pair_key": "a:b"
        }"#;
        let room: ChatRoom = serde_json::from_str(json).unwrap();
        assert_eq!(room.dm_pair_key.as_deref(), Some("a:b"));
        assert!(room.name.is_none());
        assert!(room.last_message.is_none());
    }

    #[test]
    fn chat_message_deserializes_reply_with_reactions_and_attachments() {
        let json = r#"{
            "id": "m-1",
            "chat_room_id": "r-1",
            "content": "hello",
            "parent_message_id": "m-0",
            "sender_id": "member-1",
            "reactions": [{"emoji": "👍", "count": 2, "reacted_by_me": false}],
            "attachments": [{"id": "a-1", "file_name": "x.png", "s3_key": "k", "url": "https://x"}]
        }"#;
        let message: ChatMessage = serde_json::from_str(json).unwrap();
        assert_eq!(message.parent_message_id.as_deref(), Some("m-0"));
        assert_eq!(message.reactions[0].count, 2);
        assert_eq!(message.attachments[0].file_name, "x.png");
    }

    #[test]
    fn chat_pending_invitation_uses_camel_case_keys() {
        let json = r#"{
            "id": "i-1",
            "roomId": "r-1",
            "roomName": "general",
            "inviterName": "Alice",
            "createdAt": "2026-07-15T00:00:00Z",
            "expiresAt": "2026-07-22T00:00:00Z"
        }"#;
        let invitation: ChatPendingInvitation = serde_json::from_str(json).unwrap();
        assert_eq!(invitation.room_id, "r-1");
        assert_eq!(invitation.inviter_name, "Alice");
    }

    #[test]
    fn chat_unread_count_uses_camel_case_key() {
        let count: ChatUnreadCount = serde_json::from_str(r#"{"unreadCount": 5}"#).unwrap();
        assert_eq!(count.unread_count, 5);
    }
}
