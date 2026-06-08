use anyhow::Result;

use crate::api::{ApiClient, ApiResponse, MemberId, TodaysGoalsData};

impl ApiClient {
    /// Get today's goals for the current user or a specific member.
    pub async fn get_todays_goals(
        &self,
        org_id: &str,
        date: Option<&str>,
        member_id: Option<&MemberId>,
    ) -> Result<ApiResponse<TodaysGoalsData>> {
        let mut path = format!("/api/v2/organizations/{}/todays-goals", org_id);
        let mut query_params = Vec::new();

        if let Some(d) = date {
            query_params.push(format!("date={}", d));
        }
        if let Some(m) = member_id {
            query_params.push(format!("member_id={}", m.as_str()));
        }

        if !query_params.is_empty() {
            path.push('?');
            path.push_str(&query_params.join("&"));
        }

        self.get(&path).await
    }
}
