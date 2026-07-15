use anyhow::{Result, bail};
use clap::{Subcommand, ValueEnum};
use colored::Colorize;

use crate::api::{ApiClient, GoalSection, GoalSectionListParams, IssueListParams, IssueMessage};

use super::comment::read_body;

/// Maximum content length (in characters) accepted by the goal-issue backend
/// (domain/chatmessage MaxContentRunes).
const MAX_ISSUE_CONTENT_CHARS: usize = 4000;

/// Maximum number of IDs per preview request (usecase.MaxPreviewMessages).
const MAX_PREVIEW_IDS: usize = 50;

/// Scope filter accepted by GET /api/v2/goal-sections.
/// Omitted = incomplete goals only (organization-wide).
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SectionScope {
    /// Only goals shown on the today-todo screen
    Today,
    /// All goals including completed ones, ordered by activity
    All,
}

impl SectionScope {
    fn as_str(self) -> &'static str {
        match self {
            SectionScope::Today => "today",
            SectionScope::All => "all",
        }
    }
}

#[derive(Subcommand)]
pub enum IssueCommands {
    /// List issues (chat threads) on a goal
    List {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Filter by resolved status
        #[arg(long)]
        resolved: Option<bool>,
        /// Max number of issues to return
        #[arg(long)]
        limit: Option<u16>,
        /// Keyset cursor: return issues active before this RFC3339 timestamp
        #[arg(long)]
        before: Option<String>,
        /// Keyset cursor: message ID paired with --before
        #[arg(long)]
        before_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List issues across all goals you can access
    ListAll {
        /// Filter by resolved status
        #[arg(long)]
        resolved: Option<bool>,
        /// Max number of issues to return
        #[arg(long)]
        limit: Option<u16>,
        /// Keyset cursor: return issues active before this RFC3339 timestamp
        #[arg(long)]
        before: Option<String>,
        /// Keyset cursor: message ID paired with --before
        #[arg(long)]
        before_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create an issue (root chat message) on a goal
    Create {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Issue body
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read issue body from a file. Use '-' to read stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Mention organization member IDs (UUID), repeatable
        #[arg(long)]
        mention: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Edit an issue's content
    Update {
        /// Issue ID (root message ID)
        id: String,
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// New issue body
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// New issue body from a file. Use '-' to read stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark an issue as read
    Read {
        /// Issue ID (root message ID)
        id: String,
        /// Goal ID
        #[arg(long)]
        goal: String,
    },
    /// List messages in an issue thread
    Messages {
        /// Issue ID (root message ID)
        id: String,
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Max number of messages to return
        #[arg(long)]
        limit: Option<u16>,
        /// Keyset cursor: return messages created before this RFC3339 timestamp
        #[arg(long)]
        before: Option<String>,
        /// Keyset cursor: message ID paired with --before
        #[arg(long)]
        before_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Post a reply message to an issue thread
    Reply {
        /// Issue ID (root message ID)
        id: String,
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Message body
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read message body from a file. Use '-' to read stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Mention organization member IDs (UUID), repeatable
        #[arg(long)]
        mention: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Edit a message in an issue thread
    EditMessage {
        /// Message ID
        id: String,
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Issue ID (root message ID)
        #[arg(long)]
        issue: String,
        /// New message body
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// New message body from a file. Use '-' to read stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Add an emoji reaction to an issue message
    React {
        /// Message ID (use the issue ID to react to the root message)
        id: String,
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Issue ID (root message ID)
        #[arg(long)]
        issue: String,
        /// Emoji (e.g. 👍)
        #[arg(long)]
        emoji: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove your emoji reaction from an issue message
    Unreact {
        /// Message ID
        id: String,
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Issue ID (root message ID)
        #[arg(long)]
        issue: String,
        /// Emoji to remove
        #[arg(long)]
        emoji: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// List member IDs who reacted to an issue message with an emoji
    Reactions {
        /// Message ID
        id: String,
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Issue ID (root message ID)
        #[arg(long)]
        issue: String,
        /// Emoji to look up
        #[arg(long)]
        emoji: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Search issue messages across goals
    Search {
        /// Search query
        #[arg(long)]
        query: String,
        /// Max number of messages to return
        #[arg(long)]
        limit: Option<u16>,
        /// Keyset cursor: return messages created before this RFC3339 timestamp
        #[arg(long)]
        before: Option<String>,
        /// Keyset cursor: message ID paired with --before
        #[arg(long)]
        before_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Preview issue messages by ID (quote cards; max 50 IDs)
    Preview {
        /// Message ID (repeatable)
        #[arg(long = "id", required = true)]
        ids: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark an issue as resolved
    Resolve {
        /// Issue ID (root message ID)
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark an issue as unresolved
    Unresolve {
        /// Issue ID (root message ID)
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Goal sections (per-goal chat overview, pins, unread counters)
    Sections {
        #[command(subcommand)]
        command: SectionCommands,
    },
}

#[derive(Subcommand)]
pub enum SectionCommands {
    /// List goal sections (paged activity feed)
    List {
        /// Scope: today (today-todo goals) or all (including completed)
        #[arg(long, value_enum)]
        scope: Option<SectionScope>,
        /// Filter sections by issue resolved status
        #[arg(long)]
        resolved: Option<bool>,
        /// Exclude goals without any issues
        #[arg(long)]
        has_comments: bool,
        /// Max number of sections to return
        #[arg(long)]
        limit: Option<u16>,
        /// Continuation cursor: has_unread echo value from next_cursor
        #[arg(long)]
        has_unread: Option<bool>,
        /// Continuation cursor: section_before from next_cursor (RFC3339)
        #[arg(long)]
        section_before: Option<String>,
        /// Continuation cursor: section_before_id from next_cursor (goal ID)
        #[arg(long)]
        section_before_id: Option<String>,
        /// Continuation cursor: unread_as_of from next_cursor (RFC3339)
        #[arg(long)]
        unread_as_of: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List pinned goal sections
    Pinned {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the unread issue-comment count across goals
    UnreadCount {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the unread mention count across goals
    UnreadMentions {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Pin a goal section
    Pin {
        /// Goal ID
        goal_id: String,
    },
    /// Unpin a goal section
    Unpin {
        /// Goal ID
        goal_id: String,
    },
}

/// Validate issue/message body content (non-empty, within backend limit).
fn ensure_issue_body(content: String) -> Result<String> {
    if content.trim().is_empty() {
        bail!("Message body is empty. Specify --body, --body-file, or pipe content with --body -.");
    }
    if content.chars().count() > MAX_ISSUE_CONTENT_CHARS {
        bail!("Message body must be {MAX_ISSUE_CONTENT_CHARS} characters or less.");
    }
    Ok(content)
}

/// Validate preview IDs (at least one, at most the backend maximum).
fn ensure_preview_ids(ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        bail!("Specify at least one --id.");
    }
    if ids.len() > MAX_PREVIEW_IDS {
        bail!("--id accepts at most {MAX_PREVIEW_IDS} values per request.");
    }
    Ok(())
}

/// Validate the goal-sections continuation cursor: the backend requires all of
/// has_unread / section_before / section_before_id / unread_as_of together on
/// continuation pages, and none of them on the first page.
fn ensure_section_cursor(
    has_unread: Option<bool>,
    section_before: Option<&str>,
    section_before_id: Option<&str>,
    unread_as_of: Option<&str>,
) -> Result<()> {
    let given = [
        has_unread.is_some(),
        section_before.is_some(),
        section_before_id.is_some(),
        unread_as_of.is_some(),
    ];
    let count = given.iter().filter(|&&g| g).count();
    if count != 0 && count != given.len() {
        bail!(
            "Continuation requires all of --has-unread, --section-before, \
             --section-before-id, and --unread-as-of (copy them from next_cursor)."
        );
    }
    Ok(())
}

fn truncate_content(content: &str, max_chars: usize) -> String {
    let flattened = content.replace('\n', " ");
    if flattened.chars().count() > max_chars {
        let truncated: String = flattened
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect();
        format!("{truncated}...")
    } else {
        flattened
    }
}

fn print_issues_table(issues: &[IssueMessage]) {
    if issues.is_empty() {
        println!("{}", "No issues found.".dimmed());
        return;
    }

    println!(
        "{:<38} {:>5} {:<10} {:>7} {:<12} {}",
        "ID".bold(),
        "#".bold(),
        "RESOLVED".bold(),
        "REPLIES".bold(),
        "UPDATED".bold(),
        "CONTENT".bold()
    );
    println!("{}", "─".repeat(120));

    for issue in issues {
        let resolved = if issue.resolved_at.is_some() {
            "yes".green().to_string()
        } else {
            "no".to_string()
        };
        let date = &issue.last_activity_at[..10.min(issue.last_activity_at.len())];
        println!(
            "{:<38} {:>5} {:<10} {:>7} {:<12} {}",
            issue.id.dimmed(),
            issue.issue_number,
            resolved,
            issue.reply_count,
            date.dimmed(),
            truncate_content(&issue.content, 50)
        );
    }
}

fn print_message_line(message: &IssueMessage) {
    let date = &message.created_at[..10.min(message.created_at.len())];
    let sender = message.sender_id.as_deref().unwrap_or("-");
    println!(
        "{:<38} {:<38} {:<12} {}",
        message.id.dimmed(),
        sender,
        date.dimmed(),
        truncate_content(&message.content, 50)
    );
}

fn print_sections_table(sections: &[GoalSection]) {
    if sections.is_empty() {
        println!("{}", "No goal sections found.".dimmed());
        return;
    }

    println!(
        "{:<38} {:>7} {:>7} {:<14} {:<12} {}",
        "GOAL ID".bold(),
        "ISSUES".bold(),
        "UNREAD".bold(),
        "STATUS".bold(),
        "ACTIVITY".bold(),
        "TITLE".bold()
    );
    println!("{}", "─".repeat(120));

    for section in sections {
        let date = &section.last_activity_at[..10.min(section.last_activity_at.len())];
        println!(
            "{:<38} {:>7} {:>7} {:<14} {:<12} {}",
            section.objective_id.dimmed(),
            section.issue_count,
            section.unread_count,
            section.status,
            date.dimmed(),
            section.objective_title
        );
    }
}

pub async fn handle_issue(cmd: &IssueCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        IssueCommands::List {
            goal,
            resolved,
            limit,
            before,
            before_id,
            json,
        } => {
            let data = client
                .list_objective_issues(
                    goal,
                    IssueListParams {
                        resolved: *resolved,
                        limit: *limit,
                        before: before.as_deref(),
                        before_id: before_id.as_deref(),
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&data.issues)?);
            } else {
                print_issues_table(&data.issues);
            }
            Ok(())
        }
        IssueCommands::ListAll {
            resolved,
            limit,
            before,
            before_id,
            json,
        } => {
            let data = client
                .list_all_issues(IssueListParams {
                    resolved: *resolved,
                    limit: *limit,
                    before: before.as_deref(),
                    before_id: before_id.as_deref(),
                })
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&data.issues)?);
            } else {
                print_issues_table(&data.issues);
            }
            Ok(())
        }
        IssueCommands::Create {
            goal,
            body,
            body_file,
            mention,
            json,
        } => {
            let content = ensure_issue_body(read_body(body.as_ref(), body_file.as_ref())?)?;
            let issue = client.create_issue(goal, &content, mention.clone()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&issue)?);
            } else {
                println!("Issue created: {} (#{})", issue.id, issue.issue_number);
            }
            Ok(())
        }
        IssueCommands::Update {
            id,
            goal,
            body,
            body_file,
            json,
        } => {
            let content = ensure_issue_body(read_body(body.as_ref(), body_file.as_ref())?)?;
            let issue = client.edit_issue(goal, id, &content).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&issue)?);
            } else {
                println!("Issue updated: {}", issue.id);
            }
            Ok(())
        }
        IssueCommands::Read { id, goal } => {
            client.mark_issue_read(goal, id).await?;
            println!("Issue {id} marked as read");
            Ok(())
        }
        IssueCommands::Messages {
            id,
            goal,
            limit,
            before,
            before_id,
            json,
        } => {
            let thread = client
                .list_issue_messages(
                    goal,
                    id,
                    IssueListParams {
                        resolved: None,
                        limit: *limit,
                        before: before.as_deref(),
                        before_id: before_id.as_deref(),
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&thread)?);
            } else {
                println!("Issue #{}: {}", thread.issue.issue_number, thread.issue.id);
                println!("{}", truncate_content(&thread.issue.content, 100));
                if thread.messages.is_empty() {
                    println!("{}", "No replies.".dimmed());
                } else {
                    println!();
                    println!(
                        "{:<38} {:<38} {:<12} {}",
                        "MESSAGE ID".bold(),
                        "SENDER".bold(),
                        "DATE".bold(),
                        "CONTENT".bold()
                    );
                    println!("{}", "─".repeat(120));
                    for message in &thread.messages {
                        print_message_line(message);
                    }
                }
            }
            Ok(())
        }
        IssueCommands::Reply {
            id,
            goal,
            body,
            body_file,
            mention,
            json,
        } => {
            let content = ensure_issue_body(read_body(body.as_ref(), body_file.as_ref())?)?;
            let message = client
                .post_issue_message(goal, id, &content, mention.clone())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&message)?);
            } else {
                println!("Message posted: {}", message.id);
            }
            Ok(())
        }
        IssueCommands::EditMessage {
            id,
            goal,
            issue,
            body,
            body_file,
            json,
        } => {
            let content = ensure_issue_body(read_body(body.as_ref(), body_file.as_ref())?)?;
            let message = client.edit_issue_message(goal, issue, id, &content).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&message)?);
            } else {
                println!("Message updated: {}", message.id);
            }
            Ok(())
        }
        IssueCommands::React {
            id,
            goal,
            issue,
            emoji,
            json,
        } => {
            let message = client.add_issue_reaction(goal, issue, id, emoji).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&message)?);
            } else {
                println!("Reacted {emoji} on message {id}");
            }
            Ok(())
        }
        IssueCommands::Unreact {
            id,
            goal,
            issue,
            emoji,
            force,
        } => {
            if !*force
                && !crate::cli::commands::confirm(&format!(
                    "Remove your {emoji} reaction from message {id}?"
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            client.remove_issue_reaction(goal, issue, id, emoji).await?;
            println!("Removed {emoji} reaction from message {id}");
            Ok(())
        }
        IssueCommands::Reactions {
            id,
            goal,
            issue,
            emoji,
            json,
        } => {
            let users = client
                .list_issue_reaction_users(goal, issue, id, emoji)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&users)?);
            } else if users.member_ids.is_empty() {
                println!("No reactions with {emoji} on message {id}.");
            } else {
                for member_id in &users.member_ids {
                    println!("{member_id}");
                }
            }
            Ok(())
        }
        IssueCommands::Search {
            query,
            limit,
            before,
            before_id,
            json,
        } => {
            let data = client
                .search_issue_messages(
                    query,
                    IssueListParams {
                        resolved: None,
                        limit: *limit,
                        before: before.as_deref(),
                        before_id: before_id.as_deref(),
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                if !data.goals.is_empty() {
                    println!("{}", "Matching goals:".bold());
                    for goal in &data.goals {
                        println!("{} — {}", goal.objective_id.dimmed(), goal.objective_title);
                    }
                    println!();
                }
                if data.messages.is_empty() {
                    println!("{}", "No messages found.".dimmed());
                } else {
                    println!(
                        "{:<38} {:<38} {:<12} {}",
                        "MESSAGE ID".bold(),
                        "SENDER".bold(),
                        "DATE".bold(),
                        "CONTENT".bold()
                    );
                    println!("{}", "─".repeat(120));
                    for message in &data.messages {
                        print_message_line(message);
                    }
                }
            }
            Ok(())
        }
        IssueCommands::Preview { ids, json } => {
            ensure_preview_ids(ids)?;
            let preview = client.preview_issue_messages(ids.clone()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&preview)?);
            } else if let Some(messages) = preview.get("messages").and_then(|m| m.as_array()) {
                for message in messages {
                    let id = message.get("id").and_then(|v| v.as_str()).unwrap_or("-");
                    let available = message
                        .get("available")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if available {
                        let content = message
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        println!("{} — {}", id.dimmed(), truncate_content(content, 60));
                    } else {
                        println!("{} — {}", id.dimmed(), "(not available)".dimmed());
                    }
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&preview)?);
            }
            Ok(())
        }
        IssueCommands::Resolve { id, json } => {
            let message = client.set_issue_resolution(id, true).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&message)?);
            } else {
                println!("Issue {id} resolved");
            }
            Ok(())
        }
        IssueCommands::Unresolve { id, json } => {
            let message = client.set_issue_resolution(id, false).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&message)?);
            } else {
                println!("Issue {id} unresolved");
            }
            Ok(())
        }
        IssueCommands::Sections { command } => handle_sections(command, client).await,
    }
}

async fn handle_sections(cmd: &SectionCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        SectionCommands::List {
            scope,
            resolved,
            has_comments,
            limit,
            has_unread,
            section_before,
            section_before_id,
            unread_as_of,
            json,
        } => {
            ensure_section_cursor(
                *has_unread,
                section_before.as_deref(),
                section_before_id.as_deref(),
                unread_as_of.as_deref(),
            )?;
            let page = client
                .list_goal_sections(GoalSectionListParams {
                    resolved: *resolved,
                    limit: *limit,
                    scope: scope.map(SectionScope::as_str),
                    has_comments: *has_comments,
                    has_unread: *has_unread,
                    section_before: section_before.as_deref(),
                    section_before_id: section_before_id.as_deref(),
                    unread_as_of: unread_as_of.as_deref(),
                })
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&page)?);
            } else {
                print_sections_table(&page.sections);
                if let Some(cursor) = &page.next_cursor {
                    println!();
                    println!(
                        "More sections available. Next page: --has-unread {} \
                         --section-before {} --section-before-id {} --unread-as-of {}",
                        cursor.has_unread,
                        cursor.section_before,
                        cursor.section_before_id,
                        cursor.unread_as_of
                    );
                }
            }
            Ok(())
        }
        SectionCommands::Pinned { json } => {
            let data = client.list_pinned_goal_sections().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&data.sections)?);
            } else {
                print_sections_table(&data.sections);
            }
            Ok(())
        }
        SectionCommands::UnreadCount { json } => {
            let count = client.count_unread_issue_comments().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&count)?);
            } else {
                println!("Unread issue comments: {}", count.unread_comment_count);
            }
            Ok(())
        }
        SectionCommands::UnreadMentions { json } => {
            let count = client.count_unread_issue_mentions().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&count)?);
            } else {
                println!("Unread mentions: {}", count.count);
            }
            Ok(())
        }
        SectionCommands::Pin { goal_id } => {
            client.set_goal_section_pinned(goal_id, true).await?;
            println!("Goal {goal_id} pinned");
            Ok(())
        }
        SectionCommands::Unpin { goal_id } => {
            client.set_goal_section_pinned(goal_id, false).await?;
            println!("Goal {goal_id} unpinned");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SectionScope, ensure_issue_body, ensure_preview_ids, ensure_section_cursor,
        truncate_content,
    };

    #[test]
    fn section_scope_as_str_maps_to_backend_values() {
        assert_eq!(SectionScope::Today.as_str(), "today");
        assert_eq!(SectionScope::All.as_str(), "all");
    }

    #[test]
    fn ensure_issue_body_rejects_empty_content() {
        assert!(ensure_issue_body("   \n".to_string()).is_err());
    }

    #[test]
    fn ensure_issue_body_accepts_content_at_limit() {
        let body = "あ".repeat(4000);
        assert_eq!(ensure_issue_body(body.clone()).unwrap(), body);
    }

    #[test]
    fn ensure_issue_body_rejects_content_over_limit() {
        let err = ensure_issue_body("あ".repeat(4001)).unwrap_err();
        assert!(err.to_string().contains("4000"));
    }

    #[test]
    fn ensure_preview_ids_rejects_empty_list() {
        assert!(ensure_preview_ids(&[]).is_err());
    }

    #[test]
    fn ensure_preview_ids_accepts_up_to_fifty() {
        let ids: Vec<String> = (0..50).map(|i| format!("id-{i}")).collect();
        assert!(ensure_preview_ids(&ids).is_ok());
    }

    #[test]
    fn ensure_preview_ids_rejects_more_than_fifty() {
        let ids: Vec<String> = (0..51).map(|i| format!("id-{i}")).collect();
        let err = ensure_preview_ids(&ids).unwrap_err();
        assert!(err.to_string().contains("50"));
    }

    #[test]
    fn ensure_section_cursor_accepts_first_page() {
        assert!(ensure_section_cursor(None, None, None, None).is_ok());
    }

    #[test]
    fn ensure_section_cursor_accepts_full_cursor() {
        assert!(
            ensure_section_cursor(
                Some(true),
                Some("2026-07-15T00:00:00Z"),
                Some("obj-1"),
                Some("2026-07-15T00:00:00Z"),
            )
            .is_ok()
        );
    }

    #[test]
    fn ensure_section_cursor_rejects_partial_cursor() {
        let err =
            ensure_section_cursor(None, Some("2026-07-15T00:00:00Z"), None, None).unwrap_err();
        assert!(err.to_string().contains("--section-before-id"));
    }

    #[test]
    fn truncate_content_flattens_newlines_and_truncates() {
        assert_eq!(truncate_content("a\nb", 10), "a b");
        let truncated = truncate_content(&"x".repeat(100), 10);
        assert_eq!(truncated.chars().count(), 10);
        assert!(truncated.ends_with("..."));
    }
}
