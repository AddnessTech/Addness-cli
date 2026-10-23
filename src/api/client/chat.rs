use anyhow::Result;

use super::issue::encode_path_segment;
use crate::api::{
    ApiClient, ApiResponse, ChatHideRoomRequest, ChatInviteMembersRequest, ChatMarkRoomReadRequest,
    ChatMessage, ChatMessageRequest, ChatMessagesData, ChatPendingInvitationsData, ChatRoom,
    ChatRoomMembersData, ChatRoomsData, ChatSearchData, ChatUnreadCount, CreateDmRequest,
    CreateGroupRequest, ReactionRequest, RenameRoomRequest,
};

/// Keyset-cursor list parameters for `GET /api/v2/chat/rooms` (and `/rooms/public`).
#[derive(Default)]
pub struct ChatRoomListParams<'a> {
    /// Filter by room type: `dm` or `group`. Empty returns both.
    pub room_type: Option<&'a str>,
    pub limit: Option<u16>,
    pub before: Option<&'a str>,
    pub before_id: Option<&'a str>,
    /// When true, return only rooms currently hidden by the caller.
    pub hidden: bool,
}

/// Keyset-cursor list parameters for `GET /api/v2/chat/rooms/:roomId/messages`.
#[derive(Default)]
pub struct ChatMessageListParams<'a> {
    pub parent_message_id: Option<&'a str>,
    pub limit: Option<u16>,
    pub before: Option<&'a str>,
    pub before_id: Option<&'a str>,
}

/// Query parameters for `GET /api/v2/chat/search`.
#[derive(Default)]
pub struct ChatSearchParams<'a> {
    pub limit: Option<u16>,
    pub before: Option<&'a str>,
    pub before_id: Option<&'a str>,
}

fn chat_room_list_query_suffix(params: &ChatRoomListParams<'_>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(room_type) = params.room_type {
            query.append_pair("room_type", room_type);
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
        if params.hidden {
            query.append_pair("hidden", "true");
        }
        query.finish()
    };
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
}

fn chat_message_list_query_suffix(params: &ChatMessageListParams<'_>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(parent_message_id) = params.parent_message_id {
            query.append_pair("parent_message_id", parent_message_id);
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

impl ApiClient {
    /// GET /api/v2/chat/search
    pub async fn search_chat_messages(
        &self,
        query_text: &str,
        params: ChatSearchParams<'_>,
    ) -> Result<ChatSearchData> {
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
        let path = format!("/api/v2/chat/search?{query}");
        let resp: ApiResponse<ChatSearchData> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/chat/rooms
    pub async fn list_chat_rooms(&self, params: ChatRoomListParams<'_>) -> Result<Vec<ChatRoom>> {
        let suffix = chat_room_list_query_suffix(&params);
        let path = format!("/api/v2/chat/rooms{suffix}");
        let resp: ApiResponse<ChatRoomsData> = self.get(&path).await?;
        Ok(resp.data.rooms)
    }

    /// GET /api/v2/chat/rooms/public
    pub async fn list_public_chat_groups(
        &self,
        params: ChatRoomListParams<'_>,
    ) -> Result<Vec<ChatRoom>> {
        let suffix = chat_room_list_query_suffix(&params);
        let path = format!("/api/v2/chat/rooms/public{suffix}");
        let resp: ApiResponse<ChatRoomsData> = self.get(&path).await?;
        Ok(resp.data.rooms)
    }

    /// GET /api/v2/chat/rooms/unread-count
    pub async fn count_chat_group_unread(&self) -> Result<ChatUnreadCount> {
        let resp: ApiResponse<ChatUnreadCount> =
            self.get("/api/v2/chat/rooms/unread-count").await?;
        Ok(resp.data)
    }

    /// GET /api/v2/chat/rooms/:roomId
    pub async fn get_chat_room(&self, room_id: &str) -> Result<ChatRoom> {
        let path = format!("/api/v2/chat/rooms/{room_id}");
        let resp: ApiResponse<ChatRoom> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PATCH /api/v2/chat/rooms/:roomId
    pub async fn rename_chat_room(&self, room_id: &str, name: &str) -> Result<ChatRoom> {
        let path = format!("/api/v2/chat/rooms/{room_id}");
        let body = RenameRoomRequest {
            name: name.to_string(),
        };
        let resp: ApiResponse<ChatRoom> = self.patch(&path, &body).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v2/chat/rooms/:roomId (204)
    pub async fn delete_chat_group(&self, room_id: &str) -> Result<()> {
        let path = format!("/api/v2/chat/rooms/{room_id}");
        self.delete_no_body(&path).await
    }

    /// GET /api/v2/chat/rooms/:roomId/members
    pub async fn list_chat_room_members(&self, room_id: &str) -> Result<ChatRoomMembersData> {
        let path = format!("/api/v2/chat/rooms/{room_id}/members");
        let resp: ApiResponse<ChatRoomMembersData> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v2/chat/rooms/:roomId/members/self (204)
    pub async fn leave_chat_room(&self, room_id: &str) -> Result<()> {
        let path = format!("/api/v2/chat/rooms/{room_id}/members/self");
        self.delete_no_body(&path).await
    }

    /// DELETE /api/v2/chat/rooms/:roomId/members/:memberId (204)
    pub async fn remove_chat_room_member(&self, room_id: &str, member_id: &str) -> Result<()> {
        let path = format!("/api/v2/chat/rooms/{room_id}/members/{member_id}");
        self.delete_no_body(&path).await
    }

    /// PUT /api/v2/chat/rooms/:roomId/icon (raw image bytes as the request body)
    pub async fn upload_chat_room_icon(
        &self,
        room_id: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<ChatRoom> {
        let path = format!("/api/v2/chat/rooms/{room_id}/icon");
        let resp: ApiResponse<ChatRoom> = self.put_bytes(&path, bytes, content_type).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v2/chat/rooms/:roomId/icon (204)
    pub async fn delete_chat_room_icon(&self, room_id: &str) -> Result<()> {
        let path = format!("/api/v2/chat/rooms/{room_id}/icon");
        self.delete_no_body(&path).await
    }

    /// POST /api/v2/chat/rooms/:roomId/join (idempotent self-join for public groups)
    pub async fn join_public_chat_group(&self, room_id: &str) -> Result<ChatRoom> {
        let path = format!("/api/v2/chat/rooms/{room_id}/join");
        let resp: ApiResponse<ChatRoom> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/chat/rooms/:roomId/invitations
    pub async fn invite_chat_room_members(
        &self,
        room_id: &str,
        invited_member_ids: Vec<String>,
    ) -> Result<ChatRoom> {
        let path = format!("/api/v2/chat/rooms/{room_id}/invitations");
        let body = ChatInviteMembersRequest { invited_member_ids };
        let resp: ApiResponse<ChatRoom> = self.post(&path, &body).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/chat/invitations/pending
    pub async fn list_pending_chat_invitations(&self) -> Result<ChatPendingInvitationsData> {
        let resp: ApiResponse<ChatPendingInvitationsData> =
            self.get("/api/v2/chat/invitations/pending").await?;
        Ok(resp.data)
    }

    /// POST /api/v2/chat/invitations/:invitationId/accept
    pub async fn accept_chat_invitation(&self, invitation_id: &str) -> Result<ChatRoom> {
        let path = format!("/api/v2/chat/invitations/{invitation_id}/accept");
        let resp: ApiResponse<ChatRoom> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/chat/invitations/:invitationId/decline (204)
    pub async fn decline_chat_invitation(&self, invitation_id: &str) -> Result<()> {
        let path = format!("/api/v2/chat/invitations/{invitation_id}/decline");
        self.post_empty_no_content(&path).await
    }

    /// GET /api/v2/chat/rooms/:roomId/messages
    pub async fn list_chat_messages(
        &self,
        room_id: &str,
        params: ChatMessageListParams<'_>,
    ) -> Result<Vec<ChatMessage>> {
        let suffix = chat_message_list_query_suffix(&params);
        let path = format!("/api/v2/chat/rooms/{room_id}/messages{suffix}");
        let resp: ApiResponse<ChatMessagesData> = self.get(&path).await?;
        Ok(resp.data.messages)
    }

    /// PUT /api/v2/chat/rooms/:roomId/read (204). `message_id=None` marks the
    /// latest non-deleted message in the room as read.
    pub async fn mark_chat_room_read(&self, room_id: &str, message_id: Option<&str>) -> Result<()> {
        let path = format!("/api/v2/chat/rooms/{room_id}/read");
        let body = ChatMarkRoomReadRequest {
            message_id: message_id.map(str::to_string),
        };
        self.put_no_content(&path, &body).await
    }

    /// PATCH /api/v2/chat/rooms/:roomId/hidden (204)
    pub async fn set_chat_room_hidden(&self, room_id: &str, hidden: bool) -> Result<()> {
        let path = format!("/api/v2/chat/rooms/{room_id}/hidden");
        let body = ChatHideRoomRequest { hidden };
        self.patch_no_content(&path, &body).await
    }

    /// POST /api/v2/chat/rooms/:roomId/messages
    pub async fn post_chat_message(
        &self,
        room_id: &str,
        content: &str,
        parent_message_id: Option<&str>,
        mentions: Vec<String>,
    ) -> Result<ChatMessage> {
        let path = format!("/api/v2/chat/rooms/{room_id}/messages");
        let body = ChatMessageRequest {
            content: content.to_string(),
            parent_message_id: parent_message_id.map(str::to_string),
            mentioned_org_member_ids: mentions,
        };
        let resp: ApiResponse<ChatMessage> = self.post(&path, &body).await?;
        Ok(resp.data)
    }

    /// PATCH /api/v2/chat/rooms/:roomId/messages/:messageId
    pub async fn edit_chat_message(
        &self,
        room_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<ChatMessage> {
        let path = format!("/api/v2/chat/rooms/{room_id}/messages/{message_id}");
        let body = ChatMessageRequest {
            content: content.to_string(),
            parent_message_id: None,
            mentioned_org_member_ids: Vec::new(),
        };
        let resp: ApiResponse<ChatMessage> = self.patch(&path, &body).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v2/chat/rooms/:roomId/messages/:messageId (204)
    pub async fn delete_chat_message(&self, room_id: &str, message_id: &str) -> Result<()> {
        let path = format!("/api/v2/chat/rooms/{room_id}/messages/{message_id}");
        self.delete_no_body(&path).await
    }

    /// POST /api/v2/chat/rooms/:roomId/messages/:messageId/reactions
    pub async fn add_chat_reaction(
        &self,
        room_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<ChatMessage> {
        let path = format!("/api/v2/chat/rooms/{room_id}/messages/{message_id}/reactions");
        let body = ReactionRequest {
            emoji: emoji.to_string(),
        };
        let resp: ApiResponse<ChatMessage> = self.post(&path, &body).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v2/chat/rooms/:roomId/messages/:messageId/reactions/:emoji (204)
    pub async fn remove_chat_reaction(
        &self,
        room_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<()> {
        let emoji = encode_path_segment(emoji);
        let path = format!("/api/v2/chat/rooms/{room_id}/messages/{message_id}/reactions/{emoji}");
        self.delete_no_body(&path).await
    }

    /// GET /api/v2/chat/rooms/:roomId/messages/:messageId/reactions/:emoji/users
    pub async fn list_chat_reaction_users(
        &self,
        room_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<Vec<String>> {
        let emoji = encode_path_segment(emoji);
        let path =
            format!("/api/v2/chat/rooms/{room_id}/messages/{message_id}/reactions/{emoji}/users");
        let resp: ApiResponse<crate::api::IssueReactionUsers> = self.get(&path).await?;
        Ok(resp.data.member_ids)
    }

    /// POST /api/v2/chat/dms
    pub async fn create_chat_dm(&self, partner_id: &str) -> Result<ChatRoom> {
        let body = CreateDmRequest {
            partner_id: partner_id.to_string(),
        };
        let resp: ApiResponse<ChatRoom> = self.post("/api/v2/chat/dms", &body).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/chat/groups
    pub async fn create_chat_group(
        &self,
        name: &str,
        visibility: &str,
        member_ids: Vec<String>,
    ) -> Result<ChatRoom> {
        let body = CreateGroupRequest {
            name: name.to_string(),
            visibility: visibility.to_string(),
            member_ids,
        };
        let resp: ApiResponse<ChatRoom> = self.post("/api/v2/chat/groups", &body).await?;
        Ok(resp.data)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChatMessageListParams, ChatRoomListParams, ChatSearchParams,
        chat_message_list_query_suffix, chat_room_list_query_suffix,
    };

    #[test]
    fn chat_room_list_query_suffix_is_empty_without_params() {
        assert_eq!(
            chat_room_list_query_suffix(&ChatRoomListParams::default()),
            ""
        );
    }

    #[test]
    fn chat_room_list_query_suffix_encodes_all_params() {
        let suffix = chat_room_list_query_suffix(&ChatRoomListParams {
            room_type: Some("group"),
            limit: Some(20),
            before: Some("2026-07-15T00:00:00Z"),
            before_id: Some("room-1"),
            hidden: true,
        });
        assert_eq!(
            suffix,
            "?room_type=group&limit=20&before=2026-07-15T00%3A00%3A00Z&before_id=room-1&hidden=true"
        );
    }

    #[test]
    fn chat_message_list_query_suffix_encodes_all_params() {
        let suffix = chat_message_list_query_suffix(&ChatMessageListParams {
            parent_message_id: Some("m-1"),
            limit: Some(30),
            before: Some("2026-07-15T00:00:00Z"),
            before_id: Some("m-2"),
        });
        assert_eq!(
            suffix,
            "?parent_message_id=m-1&limit=30&before=2026-07-15T00%3A00%3A00Z&before_id=m-2"
        );
    }

    #[test]
    fn chat_search_params_default_has_no_filters() {
        let params = ChatSearchParams::default();
        assert!(params.limit.is_none());
        assert!(params.before.is_none());
        assert!(params.before_id.is_none());
    }
}
