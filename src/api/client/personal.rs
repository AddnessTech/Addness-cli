use crate::api::{
    ApiClient, ApiResponse, DailyActivityCount, PersonalOrganizationEnsureResponse,
    PersonalTodayItem,
};
use anyhow::Result;

fn query_suffix(pairs: &[(&str, Option<&str>)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    let mut any = false;
    for (key, value) in pairs {
        if let Some(value) = value {
            serializer.append_pair(key, value);
            any = true;
        }
    }
    if !any {
        return String::new();
    }
    format!("?{}", serializer.finish())
}

impl ApiClient {
    /// POST /api/v1/team/personal-organization/ensure — idempotently ensures
    /// the caller's personal organization (Chat/Perfect Days billing target)
    /// exists, returning its ID and best-effort token balance.
    pub async fn ensure_personal_organization(&self) -> Result<PersonalOrganizationEnsureResponse> {
        let resp: ApiResponse<PersonalOrganizationEnsureResponse> = self
            .post_empty("/api/v1/team/personal-organization/ensure")
            .await?;
        Ok(resp.data)
    }

    /// GET /api/v2/personal/today-list?date=&exclude_completed= — the
    /// caller's "today's todos" aggregated across every organization they
    /// belong to (org-independent, unlike `today list` which is a single
    /// organization's `todays-goals`).
    pub async fn get_personal_today_list(
        &self,
        date: Option<&str>,
        exclude_completed: Option<bool>,
    ) -> Result<Vec<PersonalTodayItem>> {
        let exclude_completed_str = exclude_completed.map(|v| v.to_string());
        let path = format!(
            "/api/v2/personal/today-list{}",
            query_suffix(&[
                ("date", date),
                ("exclude_completed", exclude_completed_str.as_deref()),
            ])
        );
        let resp: ApiResponse<Vec<PersonalTodayItem>> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/personal/daily-activity?start=&end= — cross-organization
    /// planned/done counts per activity day (used to render the "footsteps"
    /// history heatmap).
    pub async fn get_personal_daily_activity(
        &self,
        start: &str,
        end: &str,
    ) -> Result<Vec<DailyActivityCount>> {
        let path = format!(
            "/api/v2/personal/daily-activity{}",
            query_suffix(&[("start", Some(start)), ("end", Some(end))])
        );
        let resp: ApiResponse<Vec<DailyActivityCount>> = self.get(&path).await?;
        Ok(resp.data)
    }
}
