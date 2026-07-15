use crate::api::{
    ApiClient, ApiResponse, CreateOrganizationRequest, Organization, OrganizationsResponse,
    UpdateContextRequest, UpdateOrganizationRequest,
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
