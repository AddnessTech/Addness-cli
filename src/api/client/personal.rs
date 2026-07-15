use anyhow::Result;

use crate::api::{
    ApiClient, ApiResponse, PersonalAgentSession, PersonalAgentSessionCreateRequest,
    PersonalAgentSessionUpdateRequest, PersonalDailyEntry, PersonalMarkdownAnalyzeResponse,
    PersonalMarkdownEditRequest, PersonalMarkdownEditResponse, PersonalNow,
    PersonalOrganizationEnsureResponse, PersonalProject, PersonalProjectCreateRequest,
    PersonalProjectUpdateRequest, PersonalResetResponse, PersonalTextPatchRequest,
    PersonalTextPatchResponse, PersonalTodayAppendRequest,
};

/// Build the query-string suffix for the personal `list`/`get` endpoints
/// (`?status=&limit=` / `?timezone=`), sharing the same encoding style as
/// `notification.rs`/`user.rs`.
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
    /// GET /api/v2/personal/now
    pub async fn get_personal_now(&self) -> Result<PersonalNow> {
        let resp: ApiResponse<PersonalNow> = self.get("/api/v2/personal/now").await?;
        Ok(resp.data)
    }

    /// GET /api/v2/personal/today?timezone=
    pub async fn get_personal_today(&self, timezone: Option<&str>) -> Result<PersonalDailyEntry> {
        let path = format!(
            "/api/v2/personal/today{}",
            query_suffix(&[("timezone", timezone)])
        );
        let resp: ApiResponse<PersonalDailyEntry> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/personal/today/append
    pub async fn append_personal_today(
        &self,
        body: &str,
        timezone: Option<&str>,
    ) -> Result<PersonalDailyEntry> {
        let req = PersonalTodayAppendRequest {
            body: body.to_string(),
            timezone: timezone.map(str::to_string),
        };
        let resp: ApiResponse<PersonalDailyEntry> =
            self.post("/api/v2/personal/today/append", &req).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/personal/days/:date?timezone=
    pub async fn get_personal_day(
        &self,
        date: &str,
        timezone: Option<&str>,
    ) -> Result<PersonalDailyEntry> {
        let path = format!(
            "/api/v2/personal/days/{date}{}",
            query_suffix(&[("timezone", timezone)])
        );
        let resp: ApiResponse<PersonalDailyEntry> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/personal/text-patch
    pub async fn patch_personal_text(
        &self,
        req: &PersonalTextPatchRequest,
    ) -> Result<PersonalTextPatchResponse> {
        let resp: ApiResponse<PersonalTextPatchResponse> =
            self.post("/api/v2/personal/text-patch", req).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/personal/markdown/analyze?target=&id=&date=&timezone=
    pub async fn analyze_personal_markdown(
        &self,
        target: &str,
        id: Option<&str>,
        date: Option<&str>,
        timezone: Option<&str>,
    ) -> Result<PersonalMarkdownAnalyzeResponse> {
        let path = format!(
            "/api/v2/personal/markdown/analyze{}",
            query_suffix(&[
                ("target", Some(target)),
                ("id", id),
                ("date", date),
                ("timezone", timezone),
            ])
        );
        let resp: ApiResponse<PersonalMarkdownAnalyzeResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    async fn edit_personal_markdown(
        &self,
        path: &str,
        req: &PersonalMarkdownEditRequest,
    ) -> Result<PersonalMarkdownEditResponse> {
        let resp: ApiResponse<PersonalMarkdownEditResponse> = self.post(path, req).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/personal/markdown/replace-section
    pub async fn replace_personal_markdown_section(
        &self,
        req: &PersonalMarkdownEditRequest,
    ) -> Result<PersonalMarkdownEditResponse> {
        self.edit_personal_markdown("/api/v2/personal/markdown/replace-section", req)
            .await
    }

    /// POST /api/v2/personal/markdown/upsert-section
    pub async fn upsert_personal_markdown_section(
        &self,
        req: &PersonalMarkdownEditRequest,
    ) -> Result<PersonalMarkdownEditResponse> {
        self.edit_personal_markdown("/api/v2/personal/markdown/upsert-section", req)
            .await
    }

    /// POST /api/v2/personal/markdown/upsert-list-item
    pub async fn upsert_personal_markdown_list_item(
        &self,
        req: &PersonalMarkdownEditRequest,
    ) -> Result<PersonalMarkdownEditResponse> {
        self.edit_personal_markdown("/api/v2/personal/markdown/upsert-list-item", req)
            .await
    }

    /// POST /api/v2/personal/markdown/replace-document
    pub async fn replace_personal_markdown_document(
        &self,
        req: &PersonalMarkdownEditRequest,
    ) -> Result<PersonalMarkdownEditResponse> {
        self.edit_personal_markdown("/api/v2/personal/markdown/replace-document", req)
            .await
    }

    /// POST /api/v2/personal/markdown/append-log-entry
    pub async fn append_personal_markdown_log_entry(
        &self,
        req: &PersonalMarkdownEditRequest,
    ) -> Result<PersonalMarkdownEditResponse> {
        self.edit_personal_markdown("/api/v2/personal/markdown/append-log-entry", req)
            .await
    }

    /// GET /api/v2/personal/agent-sessions?status=&limit=
    pub async fn list_personal_agent_sessions(
        &self,
        status: Option<&str>,
        limit: Option<u16>,
    ) -> Result<Vec<PersonalAgentSession>> {
        let limit_str = limit.map(|l| l.to_string());
        let path = format!(
            "/api/v2/personal/agent-sessions{}",
            query_suffix(&[("status", status), ("limit", limit_str.as_deref())])
        );
        let resp: ApiResponse<Vec<PersonalAgentSession>> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/personal/agent-sessions
    pub async fn create_personal_agent_session(
        &self,
        req: &PersonalAgentSessionCreateRequest,
    ) -> Result<PersonalAgentSession> {
        let resp: ApiResponse<PersonalAgentSession> =
            self.post("/api/v2/personal/agent-sessions", req).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/personal/agent-sessions/:id
    pub async fn get_personal_agent_session(&self, id: &str) -> Result<PersonalAgentSession> {
        let path = format!("/api/v2/personal/agent-sessions/{id}");
        let resp: ApiResponse<PersonalAgentSession> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PATCH /api/v2/personal/agent-sessions/:id
    pub async fn update_personal_agent_session(
        &self,
        id: &str,
        req: &PersonalAgentSessionUpdateRequest,
    ) -> Result<PersonalAgentSession> {
        let path = format!("/api/v2/personal/agent-sessions/{id}");
        let resp: ApiResponse<PersonalAgentSession> = self.patch(&path, req).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/personal/projects?status=&limit=
    pub async fn list_personal_projects(
        &self,
        status: Option<&str>,
        limit: Option<u16>,
    ) -> Result<Vec<PersonalProject>> {
        let limit_str = limit.map(|l| l.to_string());
        let path = format!(
            "/api/v2/personal/projects{}",
            query_suffix(&[("status", status), ("limit", limit_str.as_deref())])
        );
        let resp: ApiResponse<Vec<PersonalProject>> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/personal/projects
    pub async fn create_personal_project(
        &self,
        req: &PersonalProjectCreateRequest,
    ) -> Result<PersonalProject> {
        let resp: ApiResponse<PersonalProject> =
            self.post("/api/v2/personal/projects", req).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/personal/projects/:id
    pub async fn get_personal_project(&self, id: &str) -> Result<PersonalProject> {
        let path = format!("/api/v2/personal/projects/{id}");
        let resp: ApiResponse<PersonalProject> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PATCH /api/v2/personal/projects/:id
    pub async fn update_personal_project(
        &self,
        id: &str,
        req: &PersonalProjectUpdateRequest,
    ) -> Result<PersonalProject> {
        let path = format!("/api/v2/personal/projects/{id}");
        let resp: ApiResponse<PersonalProject> = self.patch(&path, req).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v2/personal/reset — wipes the caller's entire personal
    /// context (now/days/projects). Backend responds 200 with `{"ok":true}`
    /// rather than 204, so this uses `delete_json` (not `delete_no_body`).
    pub async fn reset_personal(&self) -> Result<PersonalResetResponse> {
        let resp: ApiResponse<PersonalResetResponse> =
            self.delete_json("/api/v2/personal/reset").await?;
        Ok(resp.data)
    }

    /// POST /api/v1/team/personal-organization/ensure — idempotently ensures
    /// the caller's personal organization (Chat/Perfect Days billing target)
    /// exists, returning its ID and best-effort token balance.
    pub async fn ensure_personal_organization(&self) -> Result<PersonalOrganizationEnsureResponse> {
        let resp: ApiResponse<PersonalOrganizationEnsureResponse> = self
            .post_empty("/api/v1/team/personal-organization/ensure")
            .await?;
        Ok(resp.data)
    }
}
