use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use colored::Colorize;

use crate::api::{
    ApiClient, HuddleInvitationSendRequest, HuddleInviteableMembersParams, HuddleMemberSortBy,
    HuddleRecordingStartRequest, HuddleSortDir, MeetingBotJobCreateRequest,
    MeetingNoteCreateGoalsRequest, MeetingNoteCreateGoalsRequestItem,
    MeetingNotePostMinutesRequest, MeetingNotePostType, MeetingNoteSuggestGoalsRequest,
    MeetingNoteSummarizeRequest, MinuteCreateRequest, MinuteListParams, MinuteSourceType,
    MinuteUpdateRequest,
};
use crate::cli::commands::confirm;
use crate::cli::commands::org::resolve_org_id;

/// Build a client whose `X-Organization-ID` header targets `org_id`.
/// Mirrors `execution::client_for_org` / `media::client_for_org`.
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

/// Allowed audio MIME types for `meeting notes transcribe`, mirrored from the
/// backend content-type whitelist
/// (`presentation/handlers/meeting_note/transcribe.go`).
fn guess_meeting_audio_content_type(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    let content_type = match ext.as_deref() {
        Some("webm") => "audio/webm",
        Some("mp4") => "audio/mp4",
        Some("m4a") => "audio/x-m4a",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("flac") => "audio/flac",
        other => bail!(
            "Unsupported file extension {:?}. Meeting-note transcription only accepts \
             webm/mp4/m4a/mp3/wav/flac audio.",
            other.unwrap_or("(none)")
        ),
    };
    Ok(content_type.to_string())
}

fn parse_goals_json(raw: &str) -> Result<Vec<MeetingNoteCreateGoalsRequestItem>> {
    serde_json::from_str(raw).context("--goals-json must be a JSON array of {title, description?}")
}

#[derive(Subcommand)]
pub enum MeetingCommands {
    /// Huddle voice-call read/control commands (status, recording, invitations).
    /// Live participation (join/leave/switch, LiveKit token, heartbeat,
    /// screen-share) is out of scope — see `addness meeting huddle --help`.
    Huddle {
        #[command(subcommand)]
        command: HuddleCommands,
    },
    /// Meeting Bot (Recall.ai) job management
    Bot {
        #[command(subcommand)]
        command: BotCommands,
    },
    /// Meeting-note transcription/summary/goal workflow
    Notes {
        #[command(subcommand)]
        command: NotesCommands,
    },
    /// Minutes (議事録) CRUD
    Minutes {
        #[command(subcommand)]
        command: MinutesCommands,
    },
}

#[derive(Subcommand)]
pub enum HuddleCommands {
    /// Show a goal's huddle status (idle/active, recording state, participants)
    Status {
        /// Objective (goal) ID the huddle is attached to
        objective_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List active huddles in the subtree rooted at a goal
    ActiveSubtree {
        /// Root objective (goal) ID
        objective_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the status of a specific huddle session
    SessionStatus {
        /// Objective (goal) ID the huddle is attached to
        objective_id: String,
        /// Huddle session ID
        session_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start recording an in-progress huddle
    RecordingStart {
        /// Objective (goal) ID the huddle is attached to
        objective_id: String,
        /// Transcription language (e.g. "ja", "en")
        #[arg(long)]
        language: Option<String>,
        /// Automatically create child goals from the recording's minutes
        #[arg(long)]
        create_child_goals: bool,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Stop recording an in-progress huddle
    RecordingStop {
        /// Objective (goal) ID the huddle is attached to
        objective_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List organization members who can be invited to a goal's huddle
    InviteableMembers {
        /// Objective (goal) ID the huddle is attached to
        objective_id: String,
        /// Page number
        #[arg(long)]
        page: Option<u32>,
        /// Page size
        #[arg(long)]
        page_size: Option<u32>,
        /// Search by member name
        #[arg(long)]
        query: Option<String>,
        /// Sort field
        #[arg(long, value_enum)]
        sort_by: Option<HuddleMemberSortBy>,
        /// Sort direction
        #[arg(long, value_enum)]
        sort_dir: Option<HuddleSortDir>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the huddle you're currently in, if any (floating-bar state)
    Active {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List in-progress huddle transcription jobs
    TranscriptionProgress {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Send manual huddle invitations to organization members
    Invite {
        /// Huddle session ID
        session_id: String,
        /// Organization member ID to invite (repeatable, 1-49 total)
        #[arg(long = "member-id", required = true)]
        member_ids: Vec<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum BotCommands {
    /// List meeting-bot (Recall.ai) jobs
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a single meeting-bot job
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
    /// Create a meeting-bot job to join and record a meeting
    Create {
        /// Video platform (e.g. "zoom", "google_meet", "teams")
        #[arg(long)]
        platform: String,
        /// Meeting URL the bot should join
        #[arg(long)]
        meeting_url: String,
        /// Display name for the bot in the meeting
        #[arg(long)]
        bot_name: String,
        /// Message the bot posts in the meeting chat on join
        #[arg(long)]
        chat_join_message: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete (cancel) a meeting-bot job
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
}

#[derive(Subcommand)]
pub enum NotesCommands {
    /// Transcribe a local audio recording (webm/mp4/m4a/mp3/wav/flac, 10KB-5MB)
    Transcribe {
        /// Path to the local audio file
        file: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Summarize a transcript into meeting minutes
    Summarize {
        /// Raw transcript text
        #[arg(long)]
        transcript: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Post minutes to a goal as a comment and/or deliverable
    PostMinutes {
        /// Objective (goal) ID to post to
        objective_id: String,
        /// Existing minute ID to link (omit to post ad-hoc minutes/transcript)
        #[arg(long)]
        minute_id: Option<String>,
        /// Minutes text (required unless --minute-id is given)
        #[arg(long)]
        minutes: Option<String>,
        /// Transcript text (required unless --minute-id is given)
        #[arg(long)]
        transcript: Option<String>,
        /// Where to post: comment, deliverable, or both
        #[arg(long, value_enum)]
        post_type: MeetingNotePostType,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Suggest candidate goals from meeting minutes
    SuggestGoals {
        /// Objective (goal) ID to attach suggestions to
        objective_id: String,
        /// Minutes text (max 10000 chars)
        #[arg(long)]
        minutes: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create goals (1-7) from meeting minutes
    CreateGoals {
        /// Parent objective (goal) ID
        objective_id: String,
        /// Raw JSON array of goals: `[{"title":"...","description":"..."}]`
        #[arg(long)]
        goals_json: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum MinutesCommands {
    /// Create a minute record
    Create {
        /// Objective (goal) ID the minute belongs to
        #[arg(long)]
        objective_id: Option<String>,
        /// Zoom job ID (required when --source-type zoom)
        #[arg(long)]
        zoom_job_id: Option<String>,
        /// Source of the minute
        #[arg(long, value_enum)]
        source_type: MinuteSourceType,
        /// Title (max 200 chars)
        #[arg(long)]
        title: String,
        /// Summary (max 10000 chars)
        #[arg(long)]
        summary: String,
        /// Transcript (max 50000 chars)
        #[arg(long)]
        transcript: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List minutes
    List {
        /// Filter by objective (goal) ID
        #[arg(long)]
        objective_id: Option<String>,
        /// Filter by source type
        #[arg(long, value_enum)]
        source_type: Option<MinuteSourceType>,
        /// Only show minutes not yet linked to a goal
        #[arg(long)]
        only_unlinked: bool,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a single minute (with full transcript)
    Get {
        /// Minute ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a minute (partial update)
    Update {
        /// Minute ID
        id: String,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New summary
        #[arg(long)]
        summary: Option<String>,
        /// New transcript
        #[arg(long)]
        transcript: Option<String>,
        /// Re-link to a different objective (goal) ID
        #[arg(long)]
        objective_id: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a minute
    Delete {
        /// Minute ID
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
}

pub async fn handle_meeting(cmd: &MeetingCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        MeetingCommands::Huddle { command } => handle_huddle(command, client).await,
        MeetingCommands::Bot { command } => handle_bot(command, client).await,
        MeetingCommands::Notes { command } => handle_notes(command, client).await,
        MeetingCommands::Minutes { command } => handle_minutes(command, client).await,
    }
}

async fn handle_huddle(cmd: &HuddleCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        HuddleCommands::Status {
            objective_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let status = scoped.get_huddle_status(objective_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                println!(
                    "status: {}  recording: {}  participants: {}",
                    status.status,
                    status.recording,
                    status.participants.len()
                );
            }
            Ok(())
        }
        HuddleCommands::ActiveSubtree {
            objective_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.get_huddle_active_subtree(objective_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.active_huddles.is_empty() {
                println!("{}", "No active huddles in this subtree.".dimmed());
            } else {
                for huddle in &resp.active_huddles {
                    println!(
                        "{} — {} participant(s) since {}",
                        huddle.objective_id,
                        huddle.participants.len(),
                        huddle.started_at
                    );
                }
            }
            Ok(())
        }
        HuddleCommands::SessionStatus {
            objective_id,
            session_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let status = scoped
                .get_huddle_session_status(objective_id, session_id)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                println!(
                    "status: {}  recording: {}  participants: {}",
                    status.status,
                    status.recording,
                    status.participants.len()
                );
            }
            Ok(())
        }
        HuddleCommands::RecordingStart {
            objective_id,
            language,
            create_child_goals,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = HuddleRecordingStartRequest {
                language: language.clone(),
                create_child_goals: Some(*create_child_goals),
            };
            let resp = scoped.start_huddle_recording(objective_id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Recording started for {objective_id}.");
            }
            Ok(())
        }
        HuddleCommands::RecordingStop {
            objective_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.stop_huddle_recording(objective_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Recording stopped for {objective_id}.");
            }
            Ok(())
        }
        HuddleCommands::InviteableMembers {
            objective_id,
            page,
            page_size,
            query,
            sort_by,
            sort_dir,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let params = HuddleInviteableMembersParams {
                page: *page,
                page_size: *page_size,
                query: query.as_deref(),
                sort_by: sort_by.map(HuddleMemberSortBy::as_str),
                sort_dir: sort_dir.map(HuddleSortDir::as_str),
            };
            let resp = scoped
                .list_huddle_inviteable_members(objective_id, &params)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.members.is_empty() {
                println!("{}", "No inviteable members found.".dimmed());
            } else {
                for member in &resp.members {
                    println!("{} — {}", member.id, member.name);
                }
                println!(
                    "page {}/{} ({} total)",
                    resp.page, resp.total_pages, resp.total_count
                );
            }
            Ok(())
        }
        HuddleCommands::Active { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let active = scoped.get_huddle_active().await?;
            match active {
                Some(active) if *json => println!("{}", serde_json::to_string_pretty(&active)?),
                Some(active) => println!(
                    "{} — {} participant(s), joined {}",
                    active.objective_title,
                    active.participants.len(),
                    active.joined_at
                ),
                None if *json => println!("null"),
                None => println!("{}", "Not currently in a huddle.".dimmed()),
            }
            Ok(())
        }
        HuddleCommands::TranscriptionProgress { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.get_huddle_transcription_progress().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.items.is_empty() {
                println!("{}", "No transcription jobs in progress.".dimmed());
            } else {
                for item in &resp.items {
                    println!(
                        "{} [{}] — objective {}",
                        item.id, item.status, item.objective_id
                    );
                }
            }
            Ok(())
        }
        HuddleCommands::Invite {
            session_id,
            member_ids,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = HuddleInvitationSendRequest {
                organization_member_ids: member_ids.clone(),
            };
            let resp = scoped.send_huddle_invitations(session_id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                for result in &resp.results {
                    println!("{} — {:?}", result.organization_member_id, result.status);
                }
            }
            Ok(())
        }
    }
}

async fn handle_bot(cmd: &BotCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        BotCommands::List { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let jobs = scoped.list_meeting_bot_jobs().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&jobs)?);
            } else if jobs.is_empty() {
                println!("{}", "No meeting-bot jobs found.".dimmed());
            } else {
                for job in &jobs {
                    println!("{} [{}] — {}", job.id, job.status, job.meeting_url);
                }
            }
            Ok(())
        }
        BotCommands::Get { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let job = scoped.get_meeting_bot_job(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&job)?);
            } else {
                println!("{} [{}] — {}", job.id, job.status, job.meeting_url);
            }
            Ok(())
        }
        BotCommands::Create {
            platform,
            meeting_url,
            bot_name,
            chat_join_message,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = MeetingBotJobCreateRequest {
                platform: platform.clone(),
                meeting_url: meeting_url.clone(),
                bot_name: bot_name.clone(),
                chat_join_message: chat_join_message.clone(),
            };
            let job = scoped.create_meeting_bot_job(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&job)?);
            } else {
                println!("Created meeting-bot job {} [{}]", job.id, job.status);
            }
            Ok(())
        }
        BotCommands::Delete {
            id,
            org,
            force,
            json,
        } => {
            if !*force && !confirm(&format!("Delete meeting-bot job {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped.delete_meeting_bot_job(id).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"deleted": true, "id": id}))?
                );
            } else {
                println!("Deleted meeting-bot job {id}");
            }
            Ok(())
        }
    }
}

async fn handle_notes(cmd: &NotesCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        NotesCommands::Transcribe { file, json } => {
            let file_path = Path::new(file);
            let metadata = std::fs::metadata(file_path)
                .with_context(|| format!("Failed to stat file {file}"))?;
            if !metadata.is_file() {
                bail!("{file} is not a regular file");
            }
            let file_name = file_path
                .file_name()
                .and_then(|s| s.to_str())
                .map(String::from)
                .ok_or_else(|| anyhow::anyhow!("Cannot derive file name from {file}"))?;
            let content_type = guess_meeting_audio_content_type(file_path)?;
            let bytes =
                std::fs::read(file_path).with_context(|| format!("Failed to read file {file}"))?;
            let resp = client
                .transcribe_meeting_note(bytes, &file_name, &content_type)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("{}", resp.transcript);
            }
            Ok(())
        }
        NotesCommands::Summarize { transcript, json } => {
            let req = MeetingNoteSummarizeRequest {
                transcript: transcript.clone(),
            };
            let resp = client.summarize_meeting_note(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("{}", resp.minutes);
            }
            Ok(())
        }
        NotesCommands::PostMinutes {
            objective_id,
            minute_id,
            minutes,
            transcript,
            post_type,
            org,
            json,
        } => {
            if minute_id.is_none() && minutes.is_none() && transcript.is_none() {
                bail!("--minute-id, or --minutes/--transcript, is required");
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = MeetingNotePostMinutesRequest {
                objective_id: objective_id.clone(),
                minute_id: minute_id.clone(),
                minutes: minutes.clone(),
                transcript: transcript.clone(),
                post_type: *post_type,
            };
            let resp = scoped.post_meeting_note_minutes(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!(
                    "posted: comment={} deliverable={}",
                    resp.comment, resp.deliverable
                );
            }
            Ok(())
        }
        NotesCommands::SuggestGoals {
            objective_id,
            minutes,
            json,
        } => {
            let req = MeetingNoteSuggestGoalsRequest {
                objective_id: objective_id.clone(),
                minutes: minutes.clone(),
            };
            let resp = client.suggest_meeting_note_goals(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.suggestions.is_empty() {
                println!("{}", "No goal suggestions.".dimmed());
            } else {
                for suggestion in &resp.suggestions {
                    println!("- {}", suggestion.title);
                    if let Some(description) = &suggestion.description {
                        println!("  {description}");
                    }
                }
            }
            Ok(())
        }
        NotesCommands::CreateGoals {
            objective_id,
            goals_json,
            json,
        } => {
            let goals = parse_goals_json(goals_json)?;
            if goals.is_empty() || goals.len() > 7 {
                bail!(
                    "--goals-json must contain between 1 and 7 goals (got {})",
                    goals.len()
                );
            }
            let req = MeetingNoteCreateGoalsRequest {
                objective_id: objective_id.clone(),
                goals,
            };
            let resp = client.create_meeting_note_goals(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!(
                    "Created {}/{} goal(s):",
                    resp.created_count, resp.requested_count
                );
                for goal in &resp.goals {
                    println!("- {} ({})", goal.title, goal.id);
                }
            }
            Ok(())
        }
    }
}

async fn handle_minutes(cmd: &MinutesCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        MinutesCommands::Create {
            objective_id,
            zoom_job_id,
            source_type,
            title,
            summary,
            transcript,
            org,
            json,
        } => {
            if matches!(source_type, MinuteSourceType::Zoom) && zoom_job_id.is_none() {
                bail!("--zoom-job-id is required when --source-type zoom");
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = MinuteCreateRequest {
                objective_id: objective_id.clone(),
                zoom_job_id: zoom_job_id.clone(),
                source_type: *source_type,
                title: title.clone(),
                summary: summary.clone(),
                transcript: transcript.clone(),
            };
            let minute = scoped.create_minute(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&minute)?);
            } else {
                println!("Created minute {} — {}", minute.id, minute.title);
            }
            Ok(())
        }
        MinutesCommands::List {
            objective_id,
            source_type,
            only_unlinked,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let params = MinuteListParams {
                objective_id: objective_id.as_deref(),
                source_type: source_type.map(minute_source_type_as_str),
                only_unlinked: *only_unlinked,
            };
            let resp = scoped.list_minutes(&params).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.minutes.is_empty() {
                println!("{}", "No minutes found.".dimmed());
            } else {
                for minute in &resp.minutes {
                    println!(
                        "{} — {} — {}",
                        minute.id, minute.title, minute.summary_preview
                    );
                }
            }
            Ok(())
        }
        MinutesCommands::Get { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let minute = scoped.get_minute(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&minute)?);
            } else {
                println!("{} — {}", minute.id, minute.title);
                println!("{}", minute.summary);
            }
            Ok(())
        }
        MinutesCommands::Update {
            id,
            title,
            summary,
            transcript,
            objective_id,
            org,
            json,
        } => {
            if title.is_none()
                && summary.is_none()
                && transcript.is_none()
                && objective_id.is_none()
            {
                bail!("At least one of --title/--summary/--transcript/--objective-id is required");
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = MinuteUpdateRequest {
                title: title.clone(),
                summary: summary.clone(),
                transcript: transcript.clone(),
                objective_id: objective_id.clone(),
            };
            let minute = scoped.update_minute(id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&minute)?);
            } else {
                println!("Updated minute {} — {}", minute.id, minute.title);
            }
            Ok(())
        }
        MinutesCommands::Delete {
            id,
            org,
            force,
            json,
        } => {
            if !*force && !confirm(&format!("Delete minute {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped.delete_minute(id).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"deleted": true, "id": id}))?
                );
            } else {
                println!("Deleted minute {id}");
            }
            Ok(())
        }
    }
}

fn minute_source_type_as_str(source_type: MinuteSourceType) -> &'static str {
    match source_type {
        MinuteSourceType::Recording => "recording",
        MinuteSourceType::Zoom => "zoom",
    }
}

#[cfg(test)]
mod tests {
    use super::{guess_meeting_audio_content_type, parse_goals_json};
    use std::path::Path;

    #[test]
    fn guess_meeting_audio_content_type_maps_known_extensions() {
        assert_eq!(
            guess_meeting_audio_content_type(Path::new("a.mp3")).unwrap(),
            "audio/mpeg"
        );
        assert_eq!(
            guess_meeting_audio_content_type(Path::new("a.WAV")).unwrap(),
            "audio/wav"
        );
        assert_eq!(
            guess_meeting_audio_content_type(Path::new("a.webm")).unwrap(),
            "audio/webm"
        );
    }

    #[test]
    fn guess_meeting_audio_content_type_rejects_unsupported_extension() {
        let err = guess_meeting_audio_content_type(Path::new("a.pdf")).unwrap_err();
        assert!(err.to_string().contains("Unsupported file extension"));
    }

    #[test]
    fn parse_goals_json_accepts_valid_array() {
        let goals = parse_goals_json(r#"[{"title":"a"},{"title":"b","description":"d"}]"#).unwrap();
        assert_eq!(goals.len(), 2);
        assert_eq!(goals[0].title, "a");
        assert_eq!(goals[1].description.as_deref(), Some("d"));
    }

    #[test]
    fn parse_goals_json_rejects_invalid_json() {
        let err = parse_goals_json("not json").unwrap_err();
        assert!(err.to_string().contains("--goals-json"));
    }
}
