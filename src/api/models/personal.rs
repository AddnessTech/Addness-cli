use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalOrganizationEnsureResponse {
    pub organization_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<PersonalOrganizationBalance>,
}

// ---------------------------------------------------------------------------
// GET /api/v2/personal/today-list, /daily-activity — cross-organization
// "today" aggregation used by the execution tab's personal view.
// Backend reference: internal/goalexecution/usecase/get_personal_today.go,
// get_personal_daily_activity.go.
// ---------------------------------------------------------------------------

/// One "today's todo" item aggregated across all of the caller's
/// organizations. Embeds the same fields as `TodayTodoView`, plus the
/// originating organization and (for objective-based entries only) the
/// resolved objective-tree metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersonalTodayItem {
    pub organization_id: String,
    pub organization_name: String,
    #[serde(flatten)]
    pub today_todo: super::TodayTodoView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_objective_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<super::Owner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

/// One activity day's planned/done counts (a single "footstep" cell in the
/// personal history heatmap). Days with zero `planned` are omitted entirely
/// by the backend rather than returned as zero-valued entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyActivityCount {
    pub date: String,
    pub planned: i32,
    pub done: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalOrganizationBalance {
    pub balance_micro_usd: i64,
    pub cap_micro_usd: i64,
    pub balance_yen: i64,
    pub cap_yen: i64,
    pub remaining_percentage: f64,
    pub is_special: bool,
}
