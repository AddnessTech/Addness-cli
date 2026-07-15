use anyhow::Result;

use crate::api::{
    ApiClient, ApiResponse, Skill, SkillCreateRequest, SkillListResponse, SkillPerformance,
    SkillResource, SkillResourceCreateRequest, SkillResourceListItem, SkillResourceUpdateRequest,
    SkillSearchResponse, SkillUpdateRequest,
};

/// Build the query-string suffix for `limit`/`offset`/`q` pairs, sharing the
/// same encoding style as `personal.rs`/`notification.rs`.
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
    // -- Skill CRUD ----------------------------------------------------------

    /// POST /api/v1/team/organizations/:id/skills
    pub async fn create_skill(&self, org_id: &str, req: &SkillCreateRequest) -> Result<Skill> {
        let path = format!("/api/v1/team/organizations/{org_id}/skills");
        let resp: ApiResponse<Skill> = self.post(&path, req).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/skills?limit=&offset=
    pub async fn list_skills(
        &self,
        org_id: &str,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<SkillListResponse> {
        let limit_str = limit.map(|l| l.to_string());
        let offset_str = offset.map(|o| o.to_string());
        let path = format!(
            "/api/v1/team/organizations/{org_id}/skills{}",
            query_suffix(&[
                ("limit", limit_str.as_deref()),
                ("offset", offset_str.as_deref())
            ])
        );
        let resp: ApiResponse<SkillListResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/general-skills?limit=&offset=
    pub async fn list_general_skills(
        &self,
        org_id: &str,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<SkillListResponse> {
        let limit_str = limit.map(|l| l.to_string());
        let offset_str = offset.map(|o| o.to_string());
        let path = format!(
            "/api/v1/team/organizations/{org_id}/general-skills{}",
            query_suffix(&[
                ("limit", limit_str.as_deref()),
                ("offset", offset_str.as_deref())
            ])
        );
        let resp: ApiResponse<SkillListResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/skills/search?q=&limit=
    pub async fn search_skills(
        &self,
        org_id: &str,
        keyword: &str,
        limit: Option<u32>,
    ) -> Result<SkillSearchResponse> {
        let limit_str = limit.map(|l| l.to_string());
        let path = format!(
            "/api/v1/team/organizations/{org_id}/skills/search{}",
            query_suffix(&[("q", Some(keyword)), ("limit", limit_str.as_deref())])
        );
        let resp: ApiResponse<SkillSearchResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/skills/:skillID
    pub async fn get_skill(&self, org_id: &str, skill_id: &str) -> Result<Skill> {
        let path = format!("/api/v1/team/organizations/{org_id}/skills/{skill_id}");
        let resp: ApiResponse<Skill> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PATCH /api/v1/team/organizations/:id/skills/:skillID
    pub async fn update_skill(
        &self,
        org_id: &str,
        skill_id: &str,
        req: &SkillUpdateRequest,
    ) -> Result<Skill> {
        let path = format!("/api/v1/team/organizations/{org_id}/skills/{skill_id}");
        let resp: ApiResponse<Skill> = self.patch(&path, req).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v1/team/organizations/:id/skills/:skillID
    pub async fn delete_skill(&self, org_id: &str, skill_id: &str) -> Result<()> {
        let path = format!("/api/v1/team/organizations/{org_id}/skills/{skill_id}");
        self.delete_no_body(&path).await
    }

    /// GET /api/v1/team/organizations/:id/skills/:skillID/performance
    pub async fn get_skill_performance(
        &self,
        org_id: &str,
        skill_id: &str,
    ) -> Result<SkillPerformance> {
        let path = format!("/api/v1/team/organizations/{org_id}/skills/{skill_id}/performance");
        let resp: ApiResponse<SkillPerformance> = self.get(&path).await?;
        Ok(resp.data)
    }

    // -- Skill Refinement (改善提案の承認・却下) --------------------------------

    /// POST /api/v1/team/organizations/:id/skills/refinements/:refinementID/accept
    pub async fn accept_skill_refinement(
        &self,
        org_id: &str,
        refinement_id: &str,
    ) -> Result<Skill> {
        let path = format!(
            "/api/v1/team/organizations/{org_id}/skills/refinements/{refinement_id}/accept"
        );
        let resp: ApiResponse<Skill> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v1/team/organizations/:id/skills/refinements/:refinementID/reject
    pub async fn reject_skill_refinement(&self, org_id: &str, refinement_id: &str) -> Result<()> {
        let path = format!(
            "/api/v1/team/organizations/{org_id}/skills/refinements/{refinement_id}/reject"
        );
        self.post_empty_no_content(&path).await
    }

    // -- Skill Resource CRUD ---------------------------------------------------

    /// POST /api/v1/team/organizations/:id/skills/:skillID/resources
    pub async fn create_skill_resource(
        &self,
        org_id: &str,
        skill_id: &str,
        req: &SkillResourceCreateRequest,
    ) -> Result<SkillResource> {
        let path = format!("/api/v1/team/organizations/{org_id}/skills/{skill_id}/resources");
        let resp: ApiResponse<SkillResource> = self.post(&path, req).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/skills/:skillID/resources
    pub async fn list_skill_resources(
        &self,
        org_id: &str,
        skill_id: &str,
    ) -> Result<Vec<SkillResourceListItem>> {
        let path = format!("/api/v1/team/organizations/{org_id}/skills/{skill_id}/resources");
        let resp: ApiResponse<Vec<SkillResourceListItem>> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/skills/:skillID/resources/:resourceID
    pub async fn get_skill_resource(
        &self,
        org_id: &str,
        skill_id: &str,
        resource_id: &str,
    ) -> Result<SkillResource> {
        let path = format!(
            "/api/v1/team/organizations/{org_id}/skills/{skill_id}/resources/{resource_id}"
        );
        let resp: ApiResponse<SkillResource> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PUT /api/v1/team/organizations/:id/skills/:skillID/resources/:resourceID
    /// (no PATCH route is registered for resource updates).
    pub async fn update_skill_resource(
        &self,
        org_id: &str,
        skill_id: &str,
        resource_id: &str,
        req: &SkillResourceUpdateRequest,
    ) -> Result<SkillResource> {
        let path = format!(
            "/api/v1/team/organizations/{org_id}/skills/{skill_id}/resources/{resource_id}"
        );
        let resp: ApiResponse<SkillResource> = self.put(&path, req).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v1/team/organizations/:id/skills/:skillID/resources/:resourceID
    pub async fn delete_skill_resource(
        &self,
        org_id: &str,
        skill_id: &str,
        resource_id: &str,
    ) -> Result<()> {
        let path = format!(
            "/api/v1/team/organizations/{org_id}/skills/{skill_id}/resources/{resource_id}"
        );
        self.delete_no_body(&path).await
    }
}

#[cfg(test)]
mod tests {
    use super::query_suffix;

    #[test]
    fn query_suffix_is_empty_without_any_value() {
        assert_eq!(query_suffix(&[("limit", None), ("offset", None)]), "");
    }

    #[test]
    fn query_suffix_encodes_present_values_only() {
        assert_eq!(
            query_suffix(&[("q", Some("hello world")), ("limit", None)]),
            "?q=hello+world"
        );
        assert_eq!(
            query_suffix(&[("limit", Some("10")), ("offset", Some("5"))]),
            "?limit=10&offset=5"
        );
    }
}
