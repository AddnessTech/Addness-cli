use crate::api::{
    ApiClient, ApiResponse, CountsByObjectiveResponse, EmailDestination, MarkNotificationsResponse,
    NotificationCountResponse, NotificationIdsRequest, NotificationListResponse,
    NotificationSetting, NotificationSettingRequest,
};
use anyhow::Result;

#[derive(Default)]
pub struct ListNotificationsParams<'a> {
    pub limit: Option<u16>,
    pub offset: Option<u64>,
    pub read: Option<bool>,
    pub goal_id: Option<&'a str>,
    pub categories: &'a [String],
    pub sort: Option<&'a str>,
}

impl ApiClient {
    pub async fn list_notifications(
        &self,
        org_id: &str,
        params: ListNotificationsParams<'_>,
    ) -> Result<NotificationListResponse> {
        // Serializer は非Sendなので、ブロック内で文字列に確定させて drop する（comment.rsの流儀に合わせる）。
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            if let Some(limit) = params.limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(offset) = params.offset {
                query.append_pair("offset", &offset.to_string());
            }
            if let Some(read) = params.read {
                query.append_pair("read", if read { "true" } else { "false" });
            }
            if let Some(goal_id) = params.goal_id {
                query.append_pair("objectiveId", goal_id);
            }
            for category in params.categories {
                query.append_pair("category", category);
            }
            if let Some(sort) = params.sort {
                query.append_pair("sort", sort);
            }
            query.finish()
        };
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let path = format!("/api/v2/organizations/{org_id}/notifications{suffix}");
        self.get(&path).await
    }

    pub async fn count_notifications(
        &self,
        org_id: &str,
        categories: &[String],
    ) -> Result<NotificationCountResponse> {
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            for category in categories {
                query.append_pair("category", category);
            }
            query.finish()
        };
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let path = format!("/api/v2/organizations/{org_id}/notifications/count{suffix}");
        self.get(&path).await
    }

    pub async fn count_notifications_by_goal(
        &self,
        org_id: &str,
    ) -> Result<CountsByObjectiveResponse> {
        let path = format!("/api/v2/organizations/{org_id}/notifications/counts-by-objective");
        self.get(&path).await
    }

    pub async fn mark_notifications_read(
        &self,
        org_id: &str,
        notification_ids: &[String],
    ) -> Result<MarkNotificationsResponse> {
        let path = format!("/api/v2/organizations/{org_id}/notifications/mark-read");
        let body = NotificationIdsRequest {
            notification_ids: notification_ids.to_vec(),
        };
        self.post(&path, &body).await
    }

    pub async fn mark_notifications_unread(
        &self,
        org_id: &str,
        notification_ids: &[String],
    ) -> Result<MarkNotificationsResponse> {
        let path = format!("/api/v2/organizations/{org_id}/notifications/mark-unread");
        let body = NotificationIdsRequest {
            notification_ids: notification_ids.to_vec(),
        };
        self.post(&path, &body).await
    }

    pub async fn mark_all_notifications_read(
        &self,
        org_id: &str,
    ) -> Result<MarkNotificationsResponse> {
        let path = format!("/api/v2/organizations/{org_id}/notifications/mark-all-read");
        self.post_empty(&path).await
    }

    pub async fn list_notification_settings(&self) -> Result<Vec<NotificationSetting>> {
        let resp: ApiResponse<Vec<NotificationSetting>> =
            self.get("/api/v1/team/notification_settings").await?;
        Ok(resp.data)
    }

    pub async fn create_notification_setting(
        &self,
        req: &NotificationSettingRequest,
    ) -> Result<NotificationSetting> {
        let resp: ApiResponse<NotificationSetting> =
            self.post("/api/v1/team/notification_settings", req).await?;
        Ok(resp.data)
    }

    pub async fn update_notification_setting(
        &self,
        setting_id: &str,
        req: &NotificationSettingRequest,
    ) -> Result<NotificationSetting> {
        let path = format!("/api/v1/team/notification_settings/{setting_id}");
        let resp: ApiResponse<NotificationSetting> = self.patch(&path, req).await?;
        Ok(resp.data)
    }

    pub async fn list_email_destinations(&self) -> Result<Vec<EmailDestination>> {
        let resp: ApiResponse<Vec<EmailDestination>> =
            self.get("/api/v1/team/email_destinations").await?;
        Ok(resp.data)
    }
}
