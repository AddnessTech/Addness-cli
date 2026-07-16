//! `addness thread ...` — AI スレッド (Thread) の CRUD・メッセージ・チャット
//! (SSE)・アクショントレース・共有リンク・質問応答・ツール実行承認応答。
//!
//! `presentation/routes/api.go`の`/api/v1/team/ai/threads...`（legacy V1
//! "AI エージェント"レイヤー、`presentation/handlers/ai`）を叩く。goal-chat
//! 等のジェネリックチャット系（`internal/chat/handler`）とは別系統で、
//! アクショントレース・取消、共有リンク、質問応答、ツール実行承認といった
//! goal-chat系には無い機能を提供する唯一の現行ルート。
//!
//! `chat`/`edit-and-regenerate`のSSEイベント語彙・ワイヤ形式は
//! `addness goal decompose`と同じ（`infra/ai/streaming.SSEWriter`、
//! `event:`行なしの`data: {"type": "...", ...}`のみ）なので、レンダリングは
//! `goal_decompose_render::render_plain_event`をそのまま再利用する。

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::api::{
    ApiClient, MessageEditRequest, QuestionRespondRequest, ThreadChatRequest, ThreadCreateRequest,
    ThreadListParams, ThreadUpdateRequest, ToolConfirmationRespondRequest,
};
use crate::cli::commands::ai_chat_render::json_event_line;
use crate::cli::commands::goal_decompose_render;
use crate::cli::commands::org::resolve_org_id;

/// Build a client whose `X-Organization-ID` header targets `org_id`. Mirrors
/// `master_plan::client_for_org` / `core_values::client_for_org` — the
/// `/api/v1/team/ai/threads...` routes resolve the organization from that
/// header (no org segment in the path).
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

/// Parses a `--metadata` flag value (a JSON object as text) into a
/// `serde_json::Value`, failing loudly rather than silently sending garbage.
fn parse_metadata(raw: &str) -> Result<Value> {
    serde_json::from_str(raw).context("--metadata must be valid JSON")
}

#[derive(Subcommand)]
pub enum ThreadCommands {
    /// Create a new AI thread
    Create {
        /// Thread title (optional; the backend accepts an empty title)
        #[arg(long, default_value = "")]
        title: String,
        /// Metadata as a JSON object, e.g. '{"key":"value"}'
        #[arg(long)]
        metadata: Option<String>,
        /// Bind the thread to a specific AI agent (organization member ID)
        #[arg(long)]
        agent_id: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List your AI threads (server-paginated)
    List {
        /// Filter by AI agent (organization member ID)
        #[arg(long)]
        agent_id: Option<String>,
        /// Filter scope ("agent_hub" is the only accepted non-empty value)
        #[arg(long)]
        scope: Option<String>,
        /// Filter to threads scoped to a specific goal (objective) ID
        #[arg(long)]
        objective_id: Option<String>,
        /// Max threads to return (backend default 20, max 100)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u32>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a single thread
    Get {
        /// Thread ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Rename a thread and/or replace its metadata
    Update {
        /// Thread ID
        id: String,
        /// New title (required by the backend; the empty string is allowed)
        #[arg(long, default_value = "")]
        title: String,
        /// Metadata as a JSON object, e.g. '{"key":"value"}'
        #[arg(long)]
        metadata: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Permanently delete a thread
    Delete {
        /// Thread ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// List a thread's messages
    Messages {
        /// Thread ID
        id: String,
        /// Max messages to return (backend default 50)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u32>,
        /// Include internal-only messages (debug); omitted returns the
        /// UI-visible set only
        #[arg(long)]
        include_internal: bool,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Send a message in a thread and stream the AI agent's reply (SSE,
    /// ends on the backend's `finish` event). LLM-billed.
    Chat {
        /// Thread ID
        id: String,
        /// Message text to send
        message: String,
        /// Chat mode (e.g. hearing_mode, gekizume_mode, planning_mode)
        #[arg(long)]
        mode: Option<String>,
        /// Model name override
        #[arg(long)]
        model: Option<String>,
        /// Goal (objective) IDs mentioned in the message (comma-separated)
        #[arg(long, value_delimiter = ',')]
        mentioned_objective_ids: Vec<String>,
        /// Member IDs mentioned in the message (comma-separated)
        #[arg(long, value_delimiter = ',')]
        mentioned_member_ids: Vec<String>,
        /// Skill IDs mentioned in the message (comma-separated)
        #[arg(long, value_delimiter = ',')]
        mentioned_skill_ids: Vec<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Emit each SSE event as a JSON line (`{"event":...,"data":...}`)
        /// instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Cancel the thread's currently-running turn, if any
    Cancel {
        /// Thread ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
    },
    /// Edit a past message, drop everything after it, and stream a
    /// regenerated reply (SSE, ends on the backend's `finish` event).
    /// LLM-billed.
    EditAndRegenerate {
        /// Thread ID
        thread_id: String,
        /// Message ID to edit
        message_id: String,
        /// New message content
        content: String,
        /// Chat mode (e.g. hearing_mode, gekizume_mode, planning_mode)
        #[arg(long)]
        mode: Option<String>,
        /// Goal (objective) IDs mentioned in the message (comma-separated)
        #[arg(long, value_delimiter = ',')]
        mentioned_objective_ids: Vec<String>,
        /// Member IDs mentioned in the message (comma-separated)
        #[arg(long, value_delimiter = ',')]
        mentioned_member_ids: Vec<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Emit each SSE event as a JSON line (`{"event":...,"data":...}`)
        /// instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// List a thread's action traces (tool executions the agent performed)
    Traces {
        /// Thread ID
        id: String,
        /// Max traces to return (backend default 50)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u32>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Revert (undo) a previously-executed, revertible action trace
    RevertTrace {
        /// Thread ID
        thread_id: String,
        /// Trace ID (from `addness thread traces`)
        trace_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create (or refresh) a public share link for a thread
    ShareCreate {
        /// Thread ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Revoke a thread's public share link
    ShareRevoke {
        /// Thread ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Answer a pending in-chat question the agent asked
    QuestionRespond {
        /// Thread ID
        id: String,
        /// Pending question's request ID
        #[arg(long)]
        request_id: String,
        /// Single-choice answer
        #[arg(long)]
        answer: Option<String>,
        /// Multi-choice answers (comma-separated)
        #[arg(long, value_delimiter = ',')]
        answers: Vec<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Approve or reject a pending tool-execution confirmation request
    ToolConfirmationRespond {
        /// Thread ID
        id: String,
        /// Pending confirmation's request ID
        #[arg(long)]
        request_id: String,
        /// Approve the tool execution (mutually exclusive with --reject)
        #[arg(long, conflicts_with = "reject")]
        approve: bool,
        /// Reject the tool execution (mutually exclusive with --approve)
        #[arg(long, conflicts_with = "approve")]
        reject: bool,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Fetch the thread tying a specific goal to a specific AI member
    Assignment {
        /// Goal (objective) ID
        #[arg(long)]
        objective_id: String,
        /// AI member's organization member ID
        #[arg(long)]
        member_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_thread(cmd: &ThreadCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ThreadCommands::Create {
            title,
            metadata,
            agent_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = ThreadCreateRequest {
                title: title.clone(),
                metadata: metadata.as_deref().map(parse_metadata).transpose()?,
                ai_agent_id: agent_id.clone(),
            };
            let thread = scoped.create_thread(&req).await?;
            print_thread(&thread, *json)
        }
        ThreadCommands::List {
            agent_id,
            scope,
            objective_id,
            limit,
            offset,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let params = ThreadListParams {
                agent_id: agent_id.as_deref(),
                scope: scope.as_deref(),
                objective_id: objective_id.as_deref(),
                limit: *limit,
                offset: *offset,
            };
            let resp = scoped.list_threads(&params).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.threads.is_empty() {
                println!("{}", "No threads found.".dimmed());
            } else {
                for thread in &resp.threads {
                    let last = thread.last_message_at.as_deref().unwrap_or("-");
                    let title = single_line_title(&thread.title);
                    println!(
                        "{}  {}  status={}  messages={}  last={last}",
                        thread.id, title, thread.status, thread.message_count
                    );
                }
                println!(
                    "{}",
                    format!(
                        "-- total={} limit={} offset={}",
                        resp.total, resp.limit, resp.offset
                    )
                    .dimmed()
                );
            }
            Ok(())
        }
        ThreadCommands::Get { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let thread = scoped.get_thread(id).await?;
            print_thread(&thread, *json)
        }
        ThreadCommands::Update {
            id,
            title,
            metadata,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = ThreadUpdateRequest {
                title: title.clone(),
                metadata: metadata.as_deref().map(parse_metadata).transpose()?,
            };
            let thread = scoped.update_thread(id, &req).await?;
            print_thread(&thread, *json)
        }
        ThreadCommands::Delete { id, org, force } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            if !force && !crate::cli::commands::confirm(&format!("Delete thread {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            scoped.delete_thread(id).await?;
            println!("Thread {id} deleted");
            Ok(())
        }
        ThreadCommands::Messages {
            id,
            limit,
            offset,
            include_internal,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped
                .get_thread_messages(id, *limit, *offset, *include_internal)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.messages.is_empty() {
                println!("{}", "No messages found.".dimmed());
            } else {
                for message in &resp.messages {
                    let text = message
                        .content
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| message.content.to_string());
                    println!("[{}] {}", message.role, text);
                }
            }
            Ok(())
        }
        ThreadCommands::Chat {
            id,
            message,
            mode,
            model,
            mentioned_objective_ids,
            mentioned_member_ids,
            mentioned_skill_ids,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = ThreadChatRequest {
                message: message.clone(),
                mode: mode.clone(),
                model: model.clone(),
                mentioned_objective_ids: mentioned_objective_ids.clone(),
                mentioned_member_ids: mentioned_member_ids.clone(),
                mentioned_skill_ids: mentioned_skill_ids.clone(),
            };
            let mut stream_error: Option<String> = None;
            scoped
                .stream_thread_chat(id, &req, |event_type, data| {
                    if *json {
                        println!("{}", json_event_line(event_type, data));
                        return Ok(());
                    }
                    goal_decompose_render::render_plain_event(event_type, data, &mut stream_error);
                    Ok(())
                })
                .await?;
            if let Some(message) = stream_error {
                bail!("thread chat error: {message}");
            }
            Ok(())
        }
        ThreadCommands::Cancel { id, org } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped.cancel_thread(id).await?;
            println!("Thread {id} run cancelled");
            Ok(())
        }
        ThreadCommands::EditAndRegenerate {
            thread_id,
            message_id,
            content,
            mode,
            mentioned_objective_ids,
            mentioned_member_ids,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = MessageEditRequest {
                content: content.clone(),
                mode: mode.clone(),
                mentioned_objective_ids: mentioned_objective_ids.clone(),
                mentioned_member_ids: mentioned_member_ids.clone(),
            };
            let mut stream_error: Option<String> = None;
            scoped
                .stream_thread_edit_and_regenerate(
                    thread_id,
                    message_id,
                    &req,
                    |event_type, data| {
                        if *json {
                            println!("{}", json_event_line(event_type, data));
                            return Ok(());
                        }
                        goal_decompose_render::render_plain_event(
                            event_type,
                            data,
                            &mut stream_error,
                        );
                        Ok(())
                    },
                )
                .await?;
            if let Some(message) = stream_error {
                bail!("thread edit-and-regenerate error: {message}");
            }
            Ok(())
        }
        ThreadCommands::Traces {
            id,
            limit,
            offset,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.list_thread_traces(id, *limit, *offset).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.traces.is_empty() {
                println!("{}", "No action traces found.".dimmed());
            } else {
                for trace in &resp.traces {
                    println!(
                        "{}  {}  status={}  revertible={}",
                        trace.id, trace.tool_name, trace.status, trace.is_revertible
                    );
                }
            }
            Ok(())
        }
        ThreadCommands::RevertTrace {
            thread_id,
            trace_id,
            org,
            force,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            if !force && !crate::cli::commands::confirm(&format!("Revert trace {trace_id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let resp = scoped.revert_thread_trace(thread_id, trace_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Trace {trace_id} reverted (success={})", resp.success);
            }
            Ok(())
        }
        ThreadCommands::ShareCreate { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.create_thread_share_link(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                let url = resp
                    .share_url
                    .as_deref()
                    .unwrap_or(resp.share_token.as_str());
                println!("Share link created: {url}");
            }
            Ok(())
        }
        ThreadCommands::ShareRevoke { id, org, force } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            if !force
                && !crate::cli::commands::confirm(&format!("Revoke share link for thread {id}?"))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            scoped.revoke_thread_share_link(id).await?;
            println!("Share link revoked for thread {id}");
            Ok(())
        }
        ThreadCommands::QuestionRespond {
            id,
            request_id,
            answer,
            answers,
            org,
            json,
        } => {
            if answer.is_none() && answers.is_empty() {
                bail!("Specify either --answer or --answers");
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = QuestionRespondRequest {
                request_id: request_id.clone(),
                answer: answer.clone(),
                answers: answers.clone(),
            };
            let resp = scoped.respond_to_thread_question(id, &req).await?;
            print_action_result(&resp, *json)
        }
        ThreadCommands::ToolConfirmationRespond {
            id,
            request_id,
            approve,
            reject,
            org,
            json,
        } => {
            if !approve && !reject {
                bail!("Specify either --approve or --reject");
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = ToolConfirmationRespondRequest {
                request_id: request_id.clone(),
                approved: *approve,
            };
            let resp = scoped.respond_to_thread_tool_confirmation(id, &req).await?;
            print_action_result(&resp, *json)
        }
        ThreadCommands::Assignment {
            objective_id,
            member_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let thread = scoped
                .get_objective_assignment_thread(objective_id, member_id)
                .await?;
            print_thread(&thread, *json)
        }
    }
}

/// Collapses a (possibly multi-line) thread title into a single display
/// line, e.g. for `thread list`/`thread get` plain-text rows. Falls back to
/// a placeholder for empty titles, which the backend allows.
fn single_line_title(title: &str) -> String {
    if title.is_empty() {
        return "(untitled)".to_string();
    }
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn print_thread(thread: &crate::api::ThreadResponse, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(thread)?);
    } else {
        let title = single_line_title(&thread.title);
        println!("{} {}", thread.id.bold(), title);
        println!("  status: {}", thread.status);
        println!("  messages: {}", thread.message_count);
        if let Some(agent_id) = &thread.ai_agent_id {
            println!("  aiAgentId: {agent_id}");
        }
        if let Some(share_token) = &thread.share_token {
            println!("  shareToken: {share_token} (public={})", thread.is_public);
        }
        if let Some(last) = &thread.last_message_at {
            println!("  lastMessageAt: {last}");
        }
    }
    Ok(())
}

fn print_action_result(resp: &crate::api::ThreadActionResultResponse, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(resp)?);
    } else {
        println!(
            "{} {}",
            if resp.success { "OK" } else { "FAILED" },
            resp.message
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_metadata, single_line_title};

    #[test]
    fn single_line_title_placeholder_for_empty() {
        assert_eq!(single_line_title(""), "(untitled)");
    }

    #[test]
    fn single_line_title_collapses_newlines_and_extra_whitespace() {
        assert_eq!(
            single_line_title("今日はどれに集中する?\n\n（質問）今日は"),
            "今日はどれに集中する? （質問）今日は"
        );
    }

    #[test]
    fn single_line_title_leaves_simple_titles_untouched() {
        assert_eq!(single_line_title("Renamed"), "Renamed");
    }

    #[test]
    fn parse_metadata_accepts_json_object() {
        let value = parse_metadata(r#"{"k":"v"}"#).unwrap();
        assert_eq!(value, serde_json::json!({"k": "v"}));
    }

    #[test]
    fn parse_metadata_rejects_invalid_json() {
        assert!(parse_metadata("not json").is_err());
    }
}
