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

/// `GET /api/v2/huddle/active` (floating-bar state for the current user;
/// 204 when not currently in a huddle).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleActive {
    pub session_id: String,
    pub objective_id: String,
    pub objective_title: String,
    #[serde(default)]
    pub participants: Vec<crate::api::HuddleParticipant>,
    pub joined_at: String,
    pub started_at: String,
}

/// `GET /api/v2/huddle/transcription-progress`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleTranscriptionProgressItem {
    pub id: String,
    pub recording_attempt_id: String,
    pub session_id: String,
    pub objective_id: String,
    pub organization_id: String,
    pub recipient_org_member_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_url: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HuddleTranscriptionProgressResponse {
    #[serde(default)]
    pub items: Vec<HuddleTranscriptionProgressItem>,
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
    pub platform: String,
    pub meeting_url: String,
    pub bot_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_join_message: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joined_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// `POST /api/v1/team/meeting-bot/jobs`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingBotJobCreateRequest {
    pub platform: String,
    pub meeting_url: String,
    pub bot_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_join_message: Option<String>,
}

// ---------------------------------------------------------------------------
// Meeting Note（文字起こし/要約/議事録投稿/ゴール提案・作成）
// — presentation/handlers/meeting_note/{transcribe,summarize,post_minutes,
//   suggest_goals,create_goals}.go
// ---------------------------------------------------------------------------

/// `POST /api/v2/meeting-notes/transcribe`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteTranscribeResponse {
    pub transcript: String,
}

/// `POST /api/v2/meeting-notes/summarize`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteSummarizeRequest {
    pub transcript: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteSummarizeResponse {
    pub minutes: String,
}

/// `postType` accepted by `POST /api/v2/meeting-notes/post-minutes`.
#[derive(Clone, Copy, Debug, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingNotePostType {
    Comment,
    Deliverable,
    Both,
}

/// `POST /api/v2/meeting-notes/post-minutes`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNotePostMinutesRequest {
    pub objective_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minute_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minutes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    pub post_type: MeetingNotePostType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNotePostMinutesResponse {
    pub success: bool,
    pub comment: bool,
    pub deliverable: bool,
}

/// `POST /api/v2/meeting-notes/suggest-goals`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteSuggestGoalsRequest {
    pub objective_id: String,
    pub minutes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteGoalSuggestion {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteSuggestGoalsResponse {
    #[serde(default)]
    pub suggestions: Vec<MeetingNoteGoalSuggestion>,
}

/// `POST /api/v2/meeting-notes/create-goals`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteCreateGoalsRequestItem {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteCreateGoalsRequest {
    pub objective_id: String,
    pub goals: Vec<MeetingNoteCreateGoalsRequestItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteCreatedGoal {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNoteCreateGoalsResponse {
    #[serde(default)]
    pub goals: Vec<MeetingNoteCreatedGoal>,
    pub requested_count: i64,
    pub created_count: i64,
}

// ---------------------------------------------------------------------------
// 議事録（Minutes）CRUD — presentation/handlers/meeting_note/*_minute.go
// ---------------------------------------------------------------------------

/// `sourceType` accepted when *creating* a minute (`recording` = a huddle
/// recording, `zoom` = a linked Zoom recording job). Only these two are
/// documented as valid input for `POST /api/v2/minutes` /
/// `GET /api/v2/minutes?sourceType=`.
#[derive(Clone, Copy, Debug, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MinuteSourceType {
    Recording,
    Zoom,
}

/// `sourceType` as returned on `Minute`/`MinuteListItem`. This is a superset
/// of the creatable `MinuteSourceType`: the backend also stamps `"bot"` on
/// minutes generated by a Meeting Bot (Recall.ai) job, confirmed against
/// production data, and may add further values later — the catch-all keeps
/// deserialization forward-compatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MinuteSourceKind {
    Recording,
    Zoom,
    Bot,
    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinuteObjectiveSummary {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Minute {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom_job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meeting_bot_job_id: Option<String>,
    pub source_type: MinuteSourceKind,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub transcript: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<MinuteObjectiveSummary>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinuteListItem {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom_job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meeting_bot_job_id: Option<String>,
    pub source_type: MinuteSourceKind,
    pub title: String,
    pub summary_preview: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MinuteListResponse {
    #[serde(default)]
    pub minutes: Vec<MinuteListItem>,
}

/// `POST /api/v2/minutes`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinuteCreateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom_job_id: Option<String>,
    pub source_type: MinuteSourceType,
    pub title: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
}

/// `PATCH /api/v2/minutes/:id` (partial update; all fields optional).
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MinuteUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<String>,
}
