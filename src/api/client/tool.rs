use anyhow::Result;

use crate::api::{
    ApiClient, ApiResponse, Tool, ToolCreateRequest, ToolExecuteRequest, ToolExecuteResponse,
    ToolListResponse, ToolSearchResponse, ToolUpdateRequest,
};

/// Build the query-string suffix for `limit`/`offset`/`q` pairs, sharing the
/// same encoding style as `skill.rs`/`personal.rs`.
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
    /// POST /api/v1/team/organizations/:id/tools
    pub async fn create_tool(&self, org_id: &str, req: &ToolCreateRequest) -> Result<Tool> {
        let path = format!("/api/v1/team/organizations/{org_id}/tools");
        let resp: ApiResponse<Tool> = self.post(&path, req).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/tools?limit=&offset=
    pub async fn list_tools(
        &self,
        org_id: &str,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<ToolListResponse> {
        let limit_str = limit.map(|l| l.to_string());
        let offset_str = offset.map(|o| o.to_string());
        let path = format!(
            "/api/v1/team/organizations/{org_id}/tools{}",
            query_suffix(&[
                ("limit", limit_str.as_deref()),
                ("offset", offset_str.as_deref())
            ])
        );
        let resp: ApiResponse<ToolListResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/tools/search?q=&limit=
    pub async fn search_tools(
        &self,
        org_id: &str,
        keyword: &str,
        limit: Option<u32>,
    ) -> Result<ToolSearchResponse> {
        let limit_str = limit.map(|l| l.to_string());
        let path = format!(
            "/api/v1/team/organizations/{org_id}/tools/search{}",
            query_suffix(&[("q", Some(keyword)), ("limit", limit_str.as_deref())])
        );
        let resp: ApiResponse<ToolSearchResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/team/organizations/:id/tools/:toolID
    pub async fn get_tool(&self, org_id: &str, tool_id: &str) -> Result<Tool> {
        let path = format!("/api/v1/team/organizations/{org_id}/tools/{tool_id}");
        let resp: ApiResponse<Tool> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PATCH /api/v1/team/organizations/:id/tools/:toolID
    pub async fn update_tool(
        &self,
        org_id: &str,
        tool_id: &str,
        req: &ToolUpdateRequest,
    ) -> Result<Tool> {
        let path = format!("/api/v1/team/organizations/{org_id}/tools/{tool_id}");
        let resp: ApiResponse<Tool> = self.patch(&path, req).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v1/team/organizations/:id/tools/:toolID
    pub async fn delete_tool(&self, org_id: &str, tool_id: &str) -> Result<()> {
        let path = format!("/api/v1/team/organizations/{org_id}/tools/{tool_id}");
        self.delete_no_body(&path).await
    }

    /// POST /api/v1/team/organizations/:id/tools/:toolID/execute
    pub async fn execute_tool(
        &self,
        org_id: &str,
        tool_id: &str,
        req: &ToolExecuteRequest,
    ) -> Result<ToolExecuteResponse> {
        let path = format!("/api/v1/team/organizations/{org_id}/tools/{tool_id}/execute");
        let resp: ApiResponse<ToolExecuteResponse> = self.post(&path, req).await?;
        Ok(resp.data)
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
            query_suffix(&[("q", Some("build script")), ("limit", Some("5"))]),
            "?q=build+script&limit=5"
        );
    }
}
