use anyhow::Result;
use futures::TryStreamExt;

use super::issue::encode_path_segment;
use crate::api::{
    ApiClient, ApiResponse, GoalChatEncouragement, GoalChatMessage, GoalChatStreamRequest,
    GoalChatThreadsData,
};

/// Query parameters for `GET /api/v2/ai-goal-chat/threads`. `page` is always
/// sent (defaulting to 1 at the call site) so the backend always returns the
/// paginated envelope (`{"data":{"threads":[...],"meta":{...}}}`) rather than
/// the legacy bare-array form it falls back to when `page` is omitted.
#[derive(Default)]
pub struct GoalChatThreadListParams<'a> {
    /// Filter to threads under a specific goal. Omit to list across all of
    /// the caller's goals.
    pub goal_id: Option<&'a str>,
    pub page: u32,
    pub page_size: Option<u32>,
}

fn goal_chat_thread_list_query_suffix(params: &GoalChatThreadListParams<'_>) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    query.append_pair("page", &params.page.to_string());
    if let Some(page_size) = params.page_size {
        query.append_pair("pageSize", &page_size.to_string());
    }
    if let Some(goal_id) = params.goal_id {
        query.append_pair("objectiveId", goal_id);
    }
    format!("?{}", query.finish())
}

impl ApiClient {
    /// POST /api/v2/ai-goal-chat/stream (SSE)
    ///
    /// Sends one chat turn and streams the AI agent's reply to
    /// `on_event(event_type, data_json)` until the backend emits `done`
    /// (success) or `error` (failure; the caller decides whether to
    /// surface it). Event types: `goal`, `thread`, `reasoning_delta`,
    /// `text_delta`, `tool_call`, `tool_result`, `usage`, `message_saved`,
    /// `done`, `error`.
    pub async fn stream_goal_chat<F>(
        &self,
        req: &GoalChatStreamRequest,
        mut on_event: F,
    ) -> Result<()>
    where
        F: FnMut(&str, &str) -> Result<()>,
    {
        use eventsource_stream::Eventsource;

        let response = self.post_stream("/api/v2/ai-goal-chat/stream", req).await?;
        let mut events = response.bytes_stream().eventsource();
        while let Some(event) = events.try_next().await? {
            on_event(&event.event, &event.data)?;
        }
        Ok(())
    }

    /// GET /api/v2/ai-goal-chat/encouragement?goalId=
    ///
    /// Returns a short (<=40 char) AI-generated encouragement message for
    /// the given goal. Plain JSON, not SSE.
    pub async fn get_goal_chat_encouragement(&self, goal_id: &str) -> Result<String> {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("goalId", goal_id)
            .finish();
        let path = format!("/api/v2/ai-goal-chat/encouragement?{query}");
        let resp: ApiResponse<GoalChatEncouragement> = self.get(&path).await?;
        Ok(resp.data.message)
    }

    /// GET /api/v2/ai-goal-chat/threads?objectiveId=&page=&pageSize=
    pub async fn list_goal_chat_threads(
        &self,
        params: GoalChatThreadListParams<'_>,
    ) -> Result<GoalChatThreadsData> {
        let suffix = goal_chat_thread_list_query_suffix(&params);
        let path = format!("/api/v2/ai-goal-chat/threads{suffix}");
        let resp: ApiResponse<GoalChatThreadsData> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/ai-goal-chat/threads/:threadId/messages
    pub async fn list_goal_chat_messages(&self, thread_id: &str) -> Result<Vec<GoalChatMessage>> {
        let path = format!(
            "/api/v2/ai-goal-chat/threads/{}/messages",
            encode_path_segment(thread_id)
        );
        let resp: ApiResponse<Vec<GoalChatMessage>> = self.get(&path).await?;
        Ok(resp.data)
    }
}

#[cfg(test)]
mod tests {
    use super::{GoalChatThreadListParams, goal_chat_thread_list_query_suffix};

    #[test]
    fn thread_list_query_always_includes_page() {
        let suffix = goal_chat_thread_list_query_suffix(&GoalChatThreadListParams {
            goal_id: None,
            page: 1,
            page_size: None,
        });
        assert_eq!(suffix, "?page=1");
    }

    #[test]
    fn thread_list_query_encodes_all_params() {
        let suffix = goal_chat_thread_list_query_suffix(&GoalChatThreadListParams {
            goal_id: Some("goal-1"),
            page: 2,
            page_size: Some(50),
        });
        assert_eq!(suffix, "?page=2&pageSize=50&objectiveId=goal-1");
    }
}
