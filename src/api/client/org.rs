use crate::api::{
    ApiClient, ApiResponse, CreateOrganizationRequest, EnabledFlagRequest, MyAdSettingRequest,
    Organization, OrganizationsResponse, PushTokenRegisterRequest, RegisterSubscriptionRequest,
    UpdateContextRequest, UpdateDefaultTimezoneRequest, UpdateOrganizationRequest,
};
use anyhow::Result;
use serde_json::Value;

pub struct CreateOrganizationParams {
    pub name: String,
    pub organization_type: String,
    pub team_scale: Option<String>,
    pub plan_type: Option<String>,
    pub industry: Option<String>,
    pub phone_number: Option<String>,
    pub browser_timezone: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Default)]
pub struct ListAllOrganizationsParams<'a> {
    pub name: Option<&'a str>,
    pub limit: Option<u16>,
    pub offset: Option<u64>,
}

impl ApiClient {
    pub async fn list_organizations(&self) -> Result<OrganizationsResponse> {
        self.get_without_org("/api/v2/organizations/me").await
    }

    pub async fn create_organization(
        &self,
        params: CreateOrganizationParams,
    ) -> Result<ApiResponse<Organization>> {
        let body = CreateOrganizationRequest {
            name: params.name,
            organization_type: params.organization_type,
            team_scale: params.team_scale,
            plan_type: params.plan_type,
            industry: params.industry,
            phone_number: params.phone_number,
            browser_timezone: params.browser_timezone,
            logo_url: params.logo_url,
        };
        self.post_without_org("/api/v1/team/organizations", &body)
            .await
    }

    pub async fn update_organization(
        &self,
        org_id: &str,
        name: &str,
    ) -> Result<ApiResponse<Organization>> {
        let path = format!("/api/v2/organizations/{org_id}");
        let body = UpdateOrganizationRequest {
            name: name.to_string(),
        };
        self.patch(&path, &body).await
    }

    pub async fn delete_organization(&self, org_id: &str) -> Result<()> {
        let path = format!("/api/v1/team/organizations/{org_id}");
        self.delete_no_body(&path).await
    }

    pub async fn update_organization_context(
        &self,
        org_id: &str,
        context_text: &str,
    ) -> Result<ApiResponse<Organization>> {
        let path = format!("/api/v2/organizations/{org_id}/context");
        let body = UpdateContextRequest {
            context_text: context_text.to_string(),
        };
        self.patch(&path, &body).await
    }

    // ---- Organization info / read endpoints ----
    //
    // Every organization endpoint (both the v1 `/team` handlers and the v2
    // `/organizations` handlers) wraps its payload in `{"data": ..., "message":
    // "success"}`, so these unwrap `ApiResponse<Value>` and return the inner
    // `data`. The response shapes vary per endpoint (and even mix snake_case and
    // camelCase field names), so they are surfaced as raw JSON rather than modeled.

    /// GET /api/v1/team/organizations/:id
    pub async fn get_organization(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v1/team/organizations/{org_id}");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/root_owner
    pub async fn get_organization_root_owner(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v1/team/organizations/{org_id}/root_owner");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/accessible_root
    pub async fn get_organization_accessible_root(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v1/team/organizations/{org_id}/accessible_root");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/ai_agent_member
    pub async fn get_organization_ai_agent_member(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v1/team/organizations/{org_id}/ai_agent_member");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/access-state
    pub async fn get_organization_access_state(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v1/team/organizations/{org_id}/access-state");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations?name=&limit=&offset=
    /// Subscription-guarded, paginated organization list (distinct from `/me`).
    pub async fn list_all_organizations(
        &self,
        params: ListAllOrganizationsParams<'_>,
    ) -> Result<Value> {
        let path = format!("/api/v2/organizations{}", list_all_query_suffix(&params));
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/context
    pub async fn get_organization_context(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/context");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/context/revisions?limit=
    pub async fn list_organization_context_revisions(
        &self,
        org_id: &str,
        limit: Option<u16>,
    ) -> Result<Value> {
        let suffix = match limit {
            Some(limit) => format!("?limit={limit}"),
            None => String::new(),
        };
        let path = format!("/api/v2/organizations/{org_id}/context/revisions{suffix}");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/admin/check
    pub async fn check_organization_admin(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/admin/check");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/current-member
    pub async fn get_organization_current_member(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/current-member");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    // ---- Organization settings / subscription write endpoints ----

    /// POST /api/v1/team/organizations/:id/push_tokens
    pub async fn register_organization_push_token(
        &self,
        org_id: &str,
        token: &str,
    ) -> Result<Value> {
        let path = format!("/api/v1/team/organizations/{org_id}/push_tokens");
        let body = PushTokenRegisterRequest {
            token: token.to_string(),
        };
        let resp: ApiResponse<Value> = self.post(&path, &body).await?;
        Ok(resp.data)
    }

    /// POST /api/v1/team/organization_subscriptions/register
    pub async fn register_organization_subscription(
        &self,
        univapay_subscription_id: &str,
    ) -> Result<Value> {
        let body = RegisterSubscriptionRequest {
            univapay_subscription_id: univapay_subscription_id.to_string(),
        };
        let resp: ApiResponse<Value> = self
            .post("/api/v1/team/organization_subscriptions/register", &body)
            .await?;
        Ok(resp.data)
    }

    /// PATCH /api/v1/team/organization_subscriptions/:id/cancel
    pub async fn cancel_organization_subscription(&self, subscription_id: &str) -> Result<Value> {
        let path = format!("/api/v1/team/organization_subscriptions/{subscription_id}/cancel");
        let resp: ApiResponse<Value> = self.patch_empty(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organization_subscriptions/current
    /// Resolves the subscription from the `X-Organization-ID` header.
    pub async fn get_current_organization_subscription(&self) -> Result<Value> {
        let resp: ApiResponse<Value> = self
            .get("/api/v1/team/organization_subscriptions/current")
            .await?;
        Ok(resp.data)
    }

    /// PATCH /api/v2/organizations/:id/default-timezone
    pub async fn update_organization_default_timezone(
        &self,
        org_id: &str,
        timezone: &str,
    ) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/default-timezone");
        let body = UpdateDefaultTimezoneRequest {
            default_timezone: timezone.to_string(),
        };
        let resp: ApiResponse<Value> = self.patch(&path, &body).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/onboarding-billing-state
    pub async fn get_organization_onboarding_billing_state(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/onboarding-billing-state");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/organizations/:id/onboarding-billing/require
    pub async fn require_organization_onboarding_billing(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/onboarding-billing/require");
        let resp: ApiResponse<Value> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/organizations/:id/onboarding-billing/free
    pub async fn complete_organization_onboarding_billing_free(
        &self,
        org_id: &str,
    ) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/onboarding-billing/free");
        let resp: ApiResponse<Value> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/ai-schedule-settings
    pub async fn get_organization_ai_schedule_settings(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/ai-schedule-settings");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PUT /api/v2/organizations/:id/ai-schedule-settings
    pub async fn set_organization_ai_schedule_settings(
        &self,
        org_id: &str,
        enabled: bool,
    ) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/ai-schedule-settings");
        let body = EnabledFlagRequest { enabled };
        let resp: ApiResponse<Value> = self.put(&path, &body).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/ad-settings
    pub async fn get_organization_ad_settings(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/ad-settings");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PUT /api/v2/organizations/:id/ad-settings
    pub async fn set_organization_ad_settings(&self, org_id: &str, enabled: bool) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/ad-settings");
        let body = EnabledFlagRequest { enabled };
        let resp: ApiResponse<Value> = self.put(&path, &body).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/ad-settings/me
    pub async fn get_my_organization_ad_settings(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/ad-settings/me");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PUT /api/v2/organizations/:id/ad-settings/me
    pub async fn set_my_organization_ad_settings(
        &self,
        org_id: &str,
        body: &MyAdSettingRequest,
    ) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/ad-settings/me");
        let resp: ApiResponse<Value> = self.put(&path, body).await?;
        Ok(resp.data)
    }

    /// PUT /api/v2/organizations/:id/logo (raw file bytes as the request body)
    pub async fn upload_organization_logo(
        &self,
        org_id: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/logo");
        let resp: ApiResponse<Value> = self.put_bytes(&path, bytes, content_type).await?;
        Ok(resp.data)
    }
}

/// Build the `?name=&limit=&offset=` query suffix for `GET /api/v2/organizations`.
/// Returns an empty string when no parameters are set (so no trailing `?`).
fn list_all_query_suffix(params: &ListAllOrganizationsParams<'_>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(name) = params.name {
            query.append_pair("name", name);
        }
        if let Some(limit) = params.limit {
            query.append_pair("limit", &limit.to_string());
        }
        if let Some(offset) = params.offset {
            query.append_pair("offset", &offset.to_string());
        }
        query.finish()
    };
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
}

#[cfg(test)]
mod tests {
    use super::{ListAllOrganizationsParams, list_all_query_suffix};

    #[test]
    fn list_all_query_suffix_is_empty_without_params() {
        assert_eq!(
            list_all_query_suffix(&ListAllOrganizationsParams::default()),
            ""
        );
    }

    #[test]
    fn list_all_query_suffix_encodes_all_params() {
        let suffix = list_all_query_suffix(&ListAllOrganizationsParams {
            name: Some("Acme Inc"),
            limit: Some(25),
            offset: Some(50),
        });
        assert_eq!(suffix, "?name=Acme+Inc&limit=25&offset=50");
    }

    #[test]
    fn list_all_query_suffix_includes_only_set_params() {
        let suffix = list_all_query_suffix(&ListAllOrganizationsParams {
            name: None,
            limit: Some(10),
            offset: None,
        });
        assert_eq!(suffix, "?limit=10");
    }
}
