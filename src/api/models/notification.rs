use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// GET /api/v2/organizations/:id/notifications
// レスポンスは { "data": ... } でラップされない（notificationパッケージのBaseHandlerが素のJSONを返すため）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationListResponse {
    #[serde(default)]
    pub notifications: Vec<NotificationBundle>,
    #[serde(default)]
    pub unread_by_category: HashMap<String, i64>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub recipient_name: String,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// 個別通知、または関連通知をまとめたバンドルの1項目。
/// バンドル項目には `id` が無く、代わりに `notification_ids` に実際のIDが並ぶ
/// （mark-read/mark-unreadにはこちらを使う）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationBundle {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub read_at: Option<String>,
    pub event_type: String,
    #[serde(default)]
    pub relation: Option<String>,
    #[serde(default)]
    pub subject_type: Option<String>,
    #[serde(default, rename = "subjectID")]
    pub subject_id: Option<String>,
    #[serde(default)]
    pub subject_title: Option<String>,
    #[serde(default)]
    pub actors: Vec<serde_json::Value>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub occurred_at: Option<String>,
    #[serde(default)]
    pub unread_since: Option<String>,
    #[serde(default)]
    pub is_resolved: Option<bool>,
    #[serde(default)]
    pub notification_ids: Vec<String>,
}

// GET /api/v2/organizations/:id/notifications/count
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationCountResponse {
    #[serde(default)]
    pub unread_count: i64,
    #[serde(default)]
    pub unread_by_category: HashMap<String, i64>,
    #[serde(default)]
    pub latest_notification_at: Option<String>,
    #[serde(default)]
    pub latest_notification_at_in_scope: Option<String>,
}

// GET /api/v2/organizations/:id/notifications/counts-by-objective
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectiveNotificationCounts {
    #[serde(default)]
    pub comments: i64,
    #[serde(default)]
    pub deliverables: i64,
    #[serde(default)]
    pub assignments: i64,
    #[serde(default)]
    pub child_activity: i64,
    #[serde(default)]
    pub total: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CountsByObjectiveResponse {
    #[serde(default)]
    pub counts: HashMap<String, ObjectiveNotificationCounts>,
}

// POST .../notifications/mark-read, .../mark-unread
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationIdsRequest {
    pub notification_ids: Vec<String>,
}

// POST .../notifications/mark-read, .../mark-unread, .../mark-all-read
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkNotificationsResponse {
    #[serde(default)]
    pub unread_count: i64,
    #[serde(default)]
    pub marked_count: i64,
}

// GET/POST/PATCH /api/v1/team/notification_settings
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSetting {
    pub id: String,
    #[serde(default)]
    pub organization_member_id: Option<String>,
    pub provider: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

// POST /api/v1/team/notification_settings, PATCH /api/v1/team/notification_settings/:id
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettingRequest {
    pub provider: String,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

// GET /api/v1/team/email_destinations
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailDestination {
    pub id: String,
    #[serde(default)]
    pub organization_member_id: Option<String>,
    pub email: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}
