use anyhow::{Result, bail};
use clap::{Subcommand, ValueEnum};
use colored::Colorize;

use crate::api::{ApiClient, MasterPlanChatStreamRequest};
use crate::cli::commands::ai_chat_render::{
    extract_error_message, format_thread_line, format_tool_call_line, format_tool_result_line,
    format_usage_line, json_event_line, render_delta,
};
use crate::cli::commands::org::resolve_org_id;

/// `reasoning` effort accepted by `POST /api/v2/ai-master-plan/stream`.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum MasterPlanReasoning {
    Low,
    Medium,
    High,
}

impl MasterPlanReasoning {
    fn as_str(self) -> &'static str {
        match self {
            MasterPlanReasoning::Low => "low",
            MasterPlanReasoning::Medium => "medium",
            MasterPlanReasoning::High => "high",
        }
    }
}

/// Build a client whose `X-Organization-ID` header targets `org_id`. Mirrors
/// `core_values::client_for_org` — the `/api/v2/ai-master-plan/*` routes
/// resolve the organization from that header (no org segment in the path).
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

#[derive(Subcommand)]
pub enum MasterPlanCommands {
    /// Send a message to the AI master-plan chat and stream its reply (SSE;
    /// ends on the backend's `done` event)
    Send {
        /// Message text to send
        message: String,
        /// Continue an existing thread instead of starting a new one
        #[arg(long)]
        thread: Option<String>,
        /// Reasoning effort
        #[arg(long)]
        reasoning: Option<MasterPlanReasoning>,
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
    /// Fire the silent "opening" turn for a brand-new thread (the panel's
    /// auto-start, as if the user just opened it without typing anything)
    /// and stream the AI agent's reply
    Start {
        /// Reasoning effort
        #[arg(long)]
        reasoning: Option<MasterPlanReasoning>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Emit each SSE event as a JSON line (`{"event":...,"data":...}`)
        /// instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// List your master-plan conversation threads (the backend doesn't
    /// paginate this list, so it's always the full set)
    Threads {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List the messages in a master-plan thread
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

pub async fn handle_master_plan(cmd: &MasterPlanCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        MasterPlanCommands::Send {
            message,
            thread,
            reasoning,
            edit_from_message_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = MasterPlanChatStreamRequest {
                reasoning: reasoning.map(|r| r.as_str().to_string()),
                thread_id: thread.clone(),
                message: message.clone(),
                opening: false,
                edit_from_message_id: edit_from_message_id.clone(),
            };
            run_stream(&scoped, &req, *json).await
        }
        MasterPlanCommands::Start {
            reasoning,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = MasterPlanChatStreamRequest {
                reasoning: reasoning.map(|r| r.as_str().to_string()),
                // Opening is new-thread-only and carries no user message.
                thread_id: None,
                message: String::new(),
                opening: true,
                edit_from_message_id: None,
            };
            run_stream(&scoped, &req, *json).await
        }
        MasterPlanCommands::Threads { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let threads = scoped.list_master_plan_threads().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&threads)?);
            } else if threads.is_empty() {
                println!("{}", "No master-plan threads found.".dimmed());
            } else {
                for thread in &threads {
                    let last = thread.last_message_at.as_deref().unwrap_or("-");
                    println!("{}  {}  last={last}", thread.id, thread.title);
                }
            }
            Ok(())
        }
        MasterPlanCommands::Messages { thread, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let messages = scoped.list_master_plan_messages(thread).await?;
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

/// Shared SSE-streaming driver for `Send` and `Start`: streams the turn and
/// turns a terminal `error` event into a command failure once the stream
/// ends (matching goal-chat/todo-chat/core-values' behavior).
async fn run_stream(
    client: &ApiClient,
    req: &MasterPlanChatStreamRequest,
    emit_json: bool,
) -> Result<()> {
    let mut stream_error: Option<String> = None;
    client
        .stream_master_plan_chat(req, |event_type, data| {
            if emit_json {
                println!("{}", json_event_line(event_type, data));
                return Ok(());
            }
            render_plain_event(event_type, data, &mut stream_error)
        })
        .await?;
    if let Some(message) = stream_error {
        bail!("master plan chat error: {message}");
    }
    Ok(())
}

/// Render one SSE event as human-readable terminal output, streaming
/// `text_delta`/`reasoning_delta` chunks without trailing newlines. `error`
/// events are captured into `stream_error` instead of printed immediately,
/// so the caller can surface them as a command failure once the stream ends.
/// Unlike goal-chat, there is no `goal` event to render since master-plan
/// isn't scoped to a single goal.
fn render_plain_event(
    event_type: &str,
    data: &str,
    stream_error: &mut Option<String>,
) -> Result<()> {
    match event_type {
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

#[cfg(test)]
mod tests {
    use super::MasterPlanReasoning;

    #[test]
    fn reasoning_as_str_matches_backend_values() {
        assert_eq!(MasterPlanReasoning::Low.as_str(), "low");
        assert_eq!(MasterPlanReasoning::Medium.as_str(), "medium");
        assert_eq!(MasterPlanReasoning::High.as_str(), "high");
    }
}
