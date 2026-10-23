use clap::ValueEnum;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Huddle（音声通話） — internal/huddle/handler/endpoints/*.go
//
// Real-time participation endpoints (join/leave/switch, LiveKit token
// re-issuance, heartbeat, screen-share acquire/release) are intentionally
// **not** modeled here: they only make sense while a client is actually
// connected to the LiveKit room, which the one-shot Addness CLI cannot do.
// The read/control endpoints below (status, recording toggle, invitations,
// member lookup) work fine as standalone CLI commands.
// ---------------------------------------------------------------------------

/// `GET /api/v2/objectives/:id/huddle` and
/// `GET /api/v2/objectives/:id/huddle/sessions/:sessionId` share this shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleStatus {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub recording: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_language: Option<String>,
    #[serde(default)]
    pub participants: Vec<crate::api::HuddleParticipant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
}

/// `POST /api/v2/objectives/:id/huddle/recording/start`
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HuddleRecordingStartRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_child_goals: Option<bool>,
}

/// Response is a free-form `gin.H` on the backend; model only the fields the
/// handler is documented to set.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HuddleRecordingStartResponse {
    #[serde(default)]
    pub recording: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_child_goals: Option<bool>,
}

/// `POST /api/v2/objectives/:id/huddle/recording/stop`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HuddleRecordingStopResponse {
    #[serde(default)]
    pub recording: bool,
}

/// Sort field accepted by `GET /api/v2/objectives/:id/huddle/inviteable-members`.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum HuddleMemberSortBy {
    Name,
    CreatedAt,
}

impl HuddleMemberSortBy {
    pub fn as_str(self) -> &'static str {
        match self {
            HuddleMemberSortBy::Name => "name",
            HuddleMemberSortBy::CreatedAt => "created_at",
        }
    }
}

/// Sort direction accepted by the same endpoint.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum HuddleSortDir {
    Asc,
    Desc,
}

impl HuddleSortDir {
    pub fn as_str(self) -> &'static str {
        match self {
            HuddleSortDir::Asc => "asc",
            HuddleSortDir::Desc => "desc",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleInviteableMember {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<HuddleMemberAvatar>,
    pub created_at: String,
}

/// Structured avatar metadata (as opposed to the flat `avatarUrl` convenience
/// field), confirmed against production data — includes per-size CDN variant
/// URLs from either Clerk or the Addness upload pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleMemberAvatar {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub variants: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleInviteableMembersMeta {
    #[serde(default)]
    pub more: bool,
    #[serde(default)]
    pub remaining_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleInviteableMembersResponse {
    #[serde(default)]
    pub members: Vec<HuddleInviteableMember>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
    pub meta: HuddleInviteableMembersMeta,
}

/// `POST /api/v2/huddle/sessions/:sessionId/invitations`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleInvitationSendRequest {
    pub organization_member_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HuddleInvitationStatus {
    Sent,
    Skipped,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleInvitationResult {
    pub organization_member_id: String,
    pub status: HuddleInvitationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleInvitationSendResponse {
    #[serde(default)]
    pub results: Vec<HuddleInvitationResult>,
}

// ---------------------------------------------------------------------------
// Meeting Bot（Recall.ai連携ジョブ） — internal/meetingbot/{handler,usecase}/*.go
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingBotJob {
    pub id: String,
    pub meeting_url: String,
    pub bot_name: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    /// Keep Drive destinations and recording/transcript status fields in JSON output.
    #[serde(flatten)]
    pub details: std::collections::BTreeMap<String, serde_json::Value>,
}

/// `POST /api/v1/team/meeting-bot/jobs`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingBotJobCreateRequest {
    pub meeting_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meeting_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drive_folder_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_video: Option<bool>,
}
