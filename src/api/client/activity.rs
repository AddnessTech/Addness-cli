use crate::api::{
    ActivityLogListResponse, ActivityLogSummaryResponse, ApiClient, ApiResponse,
    GoalActivitySummaryResponse,
};
use anyhow::Result;

#[derive(Default)]
pub struct ActivityLogByMemberParams<'a> {
    pub member_id: &'a str,
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
    pub event_types: &'a [String],
    pub event_categories: &'a [String],
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Default)]
pub struct ActivityLogByGoalParams<'a> {
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
    pub event_types: &'a [String],
    pub include_children: bool,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Default)]
pub struct ActivityLogSummaryParams<'a> {
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
}

#[derive(Default)]
pub struct GoalActivitySummaryParams<'a> {
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
    pub include_children: bool,
    pub limit: Option<u32>,
}

impl ApiClient {
    pub async fn list_activity_logs_by_member(
        &self,
        org_id: &str,
        params: ActivityLogByMemberParams<'_>,
    ) -> Result<ActivityLogListResponse> {
        // Serializer は非Sendなので、ブロック内で文字列に確定させて drop する（notification.rsの流儀に合わせる）。
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            query.append_pair("member_id", params.member_id);
            if let Some(start) = params.start_date {
                query.append_pair("start_date", start);
            }
            if let Some(end) = params.end_date {
                query.append_pair("end_date", end);
            }
            for event_type in params.event_types {
                query.append_pair("event_types[]", event_type);
            }
            for category in params.event_categories {
                query.append_pair("event_categories[]", category);
            }
            if let Some(limit) = params.limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(offset) = params.offset {
                query.append_pair("offset", &offset.to_string());
            }
            query.finish()
        };
        let path = format!("/api/v1/team/organizations/{org_id}/activity-logs/by-member?{query}");
        let resp: ApiResponse<ActivityLogListResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn list_activity_logs_by_goal(
        &self,
        org_id: &str,
        goal_id: &str,
        params: ActivityLogByGoalParams<'_>,
    ) -> Result<ActivityLogListResponse> {
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            if let Some(start) = params.start_date {
                query.append_pair("start_date", start);
            }
            if let Some(end) = params.end_date {
                query.append_pair("end_date", end);
            }
            for event_type in params.event_types {
                query.append_pair("event_types[]", event_type);
            }
            if params.include_children {
                query.append_pair("include_children", "true");
            }
            if let Some(limit) = params.limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(offset) = params.offset {
                query.append_pair("offset", &offset.to_string());
            }
            query.finish()
        };
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let path = format!(
            "/api/v1/team/organizations/{org_id}/activity-logs/objectives/{goal_id}{suffix}"
        );
        let resp: ApiResponse<ActivityLogListResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn get_activity_log_summary(
        &self,
        org_id: &str,
        params: ActivityLogSummaryParams<'_>,
    ) -> Result<ActivityLogSummaryResponse> {
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            if let Some(start) = params.start_date {
                query.append_pair("start_date", start);
            }
            if let Some(end) = params.end_date {
                query.append_pair("end_date", end);
            }
            query.finish()
        };
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let path = format!("/api/v1/team/organizations/{org_id}/activity-logs/summary{suffix}");
        let resp: ApiResponse<ActivityLogSummaryResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn get_goal_activity_summary(
        &self,
        org_id: &str,
        goal_id: &str,
        params: GoalActivitySummaryParams<'_>,
    ) -> Result<GoalActivitySummaryResponse> {
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            if let Some(start) = params.start_date {
                query.append_pair("start_date", start);
            }
            if let Some(end) = params.end_date {
                query.append_pair("end_date", end);
            }
            if params.include_children {
                query.append_pair("include_children", "true");
            }
            if let Some(limit) = params.limit {
                query.append_pair("limit", &limit.to_string());
            }
            query.finish()
        };
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let path = format!(
            "/api/v2/organizations/{org_id}/activity-logs/objectives/{goal_id}/summary{suffix}"
        );
        let resp: ApiResponse<GoalActivitySummaryResponse> = self.get(&path).await?;
        Ok(resp.data)
    }
}
