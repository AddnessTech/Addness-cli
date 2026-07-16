use anyhow::Result;
use futures::TryStreamExt;

use super::issue::encode_path_segment;
use crate::api::{ApiClient, ApiResponse, TodoChatMessage, TodoChatStreamRequest, TodoChatThread};

impl ApiClient {
    /// POST /api/v2/ai-todo-chat/stream (SSE)
    ///
    /// Sends one chat turn (or, with `req.opening == true`, fires the
    /// silent "opening" turn for a fresh thread) and streams the AI agent's
    /// reply to `on_event(event_type, data_json)` until the backend emits
    /// `done` (success) or `error` (failure; the caller decides whether to
    /// surface it). Event types: `thread`, `reasoning_delta`, `text_delta`,
    /// `tool_call`, `tool_result`, `usage`, `message_saved`, `done`,
    /// `error`. Unlike goal-chat, no `goal` event is emitted since todo-chat
    /// isn't scoped to a single goal.
    pub async fn stream_todo_chat<F>(
        &self,
        req: &TodoChatStreamRequest,
        mut on_event: F,
    ) -> Result<()>
    where
        F: FnMut(&str, &str) -> Result<()>,
    {
        use eventsource_stream::Eventsource;

        let response = self.post_stream("/api/v2/ai-todo-chat/stream", req).await?;
        let mut events = response.bytes_stream().eventsource();
        while let Some(event) = events.try_next().await? {
            on_event(&event.event, &event.data)?;
        }
        Ok(())
    }

    /// GET /api/v2/ai-todo-chat/threads
    ///
    /// Unlike goal-chat, the backend's todo-chat agent doesn't implement
    /// pagination (`runtime.ThreadPageLister`), so this always returns the
    /// full list of threads as a plain array (`{"data": [...]}`), confirmed
    /// against production — there is no `page`/`pageSize`/meta envelope to
    /// request here.
    pub async fn list_todo_chat_threads(&self) -> Result<Vec<TodoChatThread>> {
        let resp: ApiResponse<Vec<TodoChatThread>> =
            self.get("/api/v2/ai-todo-chat/threads").await?;
        Ok(resp.data)
    }

    /// GET /api/v2/ai-todo-chat/threads/:threadId/messages
    pub async fn list_todo_chat_messages(&self, thread_id: &str) -> Result<Vec<TodoChatMessage>> {
        let path = format!(
            "/api/v2/ai-todo-chat/threads/{}/messages",
            encode_path_segment(thread_id)
        );
        let resp: ApiResponse<Vec<TodoChatMessage>> = self.get(&path).await?;
        Ok(resp.data)
    }
}
