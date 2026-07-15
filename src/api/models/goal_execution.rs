use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{GoalStatus, Owner};

/// Response for the todays-goals endpoint
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodaysGoalsData {
    pub nodes: Vec<TodaysGoalNode>,
    pub auto_generated_count: i32,
    pub collapsed_goal_ids: Vec<String>,
    /// Inline count summary. Present on both `todays-goals` and
    /// `todays-goals/summary` (same backend response shape;
    /// `TodaysGoalsResponse` in `internal/goalexecution/usecase/dto.go`).
    #[serde(default)]
    pub summary: TodaysGoalsInlineSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TodaysGoalsInlineSummary {
    pub total_count: i32,
    pub incomplete_count: i32,
    pub is_goal_uncreated: bool,
}

/// Response for the standalone `GET /organizations/:id/todays-goals/summary`
/// endpoint. Despite the similar name this is a distinct, lighter-weight
/// NavBadge-style response with no `nodes` and snake_case keys — it is
/// **not** the same shape as `TodaysGoalsData.summary`
/// (`GetTodaysGoalsSummaryUseCase.Execute` returns
/// `*TodaysGoalsSummaryResponse` in
/// `internal/goalexecution/usecase/get_todays_goals_summary.go`, independent
/// of the full `TodaysGoalsResponse` used by `todays-goals`/`codex/todays-goals/view`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodaysGoalsSummaryResponse {
    pub total_count: i32,
    pub incomplete_count: i32,
    pub is_goal_uncreated: bool,
}

/// A single goal node in the today's goals response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodaysGoalNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: i32,
    pub title: String,
    pub status: String,
    pub completed_at: Option<String>,
    pub order_no: f64,
    pub is_leaf: bool,
    pub has_recurring: bool,
    pub is_recurring: bool,
    pub kind: String,
    pub execution: Option<ExecutionRecord>,
    pub owner: Option<Owner>,
    pub is_direct_assignment: bool,
    #[serde(rename = "isAIRunning")]
    pub is_ai_running: Option<bool>,
}

impl TodaysGoalNode {
    /// Parse status string to GoalStatus enum
    pub fn parsed_status(&self) -> Option<GoalStatus> {
        match self.status.as_str() {
            "NONE" => Some(GoalStatus::None),
            "IN_PROGRESS" => Some(GoalStatus::InProgress),
            "CANCELLED" => Some(GoalStatus::Cancelled),
            "" => Some(GoalStatus::None),
            _ => None,
        }
    }

    /// Check if this goal is completed (has completed_at timestamp)
    pub fn is_completed(&self) -> bool {
        self.completed_at.is_some()
    }
}

/// Execution record attached to a goal
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecord {
    pub id: String,
    pub objective_id: String,
    pub date: String, // YYYY-MM-DD
    pub status: GoalStatus,
    pub completed_at: Option<String>,
    pub created_at: Option<String>,
    pub due_at: Option<String>,
}

impl ExecutionRecord {
    /// Check if this execution is completed
    #[allow(dead_code)]
    pub fn is_completed(&self) -> bool {
        self.completed_at.is_some()
    }
}

// ---------------------------------------------------------------------------
// 「今日のToDo」(today_todos) — internal/goalexecution/usecase/today_todos.go
// ---------------------------------------------------------------------------

/// A single "today's todo" row. Objective-based rows only carry `objectiveId`
/// and `createdAt`; addness.chat-origin rows (no objective) additionally
/// carry their own title/detail/status fields (see `TodayTodoView` in the
/// backend).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TodayTodoView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_of_done: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_metadata: Option<serde_json::Value>,
}

/// Request body for `POST /organizations/:id/today-todos`. Sending
/// `objectiveId` routes to the idempotent objective-based add; omitting it
/// creates an addness.chat-origin row (title required).
#[derive(Debug, Clone, Serialize, Default)]
pub struct CreateTodayTodoRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(rename = "objectiveId", skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(rename = "executionDate", skip_serializing_if = "Option::is_none")]
    pub execution_date: Option<String>,
    #[serde(rename = "definitionOfDone", skip_serializing_if = "Option::is_none")]
    pub definition_of_done: Option<String>,
    #[serde(rename = "currentStatus", skip_serializing_if = "Option::is_none")]
    pub current_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(rename = "sortOrder", skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i32>,
    #[serde(rename = "chatMetadata", skip_serializing_if = "Option::is_none")]
    pub chat_metadata: Option<serde_json::Value>,
}

/// Request body for `PATCH /organizations/:id/today-todos/:todoId`
/// (addness.chat-origin rows only). Every field is optional and only
/// updates the row when present.
#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdateChatTodayTodoRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(rename = "executionDate", skip_serializing_if = "Option::is_none")]
    pub execution_date: Option<String>,
    #[serde(rename = "definitionOfDone", skip_serializing_if = "Option::is_none")]
    pub definition_of_done: Option<String>,
    #[serde(rename = "currentStatus", skip_serializing_if = "Option::is_none")]
    pub current_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(rename = "sortOrder", skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i32>,
    #[serde(rename = "chatMetadata", skip_serializing_if = "Option::is_none")]
    pub chat_metadata: Option<serde_json::Value>,
}

/// Request body for `POST /organizations/:id/today-todos/:todoId/activities`.
/// `action` must be one of `ENGAGE`, `PROGRESS`, `COMPLETE`
/// (`domain/goalexecution` `TodayTodoAction*` constants).
#[derive(Debug, Clone, Serialize)]
pub struct RecordTodayTodoActivityRequest {
    pub action: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: String,
    #[serde(rename = "currentStatus", skip_serializing_if = "Option::is_none")]
    pub current_status: Option<String>,
}

/// Response for `POST /organizations/:id/today-todos/:todoId/activities`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayTodoActivityView {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub activity_date: String,
    pub occurred_at: String,
    pub first_activity_of_day: bool,
    #[serde(default)]
    pub previous_current_state: String,
    #[serde(default)]
    pub current_state: String,
    pub todo: TodayTodoView,
}

// ---------------------------------------------------------------------------
// 「今日のToDoを決める材料プール」(planned_todos) —
// internal/goalexecution/usecase/planned_todos.go
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlannedTodoView {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub definition_of_done: String,
    #[serde(default)]
    pub current_status: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub scheduled_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<serde_json::Value>,
    #[serde(default)]
    pub recurrence_text: String,
    pub sort_order: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_metadata: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

/// The "material pool" split into the three buckets the execution tab uses to
/// decide what to commit to today's todos.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedTodoMaterial {
    pub date: String,
    pub due_or_overdue: Vec<PlannedTodoView>,
    pub recurring_today: Vec<PlannedTodoView>,
    pub backlog: Vec<PlannedTodoView>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CreatePlannedTodoRequest {
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(rename = "definitionOfDone", default)]
    pub definition_of_done: String,
    #[serde(rename = "currentStatus", default)]
    pub current_status: String,
    #[serde(default)]
    pub status: String,
    #[serde(rename = "scheduledDate", skip_serializing_if = "Option::is_none")]
    pub scheduled_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<serde_json::Value>,
    #[serde(rename = "sortOrder", default)]
    pub sort_order: i32,
    #[serde(rename = "chatMetadata", skip_serializing_if = "Option::is_none")]
    pub chat_metadata: Option<serde_json::Value>,
}

/// Request body for `PATCH /organizations/:id/planned-todos/:plannedId`.
/// `scheduled_date: Some("")` clears the scheduled date;
/// `clear_recurrence: true` clears the recurrence rule (sent as JSON `null`,
/// which the backend distinguishes from "field absent").
#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdatePlannedTodoRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(rename = "definitionOfDone", skip_serializing_if = "Option::is_none")]
    pub definition_of_done: Option<String>,
    #[serde(rename = "currentStatus", skip_serializing_if = "Option::is_none")]
    pub current_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(rename = "scheduledDate", skip_serializing_if = "Option::is_none")]
    pub scheduled_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<serde_json::Value>,
    #[serde(rename = "sortOrder", skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i32>,
    #[serde(rename = "chatMetadata", skip_serializing_if = "Option::is_none")]
    pub chat_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePlannedTodoResponse {
    pub deleted: bool,
}

// ---------------------------------------------------------------------------
// カレンダー — internal/goalexecution/usecase/calendar_ics.go,
// get_goal_calendar.go, get_goal_history.go
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    pub all_day: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CalendarEventCompletionRequest {
    pub date: String,
    #[serde(rename = "eventId")]
    pub event_id: String,
    #[serde(rename = "eventTitle", default)]
    pub event_title: String,
    #[serde(rename = "calendarName", default)]
    pub calendar_name: String,
    #[serde(rename = "eventStart")]
    pub event_start: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEventCompletionResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

/// One day's state in the goal-calendar heatmap.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoalCalendarDayData {
    pub goal_exists: bool,
    pub completed_goal_exists: bool,
    #[serde(default)]
    pub frozen: bool,
    #[serde(default)]
    pub completed_count: i32,
}

/// `year -> month -> day -> GoalCalendarDayData` nested map, as returned by
/// `GET /organizations/:id/goal-calendar`.
pub type GoalCalendarResponse =
    HashMap<String, HashMap<String, HashMap<String, GoalCalendarDayData>>>;

/// The handler wraps `GoalCalendarResponse` in an extra `{"data": ...}`
/// envelope on top of the standard `ApiResponse` envelope
/// (`h.JSON(c, http.StatusOK, gin.H{"data": res})` in
/// `internal/goalexecution/handler/endpoints/goal_calendar.go`), so callers
/// deserialize `ApiResponse<GoalCalendarEnvelope>` and unwrap twice.
#[derive(Debug, Clone, Deserialize)]
pub struct GoalCalendarEnvelope {
    pub data: GoalCalendarResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalHistoryResponse {
    pub nodes: Vec<TodaysGoalNode>,
}

// ---------------------------------------------------------------------------
// ゴール開閉プリファレンス — internal/goalexecution/usecase/get_goal_preference.go
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoalPreferenceResponse {
    #[serde(default)]
    pub collapsed_goal_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateGoalPreferenceRequest {
    #[serde(rename = "collapsedGoalIds")]
    pub collapsed_goal_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// 実行記録 — generate / update / history / member summary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateExecutionResponse {
    pub created: i32,
    #[serde(default)]
    pub records: Vec<ExecutionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginationInfo {
    pub total: i64,
    pub limit: i32,
    pub offset: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionStatsResponse {
    pub total_days: i32,
    pub completed_days: i32,
    pub completion_rate: f64,
    pub current_streak: i32,
    pub longest_streak: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionHistoryResponse {
    #[serde(default)]
    pub records: Vec<ExecutionRecord>,
    pub pagination: PaginationInfo,
    #[serde(default)]
    pub stats: Option<ExecutionStatsResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberExecutionSummary {
    pub member_id: String,
    pub name: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    pub completed_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummaryResponse {
    #[serde(default)]
    pub members: Vec<MemberExecutionSummary>,
    pub total_count: i32,
    pub page: i32,
    pub page_size: i32,
    pub total_pages: i32,
}

// ---------------------------------------------------------------------------
// Codex 用「今日のゴール」read/apply —
// internal/goalexecution/usecase/codex_todays_goals_{view,apply_ids}.go
//
// The view/apply payloads use a bespoke short-id + change-op DSL (assign,
// unassign, transfer-owner, complete, reorder, ...) that mirrors the Codex
// agent's internal representation rather than the CLI's own goal model.
// Modeling it 1:1 would duplicate a large slice of backend-only logic for a
// feature explicitly designed for machine (not human CLI) consumption, so
// the CLI passes the view response and the apply request/response through
// as opaque JSON, matching this codebase's existing convention for opaque
// payloads (e.g. `chatMetadata`, `search.rs`, `sharetree.rs`).
#[derive(Debug, Clone, Serialize, Default)]
pub struct CodexTodaysGoalsApplyRequest {
    pub version: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub changes: serde_json::Value,
}

// ---------------------------------------------------------------------------
// アクティブハドル — internal/huddle/usecase/{response,get_active_huddles_types}.go
// (`GET /api/v2/todays-goals/active-huddles`; the only huddle endpoint in the
// 実行タブ・カレンダー group).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuddleParticipant {
    pub organization_member_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub joined_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveHuddle {
    pub objective_id: String,
    pub session_id: String,
    #[serde(default)]
    pub participants: Vec<HuddleParticipant>,
    pub started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActiveHuddlesResponse {
    #[serde(default)]
    pub active_huddles: Vec<ActiveHuddle>,
}
