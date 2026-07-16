use anyhow::{Result, bail};
use clap::{Subcommand, ValueEnum};
use colored::Colorize;
use serde_json::Value;

use crate::api::{ApiClient, GoalChatStreamRequest, GoalChatThreadListParams};
use crate::cli::commands::ai_chat_render::{
    extract_error_message, format_thread_line, format_tool_call_line, format_tool_result_line,
    format_usage_line, json_event_line, render_delta,
};
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
        "reasoning_delta" => render_delta(data, true),
        "text_delta" => render_delta(data, false),
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

/// `goal` SSE events are exclusive to goal-chat (the target goal it's
/// scoped to), so this formatter stays local rather than moving to the
/// shared `ai_chat_render` module.
fn format_goal_line(data: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return format!("[goal] {data}");
    };
    let id = value.get("id").and_then(Value::as_str).unwrap_or("");
    let title = value.get("title").and_then(Value::as_str).unwrap_or("");
    format!("Goal: {title} ({id})")
}

#[cfg(test)]
mod tests {
    use super::format_goal_line;

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
}
