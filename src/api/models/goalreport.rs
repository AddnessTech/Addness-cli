use serde::{Deserialize, Serialize};

// Goal activity report schedule API models (internal/goalreport).
// `POST .../report-schedule/test` is intentionally not implemented: it is a
// pre-release internal test-send endpoint slated for removal (see
// docs/cli-endpoint-coverage.md category 2).
// Backend reference: internal/goalreport/handler/endpoints/schedule.go.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalReportSchedule {
    pub id: String,
    pub organization_id: String,
    pub target_objective_id: String,
    pub frequency: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalReportScheduleData {
    #[serde(default)]
    pub schedule: Option<GoalReportSchedule>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoalReportScheduleUpsertRequest {
    pub frequency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalReportScheduleDeleteData {
    #[serde(default)]
    pub success: bool,
}
