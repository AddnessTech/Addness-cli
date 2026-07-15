use anyhow::Result;

use crate::api::{
    ApiClient, ApiResponse, GoalReportSchedule, GoalReportScheduleData,
    GoalReportScheduleDeleteData, GoalReportScheduleUpsertRequest,
};

impl ApiClient {
    /// GET /api/v2/organizations/:id/objectives/:goalId/report-schedule
    pub async fn get_goal_report_schedule(
        &self,
        org_id: &str,
        goal_id: &str,
    ) -> Result<Option<GoalReportSchedule>> {
        let path = format!("/api/v2/organizations/{org_id}/objectives/{goal_id}/report-schedule");
        let resp: ApiResponse<GoalReportScheduleData> = self.get(&path).await?;
        Ok(resp.data.schedule)
    }

    /// PUT /api/v2/organizations/:id/objectives/:goalId/report-schedule (upsert)
    pub async fn upsert_goal_report_schedule(
        &self,
        org_id: &str,
        goal_id: &str,
        frequency: &str,
        enabled: Option<bool>,
    ) -> Result<GoalReportSchedule> {
        let path = format!("/api/v2/organizations/{org_id}/objectives/{goal_id}/report-schedule");
        let body = GoalReportScheduleUpsertRequest {
            frequency: frequency.to_string(),
            enabled,
        };
        let resp: ApiResponse<GoalReportScheduleData> = self.put(&path, &body).await?;
        resp.data
            .schedule
            .ok_or_else(|| anyhow::anyhow!("Server did not return the upserted schedule"))
    }

    /// DELETE /api/v2/organizations/:id/objectives/:goalId/report-schedule
    pub async fn delete_goal_report_schedule(&self, org_id: &str, goal_id: &str) -> Result<bool> {
        let path = format!("/api/v2/organizations/{org_id}/objectives/{goal_id}/report-schedule");
        let resp: ApiResponse<GoalReportScheduleDeleteData> = self.delete_json(&path).await?;
        Ok(resp.data.success)
    }
}
