//! codex CLI（`codex exec --json`）を駆動する backend。
//!
//! CLI 引数生成（`codex_exec_args`）とヘルプ本文（`slash_help_text`）・設定型
//! （`CodexExecSettings`）は現状 `codex_pane` モジュールに残しており、ここでは
//! それらへ委譲する薄い実装のみを持つ。codex 固有の引数ビルダ群・設定 enum は
//! 後続で本モジュールへ移設していく。

use serde_json::Value;

use super::{
    AgentBackend, AgentEvent, PromptDelivery, first_text_field, nested_error_message, string_at_any,
};
use crate::tui::codex_pane::{CodexExecSettings, codex_exec_args, slash_help_text};

/// codex CLI（`codex exec --json`）を駆動する backend。
pub(crate) struct CodexBackend;

impl AgentBackend for CodexBackend {
    type Settings = CodexExecSettings;

    fn turn_args(
        &self,
        session: Option<&str>,
        cwd: &str,
        settings: &Self::Settings,
    ) -> Vec<String> {
        codex_exec_args(session, cwd, settings)
    }

    fn prompt_delivery(&self) -> PromptDelivery {
        // codex exec は stdin から読み、引数末尾の `-` と対になる。
        PromptDelivery::Stdin
    }

    fn session_id_from_event(&self, value: &Value) -> Option<String> {
        string_at_any(value, &["thread_id", "threadId", "id"])
    }

    fn parse_lifecycle_event(&self, value: &Value) -> AgentEvent {
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("event");
        match event_type {
            "thread.started" => AgentEvent::SessionStarted(self.session_id_from_event(value)),
            "turn.started" => AgentEvent::TurnStarted,
            "turn.completed" | "turn.finished" => AgentEvent::TurnCompleted,
            "turn.failed" => {
                let message = nested_error_message(value)
                    .or_else(|| first_text_field(value))
                    .unwrap_or_else(|| "Codex ターンが失敗しました".to_string());
                AgentEvent::TurnFailed(message)
            }
            "error" => {
                let message = nested_error_message(value)
                    .or_else(|| first_text_field(value))
                    .unwrap_or_else(|| "Codex エラー".to_string());
                AgentEvent::Error(message)
            }
            _ => AgentEvent::Other,
        }
    }

    fn help_text(&self) -> &'static str {
        slash_help_text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_backend_normalizes_lifecycle_events() {
        let backend = CodexBackend;

        let started = serde_json::json!({"type": "thread.started", "thread_id": "abc123"});
        assert!(matches!(
            backend.parse_lifecycle_event(&started),
            AgentEvent::SessionStarted(Some(id)) if id == "abc123"
        ));
        assert_eq!(
            backend.session_id_from_event(&started).as_deref(),
            Some("abc123")
        );

        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "turn.started"})),
            AgentEvent::TurnStarted
        ));
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "turn.completed"})),
            AgentEvent::TurnCompleted
        ));
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "turn.finished"})),
            AgentEvent::TurnCompleted
        ));
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "turn.failed"})),
            AgentEvent::TurnFailed(_)
        ));
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "error"})),
            AgentEvent::Error(_)
        ));
        // ツール/本文など表層イベントは Other に集約される。
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "item.completed"})),
            AgentEvent::Other
        ));
    }
}
