use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

use crate::api::{
    ApiClient, HuddleInvitationSendRequest, HuddleInviteableMembersParams, HuddleMemberSortBy,
    HuddleRecordingStartRequest, HuddleSortDir, MeetingBotJobCreateRequest,
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
        /// Meeting URL the bot should join
        #[arg(long)]
        meeting_url: String,
        /// Optional meeting title
        #[arg(long)]
        meeting_title: Option<String>,
        /// Destination Drive folder
        #[arg(long)]
        drive_folder: Option<String>,
        /// Destination goal
        #[arg(long)]
        goal: Option<String>,
        /// Record video (true or false; defaults to the server setting)
        #[arg(long, action = clap::ArgAction::Set)]
        record_video: Option<bool>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Stop recording for a meeting-bot job
    Stop {
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

pub async fn handle_meeting(cmd: &MeetingCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        MeetingCommands::Huddle { command } => handle_huddle(command, client).await,
        MeetingCommands::Bot { command } => handle_bot(command, client).await,
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
            meeting_url,
            meeting_title,
            drive_folder,
            goal,
            record_video,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = MeetingBotJobCreateRequest {
                meeting_url: meeting_url.clone(),
                meeting_title: meeting_title.clone(),
                drive_folder_id: drive_folder.clone(),
                objective_id: goal.clone(),
                record_video: *record_video,
            };
            let job = scoped.create_meeting_bot_job(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&job)?);
            } else {
                println!("Created meeting-bot job {} [{}]", job.id, job.status);
            }
            Ok(())
        }
        BotCommands::Stop {
            id,
            org,
            force,
            json,
        } => {
            if !*force && !confirm(&format!("Stop recording for meeting-bot job {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped.stop_meeting_bot_job(id).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"stopRequested": true, "id": id})
                    )?
                );
            } else {
                println!("Stop requested for meeting-bot job {id}");
            }
            Ok(())
        }
    }
}
