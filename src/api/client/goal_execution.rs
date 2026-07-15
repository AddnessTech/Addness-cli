use anyhow::Result;

use crate::api::{
    ActiveHuddlesResponse, ApiClient, ApiResponse, CalendarEvent, CalendarEventCompletionRequest,
    CalendarEventCompletionResponse, CodexTodaysGoalsApplyRequest, CreatePlannedTodoRequest,
    CreateTodayTodoRequest, DeletePlannedTodoResponse, ExecutionHistoryResponse, ExecutionRecord,
    ExecutionSummaryResponse, GenerateExecutionResponse, GoalCalendarEnvelope,
    GoalCalendarResponse, GoalHistoryResponse, GoalPreferenceResponse, MemberId,
    PlannedTodoMaterial, PlannedTodoView, RecordTodayTodoActivityRequest, TodayTodoActivityView,
    TodayTodoView, TodaysGoalsData, TodaysGoalsSummaryResponse, UpdateChatTodayTodoRequest,
    UpdateGoalPreferenceRequest, UpdatePlannedTodoRequest,
};

/// Build the `?a=1&b=2` query-string suffix, sharing the same encoding style
/// as `personal.rs`/`notification.rs`/`user.rs`.
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

    /// GET /api/v2/organizations/:id/todays-goals/summary
    pub async fn get_todays_goals_summary(
        &self,
        org_id: &str,
        date: &str,
        member_id: Option<&str>,
    ) -> Result<TodaysGoalsSummaryResponse> {
        let path = format!(
            "/api/v2/organizations/{org_id}/todays-goals/summary{}",
            query_suffix(&[("date", Some(date)), ("member_id", member_id)])
        );
        let resp: ApiResponse<TodaysGoalsSummaryResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    // -- 今日のToDo (today_todos) --------------------------------------------

    /// GET /api/v2/organizations/:id/today-todos
    pub async fn get_today_todos(
        &self,
        org_id: &str,
        date: Option<&str>,
        member_id: Option<&str>,
    ) -> Result<Vec<TodayTodoView>> {
        let path = format!(
            "/api/v2/organizations/{org_id}/today-todos{}",
            query_suffix(&[("date", date), ("member_id", member_id)])
        );
        let resp: ApiResponse<Vec<TodayTodoView>> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/organizations/:id/today-todos
    pub async fn add_today_todo(
        &self,
        org_id: &str,
        req: &CreateTodayTodoRequest,
    ) -> Result<TodayTodoView> {
        let path = format!("/api/v2/organizations/{org_id}/today-todos");
        let resp: ApiResponse<TodayTodoView> = self.post(&path, req).await?;
        Ok(resp.data)
    }

    /// PATCH /api/v2/organizations/:id/today-todos/:todoId (addness.chat-origin rows only)
    pub async fn update_chat_today_todo(
        &self,
        org_id: &str,
        todo_id: &str,
        req: &UpdateChatTodayTodoRequest,
    ) -> Result<TodayTodoView> {
        let path = format!("/api/v2/organizations/{org_id}/today-todos/{todo_id}");
        let resp: ApiResponse<TodayTodoView> = self.patch(&path, req).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/organizations/:id/today-todos/:todoId/activities
    pub async fn record_today_todo_activity(
        &self,
        org_id: &str,
        todo_id: &str,
        req: &RecordTodayTodoActivityRequest,
    ) -> Result<TodayTodoActivityView> {
        let path = format!("/api/v2/organizations/{org_id}/today-todos/{todo_id}/activities");
        let resp: ApiResponse<TodayTodoActivityView> = self.post(&path, req).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v2/organizations/:id/today-todos/:todoId
    /// `todo_id` accepts either an addness.chat-origin todo id or an
    /// objective id (the backend tries chat-origin first, then falls back to
    /// the objective-based row for the given `date`).
    pub async fn delete_today_todo(
        &self,
        org_id: &str,
        todo_id: &str,
        date: Option<&str>,
    ) -> Result<()> {
        let path = format!(
            "/api/v2/organizations/{org_id}/today-todos/{todo_id}{}",
            query_suffix(&[("date", date)])
        );
        self.delete_no_body(&path).await
    }

    // -- 材料プール (planned_todos) -------------------------------------------

    /// GET /api/v2/organizations/:id/planned-todos
    pub async fn list_planned_todos(&self, org_id: &str) -> Result<Vec<PlannedTodoView>> {
        let path = format!("/api/v2/organizations/{org_id}/planned-todos");
        let resp: ApiResponse<Vec<PlannedTodoView>> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/planned-todos/material
    pub async fn get_planned_todo_material(
        &self,
        org_id: &str,
        date: Option<&str>,
    ) -> Result<PlannedTodoMaterial> {
        let path = format!(
            "/api/v2/organizations/{org_id}/planned-todos/material{}",
            query_suffix(&[("date", date)])
        );
        let resp: ApiResponse<PlannedTodoMaterial> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/organizations/:id/planned-todos
    pub async fn create_planned_todo(
        &self,
        org_id: &str,
        req: &CreatePlannedTodoRequest,
    ) -> Result<PlannedTodoView> {
        let path = format!("/api/v2/organizations/{org_id}/planned-todos");
        let resp: ApiResponse<PlannedTodoView> = self.post(&path, req).await?;
        Ok(resp.data)
    }

    /// PATCH /api/v2/organizations/:id/planned-todos/:plannedId
    pub async fn update_planned_todo(
        &self,
        org_id: &str,
        planned_id: &str,
        req: &UpdatePlannedTodoRequest,
    ) -> Result<PlannedTodoView> {
        let path = format!("/api/v2/organizations/{org_id}/planned-todos/{planned_id}");
        let resp: ApiResponse<PlannedTodoView> = self.patch(&path, req).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v2/organizations/:id/planned-todos/:plannedId
    pub async fn delete_planned_todo(
        &self,
        org_id: &str,
        planned_id: &str,
    ) -> Result<DeletePlannedTodoResponse> {
        let path = format!("/api/v2/organizations/{org_id}/planned-todos/{planned_id}");
        let resp: ApiResponse<DeletePlannedTodoResponse> = self.delete_json(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/organizations/:id/planned-todos/:plannedId/adopt
    pub async fn adopt_planned_todo(
        &self,
        org_id: &str,
        planned_id: &str,
    ) -> Result<TodayTodoView> {
        let path = format!("/api/v2/organizations/{org_id}/planned-todos/{planned_id}/adopt");
        let resp: ApiResponse<TodayTodoView> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    // -- カレンダー -----------------------------------------------------------

    /// GET /api/v2/organizations/:id/calendar-events
    pub async fn get_calendar_events(
        &self,
        org_id: &str,
        date: Option<&str>,
    ) -> Result<Vec<CalendarEvent>> {
        let path = format!(
            "/api/v2/organizations/{org_id}/calendar-events{}",
            query_suffix(&[("date", date)])
        );
        let resp: ApiResponse<Vec<CalendarEvent>> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/organizations/:id/calendar-events/completion
    pub async fn complete_calendar_event(
        &self,
        org_id: &str,
        req: &CalendarEventCompletionRequest,
    ) -> Result<CalendarEventCompletionResponse> {
        let path = format!("/api/v2/organizations/{org_id}/calendar-events/completion");
        let resp: ApiResponse<CalendarEventCompletionResponse> = self.post(&path, req).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/goal-calendar
    pub async fn get_goal_calendar(
        &self,
        org_id: &str,
        from: &str,
        to: &str,
        member_id: Option<&str>,
        include_counts: bool,
    ) -> Result<GoalCalendarResponse> {
        let mut pairs = vec![
            ("from", Some(from)),
            ("to", Some(to)),
            ("member_id", member_id),
        ];
        if include_counts {
            pairs.push(("include", Some("counts")));
        }
        let path = format!(
            "/api/v2/organizations/{org_id}/goal-calendar{}",
            query_suffix(&pairs)
        );
        // Doubly-wrapped: `ApiResponse<{"data": GoalCalendarResponse}>`.
        let resp: ApiResponse<GoalCalendarEnvelope> = self.get(&path).await?;
        Ok(resp.data.data)
    }

    /// GET /api/v2/organizations/:id/goal-history
    pub async fn get_goal_history(
        &self,
        org_id: &str,
        date: &str,
        member_id: Option<&str>,
    ) -> Result<GoalHistoryResponse> {
        let path = format!(
            "/api/v2/organizations/{org_id}/goal-history{}",
            query_suffix(&[("date", Some(date)), ("member_id", member_id)])
        );
        let resp: ApiResponse<GoalHistoryResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    // -- ゴール開閉プリファレンス ------------------------------------------------

    /// GET /api/v2/organizations/:id/preferences/goal-collapse
    pub async fn get_goal_preference(&self, org_id: &str) -> Result<GoalPreferenceResponse> {
        let path = format!("/api/v2/organizations/{org_id}/preferences/goal-collapse");
        let resp: ApiResponse<GoalPreferenceResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PUT /api/v2/organizations/:id/preferences/goal-collapse
    pub async fn update_goal_preference(
        &self,
        org_id: &str,
        req: &UpdateGoalPreferenceRequest,
    ) -> Result<()> {
        let path = format!("/api/v2/organizations/{org_id}/preferences/goal-collapse");
        self.put_no_content(&path, req).await
    }

    // -- 実行タブサマリー / 実行記録 --------------------------------------------

    /// GET /api/v2/organizations/:id/execute-goals/summary (member-by-member completion counts)
    #[allow(clippy::too_many_arguments)]
    pub async fn get_execution_member_summary(
        &self,
        org_id: &str,
        from: Option<&str>,
        to: Option<&str>,
        query: Option<&str>,
        objective_id: Option<&str>,
        assignment_types: Option<&str>,
        tag_ids: Option<&str>,
        page: Option<u32>,
        page_size: Option<u32>,
        sort_by: Option<&str>,
        sort_dir: Option<&str>,
    ) -> Result<ExecutionSummaryResponse> {
        let page_str = page.map(|p| p.to_string());
        let page_size_str = page_size.map(|p| p.to_string());
        let path = format!(
            "/api/v2/organizations/{org_id}/execute-goals/summary{}",
            query_suffix(&[
                ("from", from),
                ("to", to),
                ("query", query),
                ("objective_id", objective_id),
                ("type", assignment_types),
                ("tag_ids", tag_ids),
                ("page", page_str.as_deref()),
                ("pageSize", page_size_str.as_deref()),
                ("sort_by", sort_by),
                ("sort_dir", sort_dir),
            ])
        );
        let resp: ApiResponse<ExecutionSummaryResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/execute-goals/generate — requires an X-Organization-ID
    /// header (no path/query org param), so callers must scope the client
    /// with `set_org_id` first.
    pub async fn generate_execution(&self) -> Result<GenerateExecutionResponse> {
        let resp: ApiResponse<GenerateExecutionResponse> =
            self.post_empty("/api/v2/execute-goals/generate").await?;
        Ok(resp.data)
    }

    /// PUT /api/v2/execute-goals/:id — requires an X-Organization-ID header.
    pub async fn update_execution(
        &self,
        record_id: &str,
        req: &serde_json::Value,
    ) -> Result<Option<ExecutionRecord>> {
        let path = format!("/api/v2/execute-goals/{record_id}");
        let resp: ApiResponse<Option<ExecutionRecord>> = self.put(&path, req).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/execute-goals/history
    #[allow(clippy::too_many_arguments)]
    pub async fn get_execution_history(
        &self,
        org_id: &str,
        from: &str,
        to: &str,
        objective_id: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<ExecutionHistoryResponse> {
        let limit_str = limit.map(|l| l.to_string());
        let offset_str = offset.map(|o| o.to_string());
        let path = format!(
            "/api/v2/execute-goals/history{}",
            query_suffix(&[
                ("organization_id", Some(org_id)),
                ("from", Some(from)),
                ("to", Some(to)),
                ("objective_id", objective_id),
                ("limit", limit_str.as_deref()),
                ("offset", offset_str.as_deref()),
            ])
        );
        let resp: ApiResponse<ExecutionHistoryResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    // -- アクティブハドル -------------------------------------------------------

    /// GET /api/v2/todays-goals/active-huddles — requires an
    /// X-Organization-ID header.
    pub async fn get_active_huddles(&self) -> Result<ActiveHuddlesResponse> {
        let resp: ApiResponse<ActiveHuddlesResponse> =
            self.get("/api/v2/todays-goals/active-huddles").await?;
        Ok(resp.data)
    }

    // -- Codex 用「今日のゴール」read/apply --------------------------------------

    /// GET /api/v2/codex/todays-goals/view — requires an X-Organization-ID
    /// header. Response passed through as raw JSON (see
    /// `CodexTodaysGoalsApplyRequest` doc comment for why).
    pub async fn get_codex_todays_goals_view(
        &self,
        date: Option<&str>,
    ) -> Result<serde_json::Value> {
        let path = format!(
            "/api/v2/codex/todays-goals/view{}",
            query_suffix(&[("date", date)])
        );
        let resp: ApiResponse<serde_json::Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/codex/todays-goals/apply — requires an X-Organization-ID
    /// header.
    pub async fn apply_codex_todays_goals(
        &self,
        req: &CodexTodaysGoalsApplyRequest,
    ) -> Result<serde_json::Value> {
        let resp: ApiResponse<serde_json::Value> =
            self.post("/api/v2/codex/todays-goals/apply", req).await?;
        Ok(resp.data)
    }
}
