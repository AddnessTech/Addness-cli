//! SSE event rendering for `addness goal decompose`
//! (`POST /api/v1/objectives/:id/decompose`).
//!
//! This endpoint's event vocabulary and wire format are unrelated to the
//! `goal-chat`/`todo-chat`/`core-values`/`master-plan` family rendered by
//! `ai_chat_render`: it's the legacy V1 graph-run agent loop
//! (`pkg/ai/streaming.EventType`, ~30 kinds — `agent-start`, `tool-start`,
//! `text-delta`, `plan-created`, `objective-active`, `finish`, `error`, ...)
//! and every frame is dispatched by the `type` key inside the JSON `data`
//! payload (see `crate::api::client::goal_decompose`), not by an SSE
//! `event:` line. A handful of "structured" event types
//! (`infra/ai/streaming/sse_writer.go`'s `structuredEvents` set —
//! `resource-updated`, `agent-end`, `goal-proposal-created`,
//! `objective-active`, `objective-completed`) additionally nest their
//! payload one level deeper, under a `data` key, alongside `index`/
//! `timestamp` bookkeeping fields; the rest are flattened directly onto the
//! top-level object.
//!
//! This renders the kinds most useful to follow along in a terminal
//! (streamed text, tool calls, plan steps, per-goal progress, terminal
//! success/error) and falls back to a generic `[type] data` line for the
//! rest, matching the fallback convention used by `ai_chat_render`.

use colored::Colorize;
use serde_json::Value;

use crate::cli::commands::ai_chat_render::extract_delta;

/// Render one `addness goal decompose` SSE event as human-readable terminal
/// output. `error` events are captured into `stream_error` instead of
/// printed immediately, so the caller can surface them as a command failure
/// once the stream ends (matching the chat-family commands' behavior).
pub fn render_plain_event(event_type: &str, data: &str, stream_error: &mut Option<String>) {
    match event_type {
        "text-delta" | "reasoning-delta" => render_delta(data),
        "tool-start" => println!("\n{}", format_tool_start_line(data)),
        "tool-result" => println!("{}", format_tool_result_line(data)),
        "tool-error" => println!("{}", format_tool_error_line(data).red()),
        "plan-created" => println!("{}", format_plan_created_line(data).bold()),
        "plan-step-started" => println!("{}", format_plan_step_started_line(data)),
        "plan-step-completed" => println!("{}", format_plan_step_completed_line(data)),
        "objective-active" => println!("{}", format_objective_line(event_type, data).dimmed()),
        "objective-completed" => println!("{}", format_objective_line(event_type, data).green()),
        "keep-alive" => {}
        "finish" => println!(),
        "error" => *stream_error = Some(extract_decompose_error_message(data)),
        other => println!("[{other}] {data}"),
    }
}

fn render_delta(data: &str) {
    crate::cli::commands::ai_chat_render::print_flushed(&extract_delta(data));
}

fn format_tool_start_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[tool-start] {data}");
    };
    let tool = value.get("tool").and_then(Value::as_str).unwrap_or("");
    let arguments = value.get("arguments").cloned().unwrap_or(Value::Null);
    format!("[tool-start] {tool} {arguments}")
}

fn format_tool_result_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[tool-result] {data}");
    };
    let tool = value.get("tool").and_then(Value::as_str).unwrap_or("");
    let success = value
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let summary = value.get("summary").and_then(Value::as_str).unwrap_or("");
    format!("[tool-result] {tool} success={success} {summary}")
}

fn format_tool_error_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[tool-error] {data}");
    };
    let tool = value.get("tool").and_then(Value::as_str).unwrap_or("");
    let error = value.get("error").and_then(Value::as_str).unwrap_or("");
    format!("[tool-error] {tool}: {error}")
}

fn format_plan_created_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[plan-created] {data}");
    };
    let step_count = value
        .get("steps")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    format!("Plan created: {step_count} step(s)")
}

fn format_plan_step_started_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[plan-step-started] {data}");
    };
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    format!("  -> {description}")
}

fn format_plan_step_completed_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[plan-step-completed] {data}");
    };
    match value.get("result").and_then(Value::as_str) {
        Some(result) if !result.is_empty() => format!("  done: {result}"),
        _ => "  done".to_string(),
    }
}

/// `objective-active`/`objective-completed` are "structured" events: their
/// payload sits one level deeper under a `data` key
/// (`infra/ai/streaming/sse_writer.go`'s `structuredEvents` set), alongside
/// `index`/`timestamp` bookkeeping this CLI doesn't surface.
fn format_objective_line(event_type: &str, data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[{event_type}] {data}");
    };
    let inner = value.get("data").unwrap_or(&value);
    let objective_id = inner
        .get("objective_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    match event_type {
        "objective-active" => {
            let tool = inner.get("tool").and_then(Value::as_str).unwrap_or("");
            format!("[objective-active] {objective_id} tool={tool}")
        }
        _ => format!("[objective-completed] {objective_id}"),
    }
}

/// Decompose's `error` events flatten to `{"type":"error","error":"..."}`
/// (`application/usecases/ai/thread_chat_usecase.go`), unlike the chat
/// family's `{"message":"...","code":"..."}` shape — hence a dedicated
/// extractor rather than reusing `ai_chat_render::extract_error_message`.
fn extract_decompose_error_message(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return data.to_string();
    };
    value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("unknown error")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        extract_decompose_error_message, format_objective_line, format_plan_created_line,
        format_plan_step_completed_line, format_plan_step_started_line, format_tool_error_line,
        format_tool_result_line, format_tool_start_line,
    };

    #[test]
    fn format_tool_start_line_includes_tool_and_arguments() {
        assert_eq!(
            format_tool_start_line(r#"{"tool":"search","arguments":{"q":"x"}}"#),
            r#"[tool-start] search {"q":"x"}"#
        );
    }

    #[test]
    fn format_tool_result_line_includes_success_and_summary() {
        assert_eq!(
            format_tool_result_line(
                r#"{"tool":"search","result":{},"summary":"3 hits","success":true,"latency_ms":120}"#
            ),
            "[tool-result] search success=true 3 hits"
        );
    }

    #[test]
    fn format_tool_error_line_includes_tool_and_error() {
        assert_eq!(
            format_tool_error_line(r#"{"tool":"search","error":"timeout"}"#),
            "[tool-error] search: timeout"
        );
    }

    #[test]
    fn format_plan_created_line_counts_steps() {
        assert_eq!(
            format_plan_created_line(r#"{"plan_id":"p1","steps":[{"id":"1"},{"id":"2"}]}"#),
            "Plan created: 2 step(s)"
        );
    }

    #[test]
    fn format_plan_step_started_line_reads_description() {
        assert_eq!(
            format_plan_step_started_line(
                r#"{"plan_id":"p1","step_id":"s1","description":"Split into phases"}"#
            ),
            "  -> Split into phases"
        );
    }

    #[test]
    fn format_plan_step_completed_line_includes_result_when_present() {
        assert_eq!(
            format_plan_step_completed_line(
                r#"{"plan_id":"p1","step_id":"s1","result":"3 sub-goals created"}"#
            ),
            "  done: 3 sub-goals created"
        );
    }

    #[test]
    fn format_plan_step_completed_line_falls_back_without_result() {
        assert_eq!(
            format_plan_step_completed_line(r#"{"plan_id":"p1","step_id":"s1"}"#),
            "  done"
        );
    }

    #[test]
    fn format_objective_line_reads_structured_nested_data() {
        assert_eq!(
            format_objective_line(
                "objective-active",
                r#"{"type":"objective-active","data":{"objective_id":"o1","tool":"create_objective"},"index":3,"timestamp":"2026-07-16T00:00:00.000Z"}"#
            ),
            "[objective-active] o1 tool=create_objective"
        );
        assert_eq!(
            format_objective_line(
                "objective-completed",
                r#"{"type":"objective-completed","data":{"objective_id":"o1"},"index":4,"timestamp":"2026-07-16T00:00:00.000Z"}"#
            ),
            "[objective-completed] o1"
        );
    }

    #[test]
    fn extract_decompose_error_message_reads_error_field() {
        assert_eq!(
            extract_decompose_error_message(r#"{"type":"error","error":"billing exhausted"}"#),
            "billing exhausted"
        );
    }

    #[test]
    fn extract_decompose_error_message_falls_back_on_invalid_json() {
        assert_eq!(extract_decompose_error_message("not json"), "not json");
    }
}
