//! Shared SSE event rendering helpers for the AI agent chat family
//! (`goal-chat` / `todo-chat` / future `core-values` / `master-plan`), which
//! all stream the same event set from `internal/chat/handler` on the
//! backend: `thread`, `reasoning_delta`, `text_delta`, `tool_call`,
//! `tool_result`, `usage`, `message_saved`, `done`, `error`. Only `goal-chat`
//! additionally emits a `goal` event, which stays command-specific.

use std::io::Write;

use colored::Colorize;
use serde_json::Value;

/// Render a single SSE event as a `{"event":...,"data":...}` JSON line
/// (used by `--json`). Falls back to a null `data` if the payload isn't
/// valid JSON, which should not happen in practice.
pub fn json_event_line(event_type: &str, data: &str) -> String {
    let parsed: Value = serde_json::from_str(data).unwrap_or(Value::Null);
    serde_json::json!({"event": event_type, "data": parsed}).to_string()
}

pub fn print_flushed(text: &str) {
    print!("{text}");
    let _ = std::io::stdout().flush();
}

pub fn format_thread_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[thread] {data}");
    };
    let thread_id = value.get("threadId").and_then(Value::as_str).unwrap_or("");
    format!("Thread: {thread_id}")
}

pub fn extract_delta(data: &str) -> String {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| {
            value
                .get("delta")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

pub fn render_delta(data: &str, dim: bool) {
    let text = extract_delta(data);
    if dim {
        print_flushed(&text.dimmed().to_string());
    } else {
        print_flushed(&text);
    }
}

pub fn format_tool_call_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[tool_call] {data}");
    };
    let name = value.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = value.get("arguments").cloned().unwrap_or(Value::Null);
    format!("[tool_call] {name} {arguments}")
}

pub fn format_tool_result_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[tool_result] {data}");
    };
    let name = value.get("name").and_then(Value::as_str).unwrap_or("");
    let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let result_data = value.get("data").cloned().unwrap_or(Value::Null);
    format!("[tool_result] {name} ok={ok} {result_data}")
}

pub fn format_usage_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[usage] {data}");
    };
    let input = value
        .get("inputTokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cached = value
        .get("cachedInputTokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output = value
        .get("outputTokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = value
        .get("totalTokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let provider = value.get("provider").and_then(Value::as_str).unwrap_or("");
    let model = value.get("model").and_then(Value::as_str).unwrap_or("");
    format!(
        "usage: input={input} cached={cached} output={output} total={total} ({provider}/{model})"
    )
}

pub fn extract_error_message(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return data.to_string();
    };
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    match value.get("code").and_then(Value::as_str) {
        Some(code) => format!("{message} ({code})"),
        None => message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_delta, extract_error_message, format_thread_line, format_tool_call_line,
        format_tool_result_line, format_usage_line, json_event_line,
    };

    #[test]
    fn format_thread_line_reads_thread_id() {
        assert_eq!(format_thread_line(r#"{"threadId":"t-1"}"#), "Thread: t-1");
    }

    #[test]
    fn extract_delta_reads_delta_field() {
        assert_eq!(extract_delta(r#"{"delta":"hello"}"#), "hello");
    }

    #[test]
    fn extract_delta_defaults_to_empty_when_missing() {
        assert_eq!(extract_delta(r#"{"round":1}"#), "");
    }

    #[test]
    fn format_tool_call_line_includes_name_and_arguments() {
        assert_eq!(
            format_tool_call_line(r#"{"name":"search","arguments":{"q":"x"}}"#),
            r#"[tool_call] search {"q":"x"}"#
        );
    }

    #[test]
    fn format_tool_result_line_includes_ok_and_data() {
        assert_eq!(
            format_tool_result_line(r#"{"name":"search","ok":true,"data":{"count":3}}"#),
            r#"[tool_result] search ok=true {"count":3}"#
        );
    }

    #[test]
    fn format_usage_line_includes_token_counts() {
        assert_eq!(
            format_usage_line(
                r#"{"inputTokens":10,"cachedInputTokens":2,"outputTokens":5,"totalTokens":15,"provider":"anthropic","model":"claude"}"#
            ),
            "usage: input=10 cached=2 output=5 total=15 (anthropic/claude)"
        );
    }

    #[test]
    fn extract_error_message_includes_code_when_present() {
        assert_eq!(
            extract_error_message(r#"{"message":"out of tokens","code":"token_exhausted"}"#),
            "out of tokens (token_exhausted)"
        );
    }

    #[test]
    fn extract_error_message_omits_code_when_absent() {
        assert_eq!(extract_error_message(r#"{"message":"boom"}"#), "boom");
    }

    #[test]
    fn json_event_line_wraps_event_and_data() {
        assert_eq!(
            json_event_line("text_delta", r#"{"delta":"hi"}"#),
            r#"{"data":{"delta":"hi"},"event":"text_delta"}"#
        );
    }
}
