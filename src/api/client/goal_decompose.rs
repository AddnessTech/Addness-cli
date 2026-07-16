use anyhow::Result;
use futures::TryStreamExt;
use serde_json::Value;

use crate::api::ApiClient;

/// POST /api/v1/objectives/:id/decompose (SSE)
///
/// Legacy V1 endpoint ("AI目標分解（SSE）", `presentation/routes/api.go`)
/// backed by `GoalDecomposeHandler`/`GoalDecomposeUsecase`
/// (`presentation/handlers/ai`, `application/usecases/ai` — both headed
/// "Deprecated. Use internal/aigoalchat / internal/aitodochat for new
/// implementations." in the Go source, but still the only user-triggered
/// "decompose this goal into sub-goals" route; it remains live and is what
/// the frontend calls). Unlike goal-chat/todo-chat/core-values/master-plan
/// (which share `internal/chat/handler`'s generic SSE contract keyed off the
/// HTTP `event:` line), this is a single-shot, non-chat generation call: no
/// request body, no thread/message history to browse afterward, and the
/// underlying `infra/ai/streaming.SSEWriter` never writes an `event:` line —
/// every frame is a bare `data: {"type": "...", ...}` payload (confirmed
/// against `infra/ai/streaming/sse_writer.go`). So event dispatch here reads
/// the `type` key out of the JSON body itself rather than the SSE event
/// name (which `eventsource_stream` reports as the spec default,
/// `"message"`, since none was sent).
///
/// Event vocabulary (`pkg/ai/streaming/event.go`, `EventType` consts) is the
/// full graph-run agent-loop set (`agent-start`/`tool-start`/`text-delta`/
/// `plan-created`/`objective-active`/`finish`/`error`/... — ~30 kinds); see
/// `crate::cli::commands::goal_decompose_render` for which ones this CLI
/// renders specially versus falls back to a generic `[type] data` line for.
impl ApiClient {
    pub async fn stream_goal_decompose<F>(&self, goal_id: &str, mut on_event: F) -> Result<()>
    where
        F: FnMut(&str, &str) -> Result<()>,
    {
        use eventsource_stream::Eventsource;

        let path = format!("/api/v1/objectives/{goal_id}/decompose");
        let response = self.post_stream(&path, &serde_json::json!({})).await?;
        let mut events = response.bytes_stream().eventsource();
        while let Some(event) = events.try_next().await? {
            let event_type = extract_event_type(&event.data);
            on_event(&event_type, &event.data)?;
        }
        Ok(())
    }
}

/// Reads the `type` field out of a decode SSE frame's JSON `data` payload.
/// Falls back to `"unknown"` if the payload isn't a JSON object or lacks a
/// string `type` field, which should not happen in practice.
fn extract_event_type(data: &str) -> String {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::extract_event_type;

    #[test]
    fn extract_event_type_reads_type_field() {
        assert_eq!(
            extract_event_type(r#"{"type":"text-delta","delta":"hi"}"#),
            "text-delta"
        );
    }

    #[test]
    fn extract_event_type_falls_back_on_missing_field() {
        assert_eq!(extract_event_type(r#"{"delta":"hi"}"#), "unknown");
    }

    #[test]
    fn extract_event_type_falls_back_on_invalid_json() {
        assert_eq!(extract_event_type("not json"), "unknown");
    }
}
