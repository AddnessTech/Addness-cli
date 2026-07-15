use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use colored::Colorize;

use crate::api::{
    ApiClient, PersonalAgentSessionCreateRequest, PersonalAgentSessionUpdateRequest,
    PersonalMarkdownEditRequest, PersonalMarkdownEditResponse, PersonalProjectCreateRequest,
    PersonalProjectUpdateRequest, PersonalTextPatchRequest,
};

/// `target` accepted by the text-patch / markdown-edit endpoints
/// (`internal/personal/usecase/markdown.go` `markdownKind`).
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PersonalTarget {
    Now,
    Day,
    Project,
}

impl PersonalTarget {
    fn as_str(self) -> &'static str {
        match self {
            PersonalTarget::Now => "now",
            PersonalTarget::Day => "day",
            PersonalTarget::Project => "project",
        }
    }
}

/// Agent session status (`internal/personal/usecase/usecase.go` `validStatus`).
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PersonalAgentSessionStatus {
    Active,
    Waiting,
    Completed,
    Archived,
}

impl PersonalAgentSessionStatus {
    fn as_str(self) -> &'static str {
        match self {
            PersonalAgentSessionStatus::Active => "active",
            PersonalAgentSessionStatus::Waiting => "waiting",
            PersonalAgentSessionStatus::Completed => "completed",
            PersonalAgentSessionStatus::Archived => "archived",
        }
    }
}

/// Project status (`internal/personal/usecase/usecase.go` `validProjectStatus`).
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PersonalProjectStatus {
    Active,
    Archived,
}

impl PersonalProjectStatus {
    fn as_str(self) -> &'static str {
        match self {
            PersonalProjectStatus::Active => "active",
            PersonalProjectStatus::Archived => "archived",
        }
    }
}

#[derive(Subcommand)]
pub enum PersonalCommands {
    /// Show your personal "now" document (scratchpad, org-independent)
    Now {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show today's personal daily-entry document
    Today {
        /// IANA timezone used to resolve "today" (default: UTC)
        #[arg(long)]
        timezone: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Append a line to today's personal daily-entry document
    TodayAppend {
        /// Text to append
        #[arg(long)]
        body: String,
        /// IANA timezone used to resolve "today" (default: UTC)
        #[arg(long)]
        timezone: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a specific day's personal daily-entry document
    Day {
        /// Local date (YYYY-MM-DD)
        date: String,
        /// IANA timezone used to interpret the date
        #[arg(long)]
        timezone: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Replace a character range of a personal document (now/day/project) by offset
    TextPatch {
        /// Document to patch
        #[arg(long, value_enum)]
        target: PersonalTarget,
        /// Project ID (required when --target project)
        #[arg(long)]
        id: Option<String>,
        /// Local date (required when --target day)
        #[arg(long)]
        date: Option<String>,
        /// IANA timezone
        #[arg(long)]
        timezone: Option<String>,
        /// Start offset (rune index, inclusive)
        #[arg(long)]
        start: i64,
        /// End offset (rune index, exclusive)
        #[arg(long)]
        end: i64,
        /// Replacement text
        #[arg(long)]
        text: String,
        /// SHA-256 hash of the document body you last read (optimistic concurrency)
        #[arg(long)]
        base_hash: String,
        /// Expected current text of the [start,end) range; rejected if it doesn't match
        #[arg(long)]
        expected: Option<String>,
        /// Preview the patch without saving it
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Analyze and edit personal documents as structured Markdown
    Markdown {
        #[command(subcommand)]
        command: MarkdownCommands,
    },
    /// Manage your personal agent sessions
    AgentSession {
        #[command(subcommand)]
        command: AgentSessionCommands,
    },
    /// Manage your personal projects
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
    /// Permanently delete your entire personal context (now/days/projects)
    Reset {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Idempotently ensure your personal organization exists (Chat/Perfect Days billing target)
    EnsureOrganization {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum MarkdownCommands {
    /// Parse a personal document into sections/list items (read-only)
    Analyze {
        /// Document to analyze
        #[arg(long, value_enum)]
        target: PersonalTarget,
        /// Project ID (required when --target project)
        #[arg(long)]
        id: Option<String>,
        /// Local date (required when --target day)
        #[arg(long)]
        date: Option<String>,
        /// IANA timezone
        #[arg(long)]
        timezone: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Replace the body of one Markdown section (matched by heading path)
    ReplaceSection {
        /// Document to edit
        #[arg(long, value_enum)]
        target: PersonalTarget,
        /// Project ID (required when --target project)
        #[arg(long)]
        id: Option<String>,
        /// Local date (required when --target day)
        #[arg(long)]
        date: Option<String>,
        /// IANA timezone
        #[arg(long)]
        timezone: Option<String>,
        /// Heading path to the target section, outermost first (repeatable)
        #[arg(long = "heading-path", required = true)]
        heading_path: Vec<String>,
        /// New section body (Markdown)
        #[arg(long)]
        content_markdown: String,
        /// SHA-256 hash of the document body you last read
        #[arg(long)]
        base_hash: String,
        /// Preview the edit without saving it
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Replace a section if it exists, otherwise create it (matched by heading path)
    UpsertSection {
        /// Document to edit
        #[arg(long, value_enum)]
        target: PersonalTarget,
        /// Project ID (required when --target project)
        #[arg(long)]
        id: Option<String>,
        /// Local date (required when --target day)
        #[arg(long)]
        date: Option<String>,
        /// IANA timezone
        #[arg(long)]
        timezone: Option<String>,
        /// Heading path to the target section, outermost first (repeatable)
        #[arg(long = "heading-path", required = true)]
        heading_path: Vec<String>,
        /// New section body (Markdown)
        #[arg(long)]
        content_markdown: String,
        /// SHA-256 hash of the document body you last read
        #[arg(long)]
        base_hash: String,
        /// Preview the edit without saving it
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Replace a list item if it exists, otherwise append it (matched by key within a section)
    UpsertListItem {
        /// Document to edit
        #[arg(long, value_enum)]
        target: PersonalTarget,
        /// Project ID (required when --target project)
        #[arg(long)]
        id: Option<String>,
        /// Local date (required when --target day)
        #[arg(long)]
        date: Option<String>,
        /// IANA timezone
        #[arg(long)]
        timezone: Option<String>,
        /// Heading path to the containing section, outermost first (repeatable)
        #[arg(long = "heading-path", required = true)]
        heading_path: Vec<String>,
        /// Stable key identifying the list item within the section
        #[arg(long)]
        key: String,
        /// New list item Markdown (e.g. "- [ ] buy milk")
        #[arg(long)]
        item_markdown: String,
        /// SHA-256 hash of the document body you last read
        #[arg(long)]
        base_hash: String,
        /// Preview the edit without saving it
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Replace the entire document body
    ReplaceDocument {
        /// Document to edit
        #[arg(long, value_enum)]
        target: PersonalTarget,
        /// Project ID (required when --target project)
        #[arg(long)]
        id: Option<String>,
        /// Local date (required when --target day)
        #[arg(long)]
        date: Option<String>,
        /// IANA timezone
        #[arg(long)]
        timezone: Option<String>,
        /// New document body (Markdown)
        #[arg(long)]
        body_markdown: String,
        /// SHA-256 hash of the document body you last read
        #[arg(long)]
        base_hash: String,
        /// Preview the edit without saving it
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Append a dated log entry under a heading (creating the heading if missing)
    AppendLogEntry {
        /// Document to edit
        #[arg(long, value_enum)]
        target: PersonalTarget,
        /// Project ID (required when --target project)
        #[arg(long)]
        id: Option<String>,
        /// Local date (required when --target day)
        #[arg(long)]
        date: Option<String>,
        /// IANA timezone
        #[arg(long)]
        timezone: Option<String>,
        /// Heading under which to append the log entry
        #[arg(long)]
        heading: String,
        /// Log entry Markdown to append
        #[arg(long)]
        content_markdown: String,
        /// SHA-256 hash of the document body you last read
        #[arg(long)]
        base_hash: String,
        /// Preview the edit without saving it
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AgentSessionCommands {
    /// List your agent sessions
    List {
        /// Filter by status
        #[arg(long, value_enum)]
        status: Option<PersonalAgentSessionStatus>,
        /// Max sessions to return (1-100, default 20)
        #[arg(long)]
        limit: Option<u16>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create an agent session
    Create {
        /// Session title
        #[arg(long)]
        title: String,
        /// Initial status (default: active)
        #[arg(long, value_enum)]
        status: Option<PersonalAgentSessionStatus>,
        /// Session body/notes
        #[arg(long, default_value = "")]
        body: String,
        /// Linked goal (objective) ID
        #[arg(long)]
        objective_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get an agent session by ID
    Get {
        /// Agent session ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update an agent session
    Update {
        /// Agent session ID
        id: String,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New status
        #[arg(long, value_enum)]
        status: Option<PersonalAgentSessionStatus>,
        /// New body/notes
        #[arg(long)]
        body: Option<String>,
        /// New linked goal (objective) ID
        #[arg(long)]
        objective_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ProjectCommands {
    /// List your personal projects
    List {
        /// Filter by status
        #[arg(long, value_enum)]
        status: Option<PersonalProjectStatus>,
        /// Max projects to return (1-100, default 20)
        #[arg(long)]
        limit: Option<u16>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a personal project
    Create {
        /// Project title
        #[arg(long)]
        title: String,
        /// Initial content (Markdown)
        #[arg(long, default_value = "")]
        content: String,
        /// Initial status (default: active)
        #[arg(long, value_enum)]
        status: Option<PersonalProjectStatus>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a personal project by ID
    Get {
        /// Project ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a personal project's title/status
    Update {
        /// Project ID
        id: String,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New status
        #[arg(long, value_enum)]
        status: Option<PersonalProjectStatus>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Validate that `--id`/`--date` were supplied when the target requires them
/// (mirrors the backend's `currentPatchText` requirement in
/// `internal/personal/usecase/patch.go`).
fn ensure_target_ref(target: PersonalTarget, id: Option<&str>, date: Option<&str>) -> Result<()> {
    match target {
        PersonalTarget::Project if id.is_none() => {
            anyhow::bail!("--target project requires --id <PROJECT_ID>")
        }
        PersonalTarget::Day if date.is_none() => {
            anyhow::bail!("--target day requires --date <YYYY-MM-DD>")
        }
        _ => Ok(()),
    }
}

fn print_markdown_edit_result(resp: &PersonalMarkdownEditResponse) {
    println!("{} ({})", resp.label.bold(), resp.target);
    if resp.dry_run {
        println!("  {}", "dry-run: not saved".dimmed());
    }
    println!("  changed: {}", resp.changed);
    println!("  changes: {}", resp.changes.len());
    if !resp.warnings.is_empty() {
        println!("  {}", "warnings:".yellow());
        for warning in &resp.warnings {
            println!("    - {} ({})", warning.message, warning.code);
        }
    }
    println!("  afterHash: {}", resp.after_hash);
}

pub async fn handle_personal(cmd: &PersonalCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        PersonalCommands::Now { json } => {
            let now = client.get_personal_now().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&now)?);
            } else {
                println!("Updated: {}", now.updated_at);
                println!("Hash: {}", now.body_hash);
                println!();
                println!("{}", now.body);
            }
            Ok(())
        }
        PersonalCommands::Today { timezone, json } => {
            let entry = client.get_personal_today(timezone.as_deref()).await?;
            print_daily_entry(&entry, *json)
        }
        PersonalCommands::TodayAppend {
            body,
            timezone,
            json,
        } => {
            let entry = client
                .append_personal_today(body, timezone.as_deref())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&entry)?);
            } else {
                println!("Appended to {} ({})", entry.local_date, entry.timezone);
            }
            Ok(())
        }
        PersonalCommands::Day {
            date,
            timezone,
            json,
        } => {
            let entry = client.get_personal_day(date, timezone.as_deref()).await?;
            print_daily_entry(&entry, *json)
        }
        PersonalCommands::TextPatch {
            target,
            id,
            date,
            timezone,
            start,
            end,
            text,
            base_hash,
            expected,
            dry_run,
            json,
        } => {
            ensure_target_ref(*target, id.as_deref(), date.as_deref())?;
            let req = PersonalTextPatchRequest {
                target: target.as_str().to_string(),
                id: id.clone(),
                date: date.clone(),
                timezone: timezone.clone(),
                start: *start,
                end: *end,
                text: text.clone(),
                base_hash: base_hash.clone(),
                expected: expected.clone(),
                dry_run: *dry_run,
            };
            let resp = client.patch_personal_text(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("{} ({})", resp.label.bold(), resp.target);
                if resp.dry_run {
                    println!("  {}", "dry-run: not saved".dimmed());
                }
                println!("  changed: {}", resp.changed);
                println!("  afterHash: {}", resp.after_hash);
            }
            Ok(())
        }
        PersonalCommands::Markdown { command } => handle_markdown(command, client).await,
        PersonalCommands::AgentSession { command } => handle_agent_session(command, client).await,
        PersonalCommands::Project { command } => handle_project(command, client).await,
        PersonalCommands::Reset { force, json } => {
            if !*force
                && !crate::cli::commands::confirm(
                    "Permanently delete your entire personal context (now/days/projects)? This cannot be undone.",
                )?
            {
                println!("Cancelled.");
                return Ok(());
            }
            let resp = client.reset_personal().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Personal context reset.");
            }
            Ok(())
        }
        PersonalCommands::EnsureOrganization { json } => {
            let resp = client.ensure_personal_organization().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Personal organization: {}", resp.organization_id);
                if let Some(balance) = &resp.balance {
                    println!(
                        "  Balance: {} / {} yen ({:.1}% remaining)",
                        balance.balance_yen, balance.cap_yen, balance.remaining_percentage
                    );
                }
            }
            Ok(())
        }
    }
}

fn print_daily_entry(entry: &crate::api::PersonalDailyEntry, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(entry)?);
    } else {
        println!("{} ({})", entry.local_date, entry.timezone);
        println!("Hash: {}", entry.body_hash);
        println!();
        println!("{}", entry.body);
    }
    Ok(())
}

async fn handle_markdown(cmd: &MarkdownCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        MarkdownCommands::Analyze {
            target,
            id,
            date,
            timezone,
            json,
        } => {
            ensure_target_ref(*target, id.as_deref(), date.as_deref())?;
            let resp = client
                .analyze_personal_markdown(
                    target.as_str(),
                    id.as_deref(),
                    date.as_deref(),
                    timezone.as_deref(),
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("{} ({})", resp.label.bold(), resp.target);
                println!("  hash: {}", resp.hash);
                println!("  sections: {}", resp.analysis.sections.len());
                println!("  listItems: {}", resp.analysis.list_items.len());
                for warning in &resp.analysis.warnings {
                    println!("  warning: {} ({})", warning.message, warning.code);
                }
            }
            Ok(())
        }
        MarkdownCommands::ReplaceSection {
            target,
            id,
            date,
            timezone,
            heading_path,
            content_markdown,
            base_hash,
            dry_run,
            json,
        } => {
            ensure_target_ref(*target, id.as_deref(), date.as_deref())?;
            let req = PersonalMarkdownEditRequest {
                target: target.as_str().to_string(),
                id: id.clone(),
                date: date.clone(),
                timezone: timezone.clone(),
                base_hash: base_hash.clone(),
                heading_path: heading_path.clone(),
                content_markdown: Some(content_markdown.clone()),
                dry_run: *dry_run,
                ..Default::default()
            };
            let resp = client.replace_personal_markdown_section(&req).await?;
            print_edit_result(&resp, *json)
        }
        MarkdownCommands::UpsertSection {
            target,
            id,
            date,
            timezone,
            heading_path,
            content_markdown,
            base_hash,
            dry_run,
            json,
        } => {
            ensure_target_ref(*target, id.as_deref(), date.as_deref())?;
            let req = PersonalMarkdownEditRequest {
                target: target.as_str().to_string(),
                id: id.clone(),
                date: date.clone(),
                timezone: timezone.clone(),
                base_hash: base_hash.clone(),
                heading_path: heading_path.clone(),
                content_markdown: Some(content_markdown.clone()),
                dry_run: *dry_run,
                ..Default::default()
            };
            let resp = client.upsert_personal_markdown_section(&req).await?;
            print_edit_result(&resp, *json)
        }
        MarkdownCommands::UpsertListItem {
            target,
            id,
            date,
            timezone,
            heading_path,
            key,
            item_markdown,
            base_hash,
            dry_run,
            json,
        } => {
            ensure_target_ref(*target, id.as_deref(), date.as_deref())?;
            let req = PersonalMarkdownEditRequest {
                target: target.as_str().to_string(),
                id: id.clone(),
                date: date.clone(),
                timezone: timezone.clone(),
                base_hash: base_hash.clone(),
                heading_path: heading_path.clone(),
                key: Some(key.clone()),
                item_markdown: Some(item_markdown.clone()),
                dry_run: *dry_run,
                ..Default::default()
            };
            let resp = client.upsert_personal_markdown_list_item(&req).await?;
            print_edit_result(&resp, *json)
        }
        MarkdownCommands::ReplaceDocument {
            target,
            id,
            date,
            timezone,
            body_markdown,
            base_hash,
            dry_run,
            json,
        } => {
            ensure_target_ref(*target, id.as_deref(), date.as_deref())?;
            let req = PersonalMarkdownEditRequest {
                target: target.as_str().to_string(),
                id: id.clone(),
                date: date.clone(),
                timezone: timezone.clone(),
                base_hash: base_hash.clone(),
                body_markdown: Some(body_markdown.clone()),
                dry_run: *dry_run,
                ..Default::default()
            };
            let resp = client.replace_personal_markdown_document(&req).await?;
            print_edit_result(&resp, *json)
        }
        MarkdownCommands::AppendLogEntry {
            target,
            id,
            date,
            timezone,
            heading,
            content_markdown,
            base_hash,
            dry_run,
            json,
        } => {
            ensure_target_ref(*target, id.as_deref(), date.as_deref())?;
            let req = PersonalMarkdownEditRequest {
                target: target.as_str().to_string(),
                id: id.clone(),
                date: date.clone(),
                timezone: timezone.clone(),
                base_hash: base_hash.clone(),
                heading: Some(heading.clone()),
                content_markdown: Some(content_markdown.clone()),
                dry_run: *dry_run,
                ..Default::default()
            };
            let resp = client.append_personal_markdown_log_entry(&req).await?;
            print_edit_result(&resp, *json)
        }
    }
}

fn print_edit_result(resp: &PersonalMarkdownEditResponse, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(resp)?);
    } else {
        print_markdown_edit_result(resp);
    }
    Ok(())
}

async fn handle_agent_session(cmd: &AgentSessionCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        AgentSessionCommands::List {
            status,
            limit,
            json,
        } => {
            let sessions = client
                .list_personal_agent_sessions(status.map(|s| s.as_str()), *limit)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&sessions)?);
            } else if sessions.is_empty() {
                println!("{}", "No agent sessions found.".dimmed());
            } else {
                for session in &sessions {
                    println!("{} — {} [{}]", session.id, session.title, session.status);
                }
            }
            Ok(())
        }
        AgentSessionCommands::Create {
            title,
            status,
            body,
            objective_id,
            json,
        } => {
            let req = PersonalAgentSessionCreateRequest {
                title: title.clone(),
                status: status
                    .map(|s| s.as_str().to_string())
                    .unwrap_or_else(|| "active".to_string()),
                body: body.clone(),
                objective_id: objective_id.clone(),
            };
            let session = client.create_personal_agent_session(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&session)?);
            } else {
                println!("Created agent session {} ({})", session.id, session.title);
            }
            Ok(())
        }
        AgentSessionCommands::Get { id, json } => {
            let session = client.get_personal_agent_session(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&session)?);
            } else {
                println!("{} — {} [{}]", session.id, session.title, session.status);
                println!("{}", session.body);
            }
            Ok(())
        }
        AgentSessionCommands::Update {
            id,
            title,
            status,
            body,
            objective_id,
            json,
        } => {
            let req = PersonalAgentSessionUpdateRequest {
                title: title.clone(),
                status: status.map(|s| s.as_str().to_string()),
                body: body.clone(),
                objective_id: objective_id.clone(),
            };
            let session = client.update_personal_agent_session(id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&session)?);
            } else {
                println!("Updated agent session {}", session.id);
            }
            Ok(())
        }
    }
}

async fn handle_project(cmd: &ProjectCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ProjectCommands::List {
            status,
            limit,
            json,
        } => {
            let projects = client
                .list_personal_projects(status.map(|s| s.as_str()), *limit)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&projects)?);
            } else if projects.is_empty() {
                println!("{}", "No projects found.".dimmed());
            } else {
                for project in &projects {
                    println!("{} — {} [{}]", project.id, project.title, project.status);
                }
            }
            Ok(())
        }
        ProjectCommands::Create {
            title,
            content,
            status,
            json,
        } => {
            let req = PersonalProjectCreateRequest {
                title: title.clone(),
                content: content.clone(),
                status: status.map(|s| s.as_str().to_string()),
            };
            let project = client.create_personal_project(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&project)?);
            } else {
                println!("Created project {} ({})", project.id, project.title);
            }
            Ok(())
        }
        ProjectCommands::Get { id, json } => {
            let project = client.get_personal_project(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&project)?);
            } else {
                println!("{} — {} [{}]", project.id, project.title, project.status);
                println!("{}", project.content);
            }
            Ok(())
        }
        ProjectCommands::Update {
            id,
            title,
            status,
            json,
        } => {
            let req = PersonalProjectUpdateRequest {
                title: title.clone(),
                status: status.map(|s| s.as_str().to_string()),
            };
            let project = client.update_personal_project(id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&project)?);
            } else {
                println!("Updated project {}", project.id);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PersonalTarget, ensure_target_ref};

    #[test]
    fn personal_target_as_str_maps_to_backend_values() {
        assert_eq!(PersonalTarget::Now.as_str(), "now");
        assert_eq!(PersonalTarget::Day.as_str(), "day");
        assert_eq!(PersonalTarget::Project.as_str(), "project");
    }

    #[test]
    fn ensure_target_ref_requires_id_for_project() {
        let err = ensure_target_ref(PersonalTarget::Project, None, None).unwrap_err();
        assert!(err.to_string().contains("--id"));
    }

    #[test]
    fn ensure_target_ref_requires_date_for_day() {
        let err = ensure_target_ref(PersonalTarget::Day, None, None).unwrap_err();
        assert!(err.to_string().contains("--date"));
    }

    #[test]
    fn ensure_target_ref_allows_now_without_id_or_date() {
        assert!(ensure_target_ref(PersonalTarget::Now, None, None).is_ok());
    }

    #[test]
    fn ensure_target_ref_accepts_project_with_id() {
        assert!(ensure_target_ref(PersonalTarget::Project, Some("p1"), None).is_ok());
    }

    #[test]
    fn ensure_target_ref_accepts_day_with_date() {
        assert!(ensure_target_ref(PersonalTarget::Day, None, Some("2026-07-15")).is_ok());
    }
}
