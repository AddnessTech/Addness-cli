use anyhow::{Result, bail};
use clap::{ArgAction, Subcommand, ValueEnum};
use colored::Colorize;

use crate::api::{
    ApiClient, ChatMessage, ChatMessageListParams, ChatRoom, ChatRoomListParams, ChatSearchParams,
};

use super::comment::read_body;
use super::org::content_type_for_path;

/// Maximum content length (in characters) accepted by the org-chat backend
/// (domain/chatmessage MaxContentRunes; shared with goal-issue).
const MAX_CHAT_CONTENT_CHARS: usize = 4000;

/// Maximum group name length (in characters), domain/orgchat MaxGroupNameRunes.
const MAX_GROUP_NAME_CHARS: usize = 255;

/// Room type filter accepted by `GET /api/v2/chat/rooms`.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ChatRoomType {
    Dm,
    Group,
}

impl ChatRoomType {
    fn as_str(self) -> &'static str {
        match self {
            ChatRoomType::Dm => "dm",
            ChatRoomType::Group => "group",
        }
    }
}

/// Visibility accepted by `POST /api/v2/chat/groups`.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ChatVisibility {
    /// Discoverable and freely joinable by any organization member
    Public,
    /// Membership by invitation only
    Private,
}

impl ChatVisibility {
    fn as_str(self) -> &'static str {
        match self {
            ChatVisibility::Public => "public",
            ChatVisibility::Private => "private",
        }
    }
}

#[derive(Subcommand)]
pub enum ChatCommands {
    /// Search messages (and matching room names) across your DM/group rooms
    Search {
        /// Search query
        #[arg(long)]
        query: String,
        /// Max number of messages to return
        #[arg(long)]
        limit: Option<u16>,
        /// Keyset cursor: return messages created before this RFC3339 timestamp
        #[arg(long)]
        before: Option<String>,
        /// Keyset cursor: message ID paired with --before
        #[arg(long)]
        before_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage chat rooms (DMs and groups)
    Room {
        #[command(subcommand)]
        command: RoomCommands,
    },
    /// Manage messages within a chat room
    Message {
        #[command(subcommand)]
        command: MessageCommands,
    },
    /// Manage pending group invitations addressed to you
    Invitation {
        #[command(subcommand)]
        command: ChatInvitationCommands,
    },
}

#[derive(Subcommand)]
pub enum RoomCommands {
    /// List rooms you belong to
    List {
        /// Filter by room type
        #[arg(long, value_enum)]
        room_type: Option<ChatRoomType>,
        /// Max number of rooms to return
        #[arg(long)]
        limit: Option<u16>,
        /// Keyset cursor: return rooms active before this RFC3339 timestamp
        #[arg(long)]
        before: Option<String>,
        /// Keyset cursor: room ID paired with --before
        #[arg(long)]
        before_id: Option<String>,
        /// Only return rooms you have hidden
        #[arg(long)]
        hidden: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List public groups in the organization (discovery; membership not required)
    ListPublic {
        /// Max number of rooms to return
        #[arg(long)]
        limit: Option<u16>,
        /// Keyset cursor: return rooms active before this RFC3339 timestamp
        #[arg(long)]
        before: Option<String>,
        /// Keyset cursor: room ID paired with --before
        #[arg(long)]
        before_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the total unread message count across your group rooms
    UnreadCount {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a room by ID
    Get {
        /// Room ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start (or resume) a direct message room with another organization member
    CreateDm {
        /// Partner's organization member ID
        #[arg(long)]
        partner: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a group room
    CreateGroup {
        /// Group name
        #[arg(long)]
        name: String,
        /// Visibility
        #[arg(long, value_enum)]
        visibility: ChatVisibility,
        /// Initial member organization member IDs, repeatable
        #[arg(long)]
        member: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Rename a group room
    Rename {
        /// Room ID
        id: String,
        /// New group name
        #[arg(long)]
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a group room
    Rm {
        /// Room ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// List members of a room
    Members {
        /// Room ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Leave a group room
    Leave {
        /// Room ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Remove a member from a group room
    RemoveMember {
        /// Room ID
        id: String,
        /// Organization member ID to remove
        member_id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Join a public group (self-service, idempotent)
    Join {
        /// Room ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Invite organization members to a group room
    Invite {
        /// Room ID
        id: String,
        /// Organization member ID to invite, repeatable
        #[arg(long)]
        member: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Set a group room's icon
    SetIcon {
        /// Room ID
        id: String,
        /// Path to the image file to upload
        #[arg(long)]
        file: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a group room's icon (reverts to the default display)
    RmIcon {
        /// Room ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Mark a room as read
    Read {
        /// Room ID
        id: String,
        /// Mark as read up to this message ID (defaults to the latest message)
        #[arg(long)]
        message_id: Option<String>,
    },
    /// Hide or unhide a room from your room list
    Hide {
        /// Room ID
        id: String,
        /// true to hide, false to unhide
        #[arg(long, action = ArgAction::Set)]
        hidden: bool,
    },
}

#[derive(Subcommand)]
pub enum MessageCommands {
    /// List messages in a room
    List {
        /// Room ID
        #[arg(long)]
        room: String,
        /// Only return replies to this message ID
        #[arg(long)]
        parent_message_id: Option<String>,
        /// Max number of messages to return
        #[arg(long)]
        limit: Option<u16>,
        /// Keyset cursor: return messages created before this RFC3339 timestamp
        #[arg(long)]
        before: Option<String>,
        /// Keyset cursor: message ID paired with --before
        #[arg(long)]
        before_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Post a message to a room
    Post {
        /// Room ID
        #[arg(long)]
        room: String,
        /// Message body
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read message body from a file. Use '-' to read stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Reply to this message ID
        #[arg(long)]
        parent_message_id: Option<String>,
        /// Mention organization member IDs (UUID), repeatable
        #[arg(long)]
        mention: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Edit a message you sent
    Update {
        /// Message ID
        id: String,
        /// Room ID
        #[arg(long)]
        room: String,
        /// New message body
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// New message body from a file. Use '-' to read stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a message you sent
    Rm {
        /// Message ID
        id: String,
        /// Room ID
        #[arg(long)]
        room: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Add an emoji reaction to a message
    React {
        /// Message ID
        id: String,
        /// Room ID
        #[arg(long)]
        room: String,
        /// Emoji (e.g. 👍)
        #[arg(long)]
        emoji: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove your emoji reaction from a message
    Unreact {
        /// Message ID
        id: String,
        /// Room ID
        #[arg(long)]
        room: String,
        /// Emoji to remove
        #[arg(long)]
        emoji: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// List member IDs who reacted to a message with an emoji
    Reactions {
        /// Message ID
        id: String,
        /// Room ID
        #[arg(long)]
        room: String,
        /// Emoji to look up
        #[arg(long)]
        emoji: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ChatInvitationCommands {
    /// List pending group invitations addressed to you
    ListPending {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Accept a pending group invitation
    Accept {
        /// Invitation ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Decline a pending group invitation
    Decline {
        /// Invitation ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

/// Validate message content (non-empty, within the backend limit).
fn ensure_chat_content(content: String) -> Result<String> {
    if content.trim().is_empty() {
        bail!("Message body is empty. Specify --body, --body-file, or pipe content with --body -.");
    }
    if content.chars().count() > MAX_CHAT_CONTENT_CHARS {
        bail!("Message body must be {MAX_CHAT_CONTENT_CHARS} characters or less.");
    }
    Ok(content)
}

/// Validate a group name (non-empty, within the backend limit).
fn ensure_group_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("Group name is empty. Specify --name.");
    }
    if name.chars().count() > MAX_GROUP_NAME_CHARS {
        bail!("Group name must be {MAX_GROUP_NAME_CHARS} characters or less.");
    }
    Ok(())
}

fn truncate_content(content: &str, max_chars: usize) -> String {
    let flattened = content.replace('\n', " ");
    if flattened.chars().count() > max_chars {
        let truncated: String = flattened
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect();
        format!("{truncated}...")
    } else {
        flattened
    }
}

fn room_label(room: &ChatRoom) -> String {
    match &room.name {
        Some(name) => name.clone(),
        None if room.room_type == "dm" => "(dm)".to_string(),
        None => "(unnamed)".to_string(),
    }
}

fn print_rooms_table(rooms: &[ChatRoom]) {
    if rooms.is_empty() {
        println!("{}", "No rooms found.".dimmed());
        return;
    }

    println!(
        "{:<38} {:<7} {:<9} {:>7} {}",
        "ID".bold(),
        "TYPE".bold(),
        "VISIBLE".bold(),
        "UNREAD".bold(),
        "NAME".bold()
    );
    println!("{}", "─".repeat(90));

    for room in rooms {
        println!(
            "{:<38} {:<7} {:<9} {:>7} {}",
            room.id.dimmed(),
            room.room_type,
            room.visibility,
            room.unread_count,
            room_label(room)
        );
    }
}

fn print_messages_table(messages: &[ChatMessage]) {
    if messages.is_empty() {
        println!("{}", "No messages found.".dimmed());
        return;
    }

    println!(
        "{:<38} {:<38} {:<12} {}",
        "MESSAGE ID".bold(),
        "SENDER".bold(),
        "DATE".bold(),
        "CONTENT".bold()
    );
    println!("{}", "─".repeat(120));
    for message in messages {
        let date = &message.created_at[..10.min(message.created_at.len())];
        let sender = message.sender_id.as_deref().unwrap_or("-");
        println!(
            "{:<38} {:<38} {:<12} {}",
            message.id.dimmed(),
            sender,
            date.dimmed(),
            truncate_content(&message.content, 50)
        );
    }
}

pub async fn handle_chat(cmd: &ChatCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ChatCommands::Search {
            query,
            limit,
            before,
            before_id,
            json,
        } => {
            let data = client
                .search_chat_messages(
                    query,
                    ChatSearchParams {
                        limit: *limit,
                        before: before.as_deref(),
                        before_id: before_id.as_deref(),
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                if !data.rooms.is_empty() {
                    println!("{}", "Matching rooms:".bold());
                    for room in &data.rooms {
                        println!("{} — {}", room.id.dimmed(), room_label(room));
                    }
                    println!();
                }
                print_messages_table(&data.messages);
            }
            Ok(())
        }
        ChatCommands::Room { command } => handle_room(command, client).await,
        ChatCommands::Message { command } => handle_message(command, client).await,
        ChatCommands::Invitation { command } => handle_invitation(command, client).await,
    }
}

async fn handle_room(cmd: &RoomCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        RoomCommands::List {
            room_type,
            limit,
            before,
            before_id,
            hidden,
            json,
        } => {
            let rooms = client
                .list_chat_rooms(ChatRoomListParams {
                    room_type: room_type.map(ChatRoomType::as_str),
                    limit: *limit,
                    before: before.as_deref(),
                    before_id: before_id.as_deref(),
                    hidden: *hidden,
                })
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&rooms)?);
            } else {
                print_rooms_table(&rooms);
            }
            Ok(())
        }
        RoomCommands::ListPublic {
            limit,
            before,
            before_id,
            json,
        } => {
            let rooms = client
                .list_public_chat_groups(ChatRoomListParams {
                    room_type: None,
                    limit: *limit,
                    before: before.as_deref(),
                    before_id: before_id.as_deref(),
                    hidden: false,
                })
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&rooms)?);
            } else {
                print_rooms_table(&rooms);
            }
            Ok(())
        }
        RoomCommands::UnreadCount { json } => {
            let count = client.count_chat_group_unread().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&count)?);
            } else {
                println!("Unread group messages: {}", count.unread_count);
            }
            Ok(())
        }
        RoomCommands::Get { id, json } => {
            let room = client.get_chat_room(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&room)?);
            } else {
                println!("{} ({})", room_label(&room), room.id);
                println!("Type: {}  Visibility: {}", room.room_type, room.visibility);
            }
            Ok(())
        }
        RoomCommands::CreateDm { partner, json } => {
            let room = client.create_chat_dm(partner).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&room)?);
            } else {
                println!("DM room created: {}", room.id);
            }
            Ok(())
        }
        RoomCommands::CreateGroup {
            name,
            visibility,
            member,
            json,
        } => {
            ensure_group_name(name)?;
            let room = client
                .create_chat_group(name, visibility.as_str(), member.clone())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&room)?);
            } else {
                println!("Group created: {} ({})", room_label(&room), room.id);
            }
            Ok(())
        }
        RoomCommands::Rename { id, name, json } => {
            ensure_group_name(name)?;
            let room = client.rename_chat_room(id, name).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&room)?);
            } else {
                println!("Room {id} renamed to {name}");
            }
            Ok(())
        }
        RoomCommands::Rm { id, force } => {
            if !*force && !crate::cli::commands::confirm(&format!("Delete group room {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_chat_group(id).await?;
            println!("Room {id} deleted");
            Ok(())
        }
        RoomCommands::Members { id, json } => {
            let data = client.list_chat_room_members(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&data.members)?);
            } else if data.members.is_empty() {
                println!("{}", "No members found.".dimmed());
            } else {
                println!(
                    "{:<38} {:<10} {}",
                    "ORG MEMBER ID".bold(),
                    "ROLE".bold(),
                    "JOINED".bold()
                );
                println!("{}", "─".repeat(70));
                for member in &data.members {
                    let date = &member.joined_at[..10.min(member.joined_at.len())];
                    println!(
                        "{:<38} {:<10} {}",
                        member.organization_member_id.dimmed(),
                        member.role,
                        date
                    );
                }
            }
            Ok(())
        }
        RoomCommands::Leave { id, force } => {
            if !*force && !crate::cli::commands::confirm(&format!("Leave room {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            client.leave_chat_room(id).await?;
            println!("Left room {id}");
            Ok(())
        }
        RoomCommands::RemoveMember {
            id,
            member_id,
            force,
        } => {
            if !*force
                && !crate::cli::commands::confirm(&format!(
                    "Remove member {member_id} from room {id}?"
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            client.remove_chat_room_member(id, member_id).await?;
            println!("Removed member {member_id} from room {id}");
            Ok(())
        }
        RoomCommands::Join { id, json } => {
            let room = client.join_public_chat_group(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&room)?);
            } else {
                println!("Joined room {} ({})", room_label(&room), room.id);
            }
            Ok(())
        }
        RoomCommands::Invite { id, member, json } => {
            if member.is_empty() {
                bail!("Specify at least one --member to invite.");
            }
            let room = client.invite_chat_room_members(id, member.clone()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&room)?);
            } else {
                println!("Invited {} member(s) to room {id}", member.len());
            }
            Ok(())
        }
        RoomCommands::SetIcon { id, file, json } => {
            let bytes = std::fs::read(file)
                .map_err(|e| anyhow::anyhow!("Failed to read icon file '{file}': {e}"))?;
            let content_type = content_type_for_path(file);
            let room = client
                .upload_chat_room_icon(id, bytes, content_type)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&room)?);
            } else {
                println!("Room {id} icon updated");
            }
            Ok(())
        }
        RoomCommands::RmIcon { id, force } => {
            if !*force && !crate::cli::commands::confirm(&format!("Remove room {id}'s icon?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_chat_room_icon(id).await?;
            println!("Room {id} icon removed");
            Ok(())
        }
        RoomCommands::Read { id, message_id } => {
            client
                .mark_chat_room_read(id, message_id.as_deref())
                .await?;
            println!("Room {id} marked as read");
            Ok(())
        }
        RoomCommands::Hide { id, hidden } => {
            client.set_chat_room_hidden(id, *hidden).await?;
            if *hidden {
                println!("Room {id} hidden");
            } else {
                println!("Room {id} unhidden");
            }
            Ok(())
        }
    }
}

async fn handle_message(cmd: &MessageCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        MessageCommands::List {
            room,
            parent_message_id,
            limit,
            before,
            before_id,
            json,
        } => {
            let messages = client
                .list_chat_messages(
                    room,
                    ChatMessageListParams {
                        parent_message_id: parent_message_id.as_deref(),
                        limit: *limit,
                        before: before.as_deref(),
                        before_id: before_id.as_deref(),
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&messages)?);
            } else {
                print_messages_table(&messages);
            }
            Ok(())
        }
        MessageCommands::Post {
            room,
            body,
            body_file,
            parent_message_id,
            mention,
            json,
        } => {
            let content = ensure_chat_content(read_body(body.as_ref(), body_file.as_ref())?)?;
            let message = client
                .post_chat_message(
                    room,
                    &content,
                    parent_message_id.as_deref(),
                    mention.clone(),
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&message)?);
            } else {
                println!("Message posted: {}", message.id);
            }
            Ok(())
        }
        MessageCommands::Update {
            id,
            room,
            body,
            body_file,
            json,
        } => {
            let content = ensure_chat_content(read_body(body.as_ref(), body_file.as_ref())?)?;
            let message = client.edit_chat_message(room, id, &content).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&message)?);
            } else {
                println!("Message updated: {}", message.id);
            }
            Ok(())
        }
        MessageCommands::Rm { id, room, force } => {
            if !*force && !crate::cli::commands::confirm(&format!("Delete message {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_chat_message(room, id).await?;
            println!("Message {id} deleted");
            Ok(())
        }
        MessageCommands::React {
            id,
            room,
            emoji,
            json,
        } => {
            let message = client.add_chat_reaction(room, id, emoji).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&message)?);
            } else {
                println!("Reacted {emoji} on message {id}");
            }
            Ok(())
        }
        MessageCommands::Unreact {
            id,
            room,
            emoji,
            force,
        } => {
            if !*force
                && !crate::cli::commands::confirm(&format!(
                    "Remove your {emoji} reaction from message {id}?"
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            client.remove_chat_reaction(room, id, emoji).await?;
            println!("Removed {emoji} reaction from message {id}");
            Ok(())
        }
        MessageCommands::Reactions {
            id,
            room,
            emoji,
            json,
        } => {
            let member_ids = client.list_chat_reaction_users(room, id, emoji).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&member_ids)?);
            } else if member_ids.is_empty() {
                println!("No reactions with {emoji} on message {id}.");
            } else {
                for member_id in &member_ids {
                    println!("{member_id}");
                }
            }
            Ok(())
        }
    }
}

async fn handle_invitation(cmd: &ChatInvitationCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ChatInvitationCommands::ListPending { json } => {
            let data = client.list_pending_chat_invitations().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&data.invitations)?);
            } else if data.invitations.is_empty() {
                println!("{}", "No pending invitations.".dimmed());
            } else {
                println!(
                    "{:<38} {:<24} {}",
                    "INVITATION ID".bold(),
                    "ROOM".bold(),
                    "INVITED BY".bold()
                );
                println!("{}", "─".repeat(90));
                for invitation in &data.invitations {
                    println!(
                        "{:<38} {:<24} {}",
                        invitation.id.dimmed(),
                        invitation.room_name,
                        invitation.inviter_name
                    );
                }
            }
            Ok(())
        }
        ChatInvitationCommands::Accept { id, json } => {
            let room = client.accept_chat_invitation(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&room)?);
            } else {
                println!("Joined room {} ({})", room_label(&room), room.id);
            }
            Ok(())
        }
        ChatInvitationCommands::Decline { id, force } => {
            if !*force && !crate::cli::commands::confirm(&format!("Decline invitation {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            client.decline_chat_invitation(id).await?;
            println!("Invitation {id} declined");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChatRoomType, ChatVisibility, ensure_chat_content, ensure_group_name, room_label,
        truncate_content,
    };
    use crate::api::ChatRoom;

    fn room(room_type: &str, name: Option<&str>) -> ChatRoom {
        ChatRoom {
            id: "r-1".to_string(),
            organization_id: String::new(),
            room_type: room_type.to_string(),
            visibility: "public".to_string(),
            is_member: true,
            created_at: String::new(),
            updated_at: String::new(),
            unread_count: 0,
            unread_mention_count: 0,
            name: name.map(str::to_string),
            dm_pair_key: None,
            icon_url: None,
            created_by: None,
            last_message_at: None,
            last_message: None,
        }
    }

    #[test]
    fn chat_room_type_as_str_maps_to_backend_values() {
        assert_eq!(ChatRoomType::Dm.as_str(), "dm");
        assert_eq!(ChatRoomType::Group.as_str(), "group");
    }

    #[test]
    fn chat_visibility_as_str_maps_to_backend_values() {
        assert_eq!(ChatVisibility::Public.as_str(), "public");
        assert_eq!(ChatVisibility::Private.as_str(), "private");
    }

    #[test]
    fn ensure_chat_content_rejects_empty_content() {
        assert!(ensure_chat_content("   \n".to_string()).is_err());
    }

    #[test]
    fn ensure_chat_content_accepts_content_at_limit() {
        let body = "あ".repeat(4000);
        assert_eq!(ensure_chat_content(body.clone()).unwrap(), body);
    }

    #[test]
    fn ensure_chat_content_rejects_content_over_limit() {
        let err = ensure_chat_content("あ".repeat(4001)).unwrap_err();
        assert!(err.to_string().contains("4000"));
    }

    #[test]
    fn ensure_group_name_rejects_empty_name() {
        assert!(ensure_group_name("  ").is_err());
    }

    #[test]
    fn ensure_group_name_rejects_name_over_limit() {
        let err = ensure_group_name(&"a".repeat(256)).unwrap_err();
        assert!(err.to_string().contains("255"));
    }

    #[test]
    fn ensure_group_name_accepts_name_at_limit() {
        assert!(ensure_group_name(&"a".repeat(255)).is_ok());
    }

    #[test]
    fn room_label_prefers_name_then_dm_then_unnamed() {
        assert_eq!(room_label(&room("group", Some("general"))), "general");
        assert_eq!(room_label(&room("dm", None)), "(dm)");
        assert_eq!(room_label(&room("group", None)), "(unnamed)");
    }

    #[test]
    fn truncate_content_flattens_newlines_and_truncates() {
        assert_eq!(truncate_content("a\nb", 10), "a b");
        let truncated = truncate_content(&"x".repeat(100), 10);
        assert_eq!(truncated.chars().count(), 10);
        assert!(truncated.ends_with("..."));
    }
}
