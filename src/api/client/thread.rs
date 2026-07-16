use anyhow::Result;
use futures::TryStreamExt;

use super::issue::encode_path_segment;
use crate::api::{
    ActionTraceListResponse, ApiClient, ApiResponse, MessageEditRequest, QuestionRespondRequest,
    ThreadActionResultResponse, ThreadChatRequest, ThreadCreateRequest, ThreadMessageListResponse,
    ThreadResponse, ThreadShareLinkResponse, ThreadUpdateRequest, ToolConfirmationRespondRequest,
};

/// Base path for the legacy V1 "AI エージェント" Thread routes
/// (`presentation/routes/api.go`'s `team.Group("/ai")` under Clerk/API-key
/// auth, `x-organization-id` header resolves the organization).
const THREADS_BASE: &str = "/api/v1/team/ai/threads";

/// Query parameters for `GET /api/v1/team/ai/threads`. `objective_id`
/// requires the backend's implicit `threadScope=objective` pairing — this
/// client sets that automatically when `objective_id` is present (see
/// `application/requests/ai/thread_chat_request.go`'s
/// `ThreadListRequest.Validate()`).
#[derive(Debug, Clone, Default)]
pub struct ThreadListParams<'a> {
    pub agent_id: Option<&'a str>,
    pub scope: Option<&'a str>,
    pub objective_id: Option<&'a str>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

fn thread_list_query_suffix(params: &ThreadListParams<'_>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(agent_id) = params.agent_id {
            query.append_pair("agentId", agent_id);
        }
        if let Some(scope) = params.scope {
            query.append_pair("scope", scope);
        }
        if let Some(objective_id) = params.objective_id {
            query.append_pair("objectiveId", objective_id);
            query.append_pair("threadScope", "objective");
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

impl ApiClient {
    /// `POST /api/v1/team/ai/threads` — create a new thread.
    pub async fn create_thread(&self, req: &ThreadCreateRequest) -> Result<ThreadResponse> {
        self.post(THREADS_BASE, req).await
    }

    /// `GET /api/v1/team/ai/threads` — list threads (server-paginated;
    /// unlike goal-chat/master-plan/core-values/todo-chat, this legacy
    /// route implements real `limit`/`offset` pagination — confirmed
    /// against `ThreadUsecase.ListThreads` in the Go source).
    pub async fn list_threads(
        &self,
        params: &ThreadListParams<'_>,
    ) -> Result<crate::api::ThreadListResponse> {
        let path = format!("{THREADS_BASE}{}", thread_list_query_suffix(params));
        self.get(&path).await
    }

    /// `GET /api/v1/team/ai/threads/:id` — fetch a single thread.
    pub async fn get_thread(&self, thread_id: &str) -> Result<ThreadResponse> {
        let path = format!("{THREADS_BASE}/{}", encode_path_segment(thread_id));
        self.get(&path).await
    }

    /// `PATCH /api/v1/team/ai/threads/:id` — rename/update thread metadata.
    pub async fn update_thread(
        &self,
        thread_id: &str,
        req: &ThreadUpdateRequest,
    ) -> Result<ThreadResponse> {
        let path = format!("{THREADS_BASE}/{}", encode_path_segment(thread_id));
        self.patch(&path, req).await
    }

    /// `DELETE /api/v1/team/ai/threads/:id` — permanently delete a thread.
    pub async fn delete_thread(&self, thread_id: &str) -> Result<()> {
        let path = format!("{THREADS_BASE}/{}", encode_path_segment(thread_id));
        self.delete_no_body(&path).await
    }

    /// `GET /api/v1/team/ai/threads/:id/messages` — list a thread's
    /// messages. `include_internal=true` surfaces internal-only messages
    /// (debug); omitted/`false` returns the UI-visible set (`visible` +
    /// `ui_only`, excludes `internal`).
    pub async fn get_thread_messages(
        &self,
        thread_id: &str,
        limit: Option<u32>,
        offset: Option<u32>,
        include_internal: bool,
    ) -> Result<ThreadMessageListResponse> {
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            if let Some(limit) = limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(offset) = offset {
                query.append_pair("offset", &offset.to_string());
            }
            if include_internal {
                query.append_pair("include_internal", "true");
            }
            query.finish()
        };
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let path = format!(
            "{THREADS_BASE}/{}/messages{suffix}",
            encode_path_segment(thread_id)
        );
        self.get(&path).await
    }

    /// `POST /api/v1/team/ai/threads/:id/chat` (SSE) — send one chat turn
    /// and stream the AI agent's reply to `on_event(event_type, data_json)`.
    /// Unlike goal-chat/master-plan/core-values/todo-chat (which share
    /// `internal/chat/handler`'s `event:`-line SSE contract), this legacy
    /// route's `infra/ai/streaming.SSEWriter` never writes an `event:` line
    /// — every frame is a bare `data: {"type": "...", ...}` payload, so
    /// event dispatch reads the `type` key out of the JSON body itself
    /// (mirrors `stream_goal_decompose`). LLM-billed: exercised only via
    /// `--help`/code wiring in CI, never against production.
    pub async fn stream_thread_chat<F>(
        &self,
        thread_id: &str,
        req: &ThreadChatRequest,
        mut on_event: F,
    ) -> Result<()>
    where
        F: FnMut(&str, &str) -> Result<()>,
    {
        use eventsource_stream::Eventsource;

        let path = format!("{THREADS_BASE}/{}/chat", encode_path_segment(thread_id));
        let response = self.post_stream(&path, req).await?;
        let mut events = response.bytes_stream().eventsource();
        while let Some(event) = events.try_next().await? {
            let event_type = super::goal_decompose::extract_event_type(&event.data);
            on_event(&event_type, &event.data)?;
        }
        Ok(())
    }

    /// `POST /api/v1/team/ai/threads/:id/cancel` — cancel the thread's
    /// currently-running turn, if any.
    pub async fn cancel_thread(&self, thread_id: &str) -> Result<()> {
        let path = format!("{THREADS_BASE}/{}/cancel", encode_path_segment(thread_id));
        self.post_empty_no_content(&path).await
    }

    /// `PUT /api/v1/team/ai/threads/:threadId/messages/:messageId/edit-and-regenerate`
    /// (SSE) — edit a past message, drop everything after it, and stream a
    /// freshly regenerated reply. Same bare `data: {"type": ...}` SSE
    /// contract as `stream_thread_chat`. LLM-billed: never exercised
    /// against production.
    pub async fn stream_thread_edit_and_regenerate<F>(
        &self,
        thread_id: &str,
        message_id: &str,
        req: &MessageEditRequest,
        mut on_event: F,
    ) -> Result<()>
    where
        F: FnMut(&str, &str) -> Result<()>,
    {
        use eventsource_stream::Eventsource;

        let path = format!(
            "{THREADS_BASE}/{}/messages/{}/edit-and-regenerate",
            encode_path_segment(thread_id),
            encode_path_segment(message_id)
        );
        let response = self.put_stream(&path, req).await?;
        let mut events = response.bytes_stream().eventsource();
        while let Some(event) = events.try_next().await? {
            let event_type = super::goal_decompose::extract_event_type(&event.data);
            on_event(&event_type, &event.data)?;
        }
        Ok(())
    }

    /// `GET /api/v1/team/ai/threads/:id/traces` — list the thread's action
    /// traces (tool executions the agent performed, with revert eligibility).
    pub async fn list_thread_traces(
        &self,
        thread_id: &str,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<ActionTraceListResponse> {
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            if let Some(limit) = limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(offset) = offset {
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
            "{THREADS_BASE}/{}/traces{suffix}",
            encode_path_segment(thread_id)
        );
        let resp: ApiResponse<ActionTraceListResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// `POST /api/v1/team/ai/threads/:id/traces/:traceId/revert` — revert
    /// (undo) a previously-executed, revertible action trace.
    pub async fn revert_thread_trace(
        &self,
        thread_id: &str,
        trace_id: &str,
    ) -> Result<ThreadActionResultResponse> {
        let path = format!(
            "{THREADS_BASE}/{}/traces/{}/revert",
            encode_path_segment(thread_id),
            encode_path_segment(trace_id)
        );
        self.post_empty(&path).await
    }

    /// `POST /api/v1/team/ai/threads/:id/share` — create (or refresh) a
    /// public share link for the thread.
    pub async fn create_thread_share_link(
        &self,
        thread_id: &str,
    ) -> Result<ThreadShareLinkResponse> {
        let path = format!("{THREADS_BASE}/{}/share", encode_path_segment(thread_id));
        self.post_empty(&path).await
    }

    /// `DELETE /api/v1/team/ai/threads/:id/share` — revoke the thread's
    /// public share link.
    pub async fn revoke_thread_share_link(&self, thread_id: &str) -> Result<()> {
        let path = format!("{THREADS_BASE}/{}/share", encode_path_segment(thread_id));
        self.delete_no_body(&path).await
    }

    /// `POST /api/v1/team/ai/threads/:id/question/respond` — answer a
    /// pending in-chat question the agent asked (single- or multi-select).
    pub async fn respond_to_thread_question(
        &self,
        thread_id: &str,
        req: &QuestionRespondRequest,
    ) -> Result<ThreadActionResultResponse> {
        let path = format!(
            "{THREADS_BASE}/{}/question/respond",
            encode_path_segment(thread_id)
        );
        self.post(&path, req).await
    }

    /// `POST /api/v1/team/ai/threads/:id/tool-confirmation/respond` —
    /// approve or reject a pending tool-execution confirmation request.
    pub async fn respond_to_thread_tool_confirmation(
        &self,
        thread_id: &str,
        req: &ToolConfirmationRespondRequest,
    ) -> Result<ThreadActionResultResponse> {
        let path = format!(
            "{THREADS_BASE}/{}/tool-confirmation/respond",
            encode_path_segment(thread_id)
        );
        self.post(&path, req).await
    }

    /// `GET /api/v1/team/ai/threads/objective-assignment` — fetch the
    /// thread tying a specific goal to a specific AI member (used to jump
    /// straight to that AI member's conversation about the goal).
    pub async fn get_objective_assignment_thread(
        &self,
        objective_id: &str,
        organization_member_id: &str,
    ) -> Result<ThreadResponse> {
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            query.append_pair("objectiveId", objective_id);
            query.append_pair("organizationMemberId", organization_member_id);
            query.finish()
        };
        let path = format!("{THREADS_BASE}/objective-assignment?{query}");
        self.get(&path).await
    }
}
