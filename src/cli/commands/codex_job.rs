use anyhow::{Context, Result, bail};
use clap::Subcommand;
use colored::Colorize;
use serde_json::{Map, Value};

use crate::api::{ApiClient, CodexJob, CodexJobCreateRequest};
use crate::cli::commands::confirm;
use crate::cli::commands::org::resolve_org_id;

/// Build a client whose `X-Organization-ID` header targets `org_id`.
/// Mirrors `skill::client_for_org` / `execution::client_for_org` — the
/// `/api/v2/codex/jobs` routes resolve the organization from that header
/// (no org segment in the path).
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

/// Parse a `--workspace-scope-json` flag value as a JSON object (the
/// backend's `map[string]interface{}` field requires an object).
fn parse_json_object(raw: &str, flag_name: &str) -> Result<Map<String, Value>> {
    let value: Value =
        serde_json::from_str(raw).with_context(|| format!("{flag_name} must be valid JSON"))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => bail!("{flag_name} must be a JSON object, e.g. '{{\"key\":\"value\"}}'"),
    }
}

/// Human-readable one-line summary shared by `list`/`get`/`create`/`resume`.
/// Active (non-terminal) jobs get a `*` marker after the status.
fn format_job_line(job: &CodexJob) -> String {
    let marker = if job.status.is_active() { "*" } else { "" };
    let prompt = job.prompt.lines().next().unwrap_or_default();
    format!("{} [{}{marker}] {}", job.id, job.status, prompt)
}

#[derive(Subcommand)]
pub enum CodexJobCommands {
    /// List your cloud Codex jobs (newest first; jobs are private to the
    /// member who started them)
    List {
        /// Max number of results (default: 50, max: 100)
        #[arg(long)]
        limit: Option<u32>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a single cloud Codex job
    Get {
        /// Job ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start a new cloud Codex job. NOTE: only one active session per user —
    /// if you already have an active job, the backend closes it first
    Create {
        /// Initial prompt (backend defaults to "Codex session" when omitted)
        #[arg(long)]
        prompt: Option<String>,
        /// Raw JSON object describing the workspace scope
        #[arg(long)]
        workspace_scope_json: Option<String>,
        /// Carry over the conversation thread of one of your finished jobs
        #[arg(long)]
        resume_job_id: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Send a follow-up prompt to an active job
    Input {
        /// Job ID
        id: String,
        /// Prompt text to send
        #[arg(long)]
        prompt: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Resume a finished (terminal) job, re-queueing it with its thread intact
    Resume {
        /// Job ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Interrupt the currently running turn (Escape-like; the session stays alive)
    Cancel {
        /// Job ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Close a job's session (requests termination; the job becomes inactive)
    Close {
        /// Job ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip the confirmation prompt
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a job (soft delete; closes it first when still active)
    Delete {
        /// Job ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip the confirmation prompt
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Stream a job's event log (SSE) until the job reaches a terminal state
    Events {
        /// Job ID
        id: String,
        /// Resume after this event sequence number (for reconnects)
        #[arg(long)]
        from_seq: Option<u64>,
        /// Replay only the last N stored events (server cap: 2000; ignored
        /// when --from-seq is set)
        #[arg(long)]
        tail: Option<u64>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Emit each event as a raw JSON line instead of formatted text
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_codex_job(cmd: &CodexJobCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        CodexJobCommands::List { limit, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.list_codex_jobs(*limit).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.jobs.is_empty() {
                println!("{}", "No Codex jobs found.".dimmed());
            } else {
                for job in &resp.jobs {
                    println!("{}", format_job_line(job));
                }
            }
            Ok(())
        }
        CodexJobCommands::Get { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let job = scoped.get_codex_job(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&job)?);
            } else {
                println!("{}", format_job_line(&job));
                println!("created: {}  updated: {}", job.created_at, job.updated_at);
                if let Some(started) = &job.started_at {
                    println!("started: {started}");
                }
                if let Some(finished) = &job.finished_at {
                    println!("finished: {finished}");
                }
                if !job.error_message.is_empty() {
                    println!("error: {}", job.error_message);
                }
            }
            Ok(())
        }
        CodexJobCommands::Create {
            prompt,
            workspace_scope_json,
            resume_job_id,
            org,
            json,
        } => {
            let workspace_scope = workspace_scope_json
                .as_deref()
                .map(|raw| parse_json_object(raw, "--workspace-scope-json"))
                .transpose()?;
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = CodexJobCreateRequest {
                prompt: prompt.clone(),
                workspace_scope,
                resume_job_id: resume_job_id.clone(),
            };
            let job = scoped.create_codex_job(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&job)?);
            } else {
                println!("Created Codex job {}", format_job_line(&job));
            }
            Ok(())
        }
        CodexJobCommands::Input {
            id,
            prompt,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped.send_codex_job_input(id, prompt).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"sent": true, "id": id}))?
                );
            } else {
                println!("Sent input to Codex job {id}");
            }
            Ok(())
        }
        CodexJobCommands::Resume { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let job = scoped.resume_codex_job(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&job)?);
            } else {
                println!("Resumed Codex job {}", format_job_line(&job));
            }
            Ok(())
        }
        CodexJobCommands::Cancel { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped.cancel_codex_job(id).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"cancelRequested": true, "id": id})
                    )?
                );
            } else {
                println!("Requested cancel of the running turn for Codex job {id}");
            }
            Ok(())
        }
        CodexJobCommands::Close {
            id,
            org,
            force,
            json,
        } => {
            if !*force && !confirm(&format!("Close Codex job {id}? (ends its session)"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped.close_codex_job(id).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"closeRequested": true, "id": id})
                    )?
                );
            } else {
                println!("Requested close of Codex job {id}");
            }
            Ok(())
        }
        CodexJobCommands::Delete {
            id,
            org,
            force,
            json,
        } => {
            if !*force && !confirm(&format!("Delete Codex job {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped.delete_codex_job(id).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"deleted": true, "id": id}))?
                );
            } else {
                println!("Deleted Codex job {id}");
            }
            Ok(())
        }
        CodexJobCommands::Events {
            id,
            from_seq,
            tail,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let emit_json = *json;
            scoped
                .stream_codex_job_events(id, *from_seq, *tail, |event_type, data| {
                    if emit_json {
                        println!("{data}");
                    } else {
                        println!("{}", format_event_line(event_type, data));
                    }
                    Ok(())
                })
                .await
        }
    }
}

/// Render one SSE event as a human-readable line. Falls back to the raw
/// payload for unknown/opaque event types.
fn format_event_line(event_type: &str, data: &str) -> String {
    let payload_text =
        serde_json::from_str::<Value>(data)
            .ok()
            .and_then(|value| match value.get("payload") {
                Some(Value::String(text)) => Some(text.clone()),
                Some(Value::Null) | None => None,
                Some(other) => Some(other.to_string()),
            });
    match payload_text {
        Some(text) if !text.is_empty() => format!("[{event_type}] {text}"),
        _ => format!("[{event_type}] {data}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{format_event_line, format_job_line, parse_json_object};
    use crate::api::{CodexJob, CodexJobStatus};

    fn sample_job() -> CodexJob {
        CodexJob {
            id: "job-1".to_string(),
            organization_id: "org-1".to_string(),
            requested_by_user_id: "user-1".to_string(),
            requested_by_member_id: "member-1".to_string(),
            status: CodexJobStatus::Running,
            prompt: "first line\nsecond line".to_string(),
            workspace_scope: None,
            runner_id: String::new(),
            error_message: String::new(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            started_at: None,
            finished_at: None,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn format_job_line_marks_active_jobs() {
        assert_eq!(
            format_job_line(&sample_job()),
            "job-1 [running*] first line"
        );
    }

    #[test]
    fn format_job_line_leaves_terminal_jobs_unmarked() {
        let mut job = sample_job();
        job.status = CodexJobStatus::Succeeded;
        assert_eq!(format_job_line(&job), "job-1 [succeeded] first line");
    }

    #[test]
    fn parse_json_object_accepts_valid_object() {
        let map = parse_json_object(r#"{"repo":"a/b"}"#, "--workspace-scope-json").unwrap();
        assert_eq!(map["repo"], "a/b");
    }

    #[test]
    fn parse_json_object_rejects_non_object() {
        let err = parse_json_object("[1,2]", "--workspace-scope-json").unwrap_err();
        assert!(err.to_string().contains("--workspace-scope-json"));
    }

    #[test]
    fn format_event_line_extracts_string_payload() {
        let line = format_event_line("stdout", r#"{"seq":1,"type":"stdout","payload":"hello"}"#);
        assert_eq!(line, "[stdout] hello");
    }

    #[test]
    fn format_event_line_stringifies_object_payload() {
        let line = format_event_line("status", r#"{"seq":2,"payload":{"status":"idle"}}"#);
        assert_eq!(line, r#"[status] {"status":"idle"}"#);
    }

    #[test]
    fn format_event_line_falls_back_to_raw_data() {
        let line = format_event_line("caught_up", r#"{"seq":3}"#);
        assert_eq!(line, r#"[caught_up] {"seq":3}"#);
    }
}
