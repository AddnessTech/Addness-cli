use std::io::Write;

use anyhow::{Result, bail};
use clap::{Subcommand, ValueEnum};
use colored::Colorize;
use serde_json::Value;

use crate::api::{ApiClient, GoalChatStreamRequest, GoalChatThreadListParams};
use crate::cli::commands::org::resolve_org_id;

/// `reasoning` effort accepted by `POST /api/v2/ai-goal-chat/stream`.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum GoalChatReasoning {
    Low,
    Medium,
    High,
}

impl GoalChatReasoning {
    fn as_str(self) -> &'static str {
        match self {
            GoalChatReasoning::Low => "low",
            GoalChatReasoning::Medium => "medium",
            GoalChatReasoning::High => "high",
        }
    }
}

/// Build a client whose `X-Organization-ID` header targets `org_id`. Mirrors
/// `codex_job::client_for_org` — the `/api/v2/ai-goal-chat/*` routes resolve
/// the organization from that header (no org segment in the path).
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

#[derive(Subcommand)]
pub enum GoalChatCommands {
    /// Send a message to the AI agent chat for a goal and stream its reply
    /// (SSE; ends on the backend's `done` event)
    Send {
        /// Goal ID to chat about
        #[arg(long)]
        goal: String,
        /// Message text to send
        message: String,
        /// Continue an existing thread instead of starting a new one
        #[arg(long)]
        thread: Option<String>,
        /// Reasoning effort
        #[arg(long)]
        reasoning: Option<GoalChatReasoning>,
        /// Re-generate the reply starting from (replacing) this message
        #[arg(long)]
        edit_from_message_id: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Emit each SSE event as a JSON line (`{"event":...,"data":...}`)
        /// instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Get a short AI-generated encouragement message for a goal
    Encouragement {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List your goal-chat conversation threads
    Threads {
        /// Filter to threads under this goal (omit to list across all goals)
        #[arg(long)]
        goal: Option<String>,
        /// Page number, 1-based (default: 1)
        #[arg(long)]
        page: Option<u32>,
        /// Page size (backend default: 20, max: 100)
        #[arg(long)]
        page_size: Option<u32>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List the messages in a goal-chat thread
    Messages {
        /// Thread ID
        thread: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_goal_chat(cmd: &GoalChatCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        GoalChatCommands::Send {
            goal,
            message,
            thread,
            reasoning,
            edit_from_message_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = GoalChatStreamRequest {
                reasoning: reasoning.map(|r| r.as_str().to_string()),
                open_goal_id: goal.clone(),
                thread_id: thread.clone(),
                message: message.clone(),
                // Goal chat always sends `opening: false` — the `true`
                // variant only applies to the sibling todo-chat/core-values/
                // master-plan "start a fresh session" flows.
                opening: false,
                edit_from_message_id: edit_from_message_id.clone(),
            };
            let emit_json = *json;
            let mut stream_error: Option<String> = None;
            scoped
                .stream_goal_chat(&req, |event_type, data| {
                    if emit_json {
                        println!("{}", json_event_line(event_type, data));
                        return Ok(());
                    }
                    render_plain_event(event_type, data, &mut stream_error)
                })
                .await?;
            if let Some(message) = stream_error {
                bail!("goal chat error: {message}");
            }
            Ok(())
        }
        GoalChatCommands::Encouragement { goal, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let message = scoped.get_goal_chat_encouragement(goal).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"message": message}))?
                );
            } else {
                println!("{message}");
            }
            Ok(())
        }
        GoalChatCommands::Threads {
            goal,
            page,
            page_size,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let current_page = page.unwrap_or(1);
            let params = GoalChatThreadListParams {
                goal_id: goal.as_deref(),
                page: current_page,
                page_size: *page_size,
            };
            let data = scoped.list_goal_chat_threads(params).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else if data.threads.is_empty() {
                println!("{}", "No goal-chat threads found.".dimmed());
            } else {
                for thread in &data.threads {
                    let last = thread.last_message_at.as_deref().unwrap_or("-");
                    let goal_title = thread.goal_title.as_deref().unwrap_or("-");
                    println!(
                        "{}  {}  goal={goal_title} last={last}",
                        thread.id, thread.title
                    );
                }
                if data.meta.more {
                    println!(
                        "{}",
                        format!(
                            "... {} more (use --page {})",
                            data.meta.remaining_count,
                            current_page + 1
                        )
                        .dimmed()
                    );
                }
            }
            Ok(())
        }
        GoalChatCommands::Messages { thread, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let messages = scoped.list_goal_chat_messages(thread).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&messages)?);
            } else if messages.is_empty() {
                println!("{}", "No messages found.".dimmed());
            } else {
                for message in &messages {
                    println!("[{}] {}", message.role, message.content);
                }
            }
            Ok(())
        }
    }
}

/// Render a single SSE event as a `{"event":...,"data":...}` JSON line
/// (used by `--json`). Falls back to a null `data` if the payload isn't
/// valid JSON, which should not happen in practice.
fn json_event_line(event_type: &str, data: &str) -> String {
    let parsed: Value = serde_json::from_str(data).unwrap_or(Value::Null);
    serde_json::json!({"event": event_type, "data": parsed}).to_string()
}

/// Render one SSE event as human-readable terminal output, streaming
/// `text_delta`/`reasoning_delta` chunks without trailing newlines. `error`
/// events are captured into `stream_error` instead of printed immediately,
/// so the caller can surface them as a command failure once the stream ends.
fn render_plain_event(
    event_type: &str,
    data: &str,
    stream_error: &mut Option<String>,
) -> Result<()> {
    match event_type {
        "goal" => println!("{}", format_goal_line(data).bold()),
        "thread" => println!("{}", format_thread_line(data).dimmed()),
        "reasoning_delta" => print_flushed(&extract_delta(data).dimmed().to_string()),
        "text_delta" => print_flushed(&extract_delta(data)),
        "tool_call" => println!("\n{}", format_tool_call_line(data)),
        "tool_result" => println!("{}", format_tool_result_line(data)),
        "usage" => println!("\n{}", format_usage_line(data).dimmed()),
        "message_saved" => {}
        "done" => println!(),
        "error" => *stream_error = Some(extract_error_message(data)),
        other => println!("[{other}] {data}"),
    }
    Ok(())
}

fn print_flushed(text: &str) {
    print!("{text}");
    let _ = std::io::stdout().flush();
}

fn format_goal_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[goal] {data}");
    };
    let id = value.get("id").and_then(Value::as_str).unwrap_or("");
    let title = value.get("title").and_then(Value::as_str).unwrap_or("");
    format!("Goal: {title} ({id})")
}

fn format_thread_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[thread] {data}");
    };
    let thread_id = value.get("threadId").and_then(Value::as_str).unwrap_or("");
    format!("Thread: {thread_id}")
}

fn extract_delta(data: &str) -> String {
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

fn format_tool_call_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[tool_call] {data}");
    };
    let name = value.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = value.get("arguments").cloned().unwrap_or(Value::Null);
    format!("[tool_call] {name} {arguments}")
}

fn format_tool_result_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[tool_result] {data}");
    };
    let name = value.get("name").and_then(Value::as_str).unwrap_or("");
    let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let result_data = value.get("data").cloned().unwrap_or(Value::Null);
    format!("[tool_result] {name} ok={ok} {result_data}")
}

fn format_usage_line(data: &str) -> String {
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

fn extract_error_message(data: &str) -> String {
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
        extract_delta, extract_error_message, format_goal_line, format_thread_line,
        format_tool_call_line, format_tool_result_line, format_usage_line, json_event_line,
    };

    #[test]
    fn format_goal_line_reads_id_and_title() {
        assert_eq!(
            format_goal_line(r#"{"id":"g-1","title":"Ship the feature"}"#),
            "Goal: Ship the feature (g-1)"
        );
    }

    #[test]
    fn format_goal_line_falls_back_on_invalid_json() {
        assert_eq!(format_goal_line("not json"), "[goal] not json");
    }

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
