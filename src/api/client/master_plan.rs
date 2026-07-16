use anyhow::Result;
use futures::TryStreamExt;

use super::issue::encode_path_segment;
use crate::api::{
    ApiClient, ApiResponse, MasterPlanChatStreamRequest, MasterPlanMessage, MasterPlanThread,
};

impl ApiClient {
    /// POST /api/v2/ai-master-plan/stream (SSE)
    ///
    /// Sends one chat turn (or, with `req.opening == true`, fires the
    /// silent "opening" turn for a fresh thread) and streams the AI agent's
    /// reply to `on_event(event_type, data_json)` until the backend emits
    /// `done` (success) or `error` (failure; the caller decides whether to
    /// surface it). Event types: `thread`, `reasoning_delta`, `text_delta`,
    /// `tool_call`, `tool_result`, `usage`, `message_saved`, `done`,
    /// `error`. Unlike goal-chat, no `goal` event is emitted since
    /// master-plan isn't scoped to a single goal. Mirrors
    /// `stream_core_values_chat` — same generic handler, same event
    /// contract.
    pub async fn stream_master_plan_chat<F>(
        &self,
        req: &MasterPlanChatStreamRequest,
        mut on_event: F,
    ) -> Result<()>
    where
        F: FnMut(&str, &str) -> Result<()>,
    {
        use eventsource_stream::Eventsource;

        let response = self
            .post_stream("/api/v2/ai-master-plan/stream", req)
            .await?;
        let mut events = response.bytes_stream().eventsource();
        while let Some(event) = events.try_next().await? {
            on_event(&event.event, &event.data)?;
        }
        Ok(())
    }

    /// GET /api/v2/ai-master-plan/threads
    ///
    /// Like core-values/todo-chat, the backend's master-plan agent
    /// (`internal/aimasterplan/chat.RuntimeAgent`) doesn't implement
    /// pagination (`runtime.ThreadPageLister`), so this always returns the
    /// full list of threads as a plain array (`{"data": [...]}`) — confirmed
    /// against the Go source, not just production behavior.
    pub async fn list_master_plan_threads(&self) -> Result<Vec<MasterPlanThread>> {
        let resp: ApiResponse<Vec<MasterPlanThread>> =
            self.get("/api/v2/ai-master-plan/threads").await?;
        Ok(resp.data)
    }

    /// GET /api/v2/ai-master-plan/threads/:threadId/messages
    pub async fn list_master_plan_messages(
        &self,
        thread_id: &str,
    ) -> Result<Vec<MasterPlanMessage>> {
        let path = format!(
            "/api/v2/ai-master-plan/threads/{}/messages",
            encode_path_segment(thread_id)
        );
        let resp: ApiResponse<Vec<MasterPlanMessage>> = self.get(&path).await?;
        Ok(resp.data)
    }
}
