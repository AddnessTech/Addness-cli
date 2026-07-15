use anyhow::{Result, bail};
use clap::{Subcommand, ValueEnum};
use serde::Serialize;
use std::io::{self, Read, Write};

use crate::api::{ApiClient, Comment, ListNotificationsParams, NotificationSettingRequest};
use crate::cli::commands::org::resolve_org_id;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum NotificationKind {
    /// General notification
    Info,
    /// Work completed
    Done,
    /// Human review or confirmation needed
    Review,
    /// Work is blocked
    Blocked,
}

impl NotificationKind {
    fn as_str(self) -> &'static str {
        match self {
            NotificationKind::Info => "info",
            NotificationKind::Done => "done",
            NotificationKind::Review => "review",
            NotificationKind::Blocked => "blocked",
        }
    }

    fn heading(self) -> Option<&'static str> {
        match self {
            NotificationKind::Info => None,
            NotificationKind::Done => Some("作業完了通知"),
            NotificationKind::Review => Some("確認依頼通知"),
            NotificationKind::Blocked => Some("ブロック通知"),
        }
    }
}

#[derive(Subcommand)]
pub enum NotificationCommands {
    /// Send a Codex work notification via a goal comment and terminal notice
    Send {
        /// Goal ID. Defaults to ADDNESS_GOAL_ID when running from the TUI codex pane.
        #[arg(long)]
        goal: Option<String>,
        /// Notification kind
        #[arg(long, value_enum, default_value_t = NotificationKind::Info)]
        kind: NotificationKind,
        /// Notification body
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read notification body from a file. Use '-' to read stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Parent comment ID for a thread reply notification
        #[arg(long)]
        parent: Option<String>,
        /// Mention member IDs (UUID), repeatable. Mentioned users get targeted notifications.
        #[arg(long)]
        mention: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List your notifications
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Max notifications to return (1-100, default 20)
        #[arg(long)]
        limit: Option<u16>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u64>,
        /// Only show unread notifications
        #[arg(long, conflicts_with = "read_only")]
        unread_only: bool,
        /// Only show already-read notifications
        #[arg(long, conflicts_with = "unread_only")]
        read_only: bool,
        /// Filter by goal ID (objective)
        #[arg(long)]
        goal: Option<String>,
        /// Filter by category (mention, comment, reply, reaction, assignment, goal, ai).
        /// Comma-separated or repeatable.
        #[arg(long)]
        category: Vec<String>,
        /// Sort order: asc (oldest first) or desc (newest first, default)
        #[arg(long)]
        sort: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show unread notification counts (overall and by category)
    Count {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Filter by category. Comma-separated or repeatable.
        #[arg(long)]
        category: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show unread notification counts grouped by goal
    CountsByGoal {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark specific notifications as read
    MarkRead {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Comma-separated notification IDs to mark as read
        #[arg(long)]
        ids: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark specific notifications as unread
    MarkUnread {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Comma-separated notification IDs to mark as unread
        #[arg(long)]
        ids: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark all notifications as read
    MarkAllRead {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage notification subscription channels (Slack/Email/LINE/Discord)
    Subscription {
        #[command(subcommand)]
        command: SubscriptionCommands,
    },
}

/// Notification delivery channel for `notification subscription` settings.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum NotificationProvider {
    Slack,
    Email,
    Line,
    Discord,
}

impl NotificationProvider {
    fn as_str(self) -> &'static str {
        match self {
            NotificationProvider::Slack => "slack",
            NotificationProvider::Email => "email",
            NotificationProvider::Line => "line",
            NotificationProvider::Discord => "discord",
        }
    }
}

#[derive(Subcommand)]
pub enum SubscriptionCommands {
    /// List your notification subscription settings
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Add a new subscription channel (active by default)
    Add {
        /// Delivery channel
        #[arg(long, value_enum)]
        provider: NotificationProvider,
        /// Email address (required for --provider email while active)
        #[arg(long)]
        email: Option<String>,
        /// Create the setting disabled instead of active
        #[arg(long)]
        inactive: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update an existing subscription setting
    Update {
        /// Notification setting ID
        id: String,
        /// Delivery channel
        #[arg(long, value_enum)]
        provider: NotificationProvider,
        /// Email address (required for --provider email while active)
        #[arg(long)]
        email: Option<String>,
        /// Disable the setting instead of activating it
        #[arg(long)]
        inactive: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List email addresses registered as notification destinations
    EmailDestinations {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NotificationSendOutput<'a> {
    delivered_via: &'a str,
    kind: &'a str,
    terminal_notification: &'a str,
    goal_id: &'a str,
    comment: &'a Comment,
}

fn read_body(inline: Option<&String>, file: Option<&String>) -> Result<String> {
    match (inline, file) {
        (Some(s), None) if s == "-" => read_stdin(),
        (Some(s), None) => Ok(s.clone()),
        (None, Some(p)) if p == "-" => read_stdin(),
        (None, Some(p)) => Ok(std::fs::read_to_string(p)?),
        (Some(_), Some(_)) => bail!("Specify only one of --body or --body-file"),
        (None, None) => bail!("Specify --body or --body-file"),
    }
}

fn read_stdin() -> Result<String> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

fn ensure_body(content: String) -> Result<String> {
    if content.trim().is_empty() {
        bail!(
            "Notification body is empty. Specify --body, --body-file, or pipe content with --body -."
        );
    }
    if content.chars().count() > 10_000 {
        bail!("Notification body must be 10000 characters or less.");
    }
    Ok(content)
}

fn format_notification_body(kind: NotificationKind, body: &str) -> Result<String> {
    let body = body.trim();
    let content = match kind.heading() {
        Some(heading) => format!("【{heading}】\n{body}"),
        None => body.to_string(),
    };
    if content.chars().count() > 10_000 {
        bail!("Notification body must be 10000 characters or less after adding the kind heading.");
    }
    Ok(content)
}

fn terminal_title(kind: NotificationKind) -> &'static str {
    kind.heading().unwrap_or("Addness通知")
}

fn terminal_message(body: &str) -> String {
    truncate_chars(&sanitize_terminal_notification_text(body), 240)
}

fn sanitize_terminal_notification_text(input: &str) -> String {
    input
        .chars()
        .filter_map(|ch| match ch {
            '\n' | '\r' | '\t' => Some(' '),
            ';' => Some(' '),
            ch if ch.is_control() => None,
            ch => Some(ch),
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }

    let keep = max_chars.saturating_sub(3);
    let mut out = input.chars().take(keep).collect::<String>();
    out.push_str("...");
    out
}

fn send_terminal_notification(kind: NotificationKind, body: &str) -> &'static str {
    let title = terminal_title(kind);
    let message = terminal_message(body);
    if message.is_empty() {
        return "skipped";
    }

    let title = sanitize_terminal_notification_text(title);
    let mut stderr = io::stderr();
    let result = write!(
        stderr,
        "\x07\x1b]9;{message}\x07\x1b]777;notify;{title};{message}\x07"
    )
    .and_then(|_| stderr.flush());
    if result.is_ok() { "sent" } else { "failed" }
}

fn resolve_goal_id(goal: Option<&String>) -> Result<String> {
    if let Some(goal) = goal
        && !goal.trim().is_empty()
    {
        return Ok(goal.clone());
    }

    if let Ok(goal) = std::env::var("ADDNESS_GOAL_ID")
        && !goal.trim().is_empty()
    {
        return Ok(goal);
    }

    bail!("Specify --goal <GOAL_ID> or run from an Addness TUI codex session.");
}

/// Comma-separated IDのリストをトリム済みの `Vec<String>` に分割する
/// （複数指定可能な `--ids`/`--category` 引数の共通処理）。
fn split_csv(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// `--category` は繰り返し指定・カンマ区切りの両方を許容する
/// （バックエンドの `?category=a&category=b` / `?category=a,b` 両対応に合わせる）。
fn flatten_categories(raw: &[String]) -> Vec<String> {
    raw.iter().flat_map(|s| split_csv(s)).collect()
}

fn resolve_read_filter(unread_only: bool, read_only: bool) -> Option<bool> {
    if unread_only {
        Some(false)
    } else if read_only {
        Some(true)
    } else {
        None
    }
}

fn build_notification_setting_request(
    provider: NotificationProvider,
    email: Option<&String>,
    inactive: bool,
) -> Result<NotificationSettingRequest> {
    let active = !inactive;
    if matches!(provider, NotificationProvider::Email) && active && email.is_none() {
        bail!("--email is required when --provider email and the setting is active.");
    }
    Ok(NotificationSettingRequest {
        provider: provider.as_str().to_string(),
        active,
        email: email.cloned(),
    })
}

pub async fn handle_notification(cmd: &NotificationCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        NotificationCommands::Send {
            goal,
            kind,
            body,
            body_file,
            parent,
            mention,
            json,
        } => {
            let goal_id = resolve_goal_id(goal.as_ref())?;
            let raw_content = ensure_body(read_body(body.as_ref(), body_file.as_ref())?)?;
            let content = format_notification_body(*kind, &raw_content)?;
            let comment = client
                .create_comment_with_options(&goal_id, &content, parent.clone(), mention.clone())
                .await?;
            let terminal_notification = send_terminal_notification(*kind, &raw_content);

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&NotificationSendOutput {
                        delivered_via: "comment",
                        kind: kind.as_str(),
                        terminal_notification,
                        goal_id: &goal_id,
                        comment: &comment,
                    })?
                );
            } else {
                let label = kind.heading().unwrap_or("通知");
                println!("{label}を送信しました: {}", comment.id);
                println!("Delivery: goal comment");
                println!("Terminal notification: {terminal_notification}");
                println!("Goal: {goal_id}");
            }
            Ok(())
        }
        NotificationCommands::List {
            org,
            limit,
            offset,
            unread_only,
            read_only,
            goal,
            category,
            sort,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let categories = flatten_categories(category);
            let resp = client
                .list_notifications(
                    &org_id,
                    ListNotificationsParams {
                        limit: *limit,
                        offset: *offset,
                        read: resolve_read_filter(*unread_only, *read_only),
                        goal_id: goal.as_deref(),
                        categories: &categories,
                        sort: sort.as_deref(),
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.notifications.is_empty() {
                println!("No notifications.");
            } else {
                for n in &resp.notifications {
                    let status = if n.read_at.is_some() {
                        "read"
                    } else {
                        "unread"
                    };
                    let title = n.subject_title.as_deref().unwrap_or("-");
                    let ids = n.notification_ids.join(",");
                    println!("[{status}] {} - {title} (ids: {ids})", n.event_type);
                }
                if resp.has_more {
                    println!("More notifications available (use --offset or --limit).");
                }
            }
            Ok(())
        }
        NotificationCommands::Count {
            org,
            category,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let categories = flatten_categories(category);
            let resp = client.count_notifications(&org_id, &categories).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Unread: {}", resp.unread_count);
                for (cat, count) in &resp.unread_by_category {
                    println!("  {cat}: {count}");
                }
            }
            Ok(())
        }
        NotificationCommands::CountsByGoal { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client.count_notifications_by_goal(&org_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.counts.is_empty() {
                println!("No unread notifications.");
            } else {
                for (goal_id, counts) in &resp.counts {
                    println!(
                        "{goal_id}: total={} comments={} deliverables={} assignments={} childActivity={}",
                        counts.total,
                        counts.comments,
                        counts.deliverables,
                        counts.assignments,
                        counts.child_activity
                    );
                }
            }
            Ok(())
        }
        NotificationCommands::MarkRead { org, ids, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let id_list = split_csv(ids);
            if id_list.is_empty() {
                bail!("--ids must contain at least one notification ID");
            }
            let resp = client.mark_notifications_read(&org_id, &id_list).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!(
                    "Marked {} notification(s) as read. Unread remaining: {}",
                    resp.marked_count, resp.unread_count
                );
            }
            Ok(())
        }
        NotificationCommands::MarkUnread { org, ids, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let id_list = split_csv(ids);
            if id_list.is_empty() {
                bail!("--ids must contain at least one notification ID");
            }
            let resp = client.mark_notifications_unread(&org_id, &id_list).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!(
                    "Marked {} notification(s) as unread. Unread remaining: {}",
                    resp.marked_count, resp.unread_count
                );
            }
            Ok(())
        }
        NotificationCommands::MarkAllRead { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client.mark_all_notifications_read(&org_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!(
                    "Marked {} notification(s) as read. Unread remaining: {}",
                    resp.marked_count, resp.unread_count
                );
            }
            Ok(())
        }
        NotificationCommands::Subscription { command } => {
            handle_subscription(command, client).await
        }
    }
}

pub async fn handle_subscription(cmd: &SubscriptionCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        SubscriptionCommands::List { json } => {
            let settings = client.list_notification_settings().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&settings)?);
            } else if settings.is_empty() {
                println!("No notification subscription settings.");
            } else {
                for s in &settings {
                    let active = if s.active { "active" } else { "inactive" };
                    println!("{} [{active}] provider={}", s.id, s.provider);
                }
            }
            Ok(())
        }
        SubscriptionCommands::Add {
            provider,
            email,
            inactive,
            json,
        } => {
            let req = build_notification_setting_request(*provider, email.as_ref(), *inactive)?;
            let setting = client.create_notification_setting(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&setting)?);
            } else {
                let active = if setting.active { "active" } else { "inactive" };
                println!(
                    "Created subscription setting {} ({} [{active}])",
                    setting.id, setting.provider
                );
            }
            Ok(())
        }
        SubscriptionCommands::Update {
            id,
            provider,
            email,
            inactive,
            json,
        } => {
            let req = build_notification_setting_request(*provider, email.as_ref(), *inactive)?;
            let setting = client.update_notification_setting(id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&setting)?);
            } else {
                let active = if setting.active { "active" } else { "inactive" };
                println!(
                    "Updated subscription setting {} ({} [{active}])",
                    setting.id, setting.provider
                );
            }
            Ok(())
        }
        SubscriptionCommands::EmailDestinations { json } => {
            let destinations = client.list_email_destinations().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&destinations)?);
            } else if destinations.is_empty() {
                println!("No email destinations registered.");
            } else {
                for d in &destinations {
                    println!("{} {}", d.id, d.email);
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NotificationKind, NotificationProvider, build_notification_setting_request,
        flatten_categories, format_notification_body, resolve_goal_id, resolve_read_filter,
        split_csv, terminal_message,
    };

    #[test]
    fn resolve_goal_id_prefers_explicit_goal() {
        assert_eq!(
            resolve_goal_id(Some(&"goal-explicit".to_string())).unwrap(),
            "goal-explicit"
        );
    }

    #[test]
    fn split_csv_trims_and_drops_empty_entries() {
        assert_eq!(
            split_csv(" id-1, id-2 ,,id-3"),
            vec!["id-1", "id-2", "id-3"]
        );
    }

    #[test]
    fn flatten_categories_supports_repeated_and_comma_separated_forms() {
        let raw = vec!["mention,reply".to_string(), "goal".to_string()];
        assert_eq!(flatten_categories(&raw), vec!["mention", "reply", "goal"]);
    }

    #[test]
    fn resolve_read_filter_prefers_unread_only() {
        assert_eq!(resolve_read_filter(true, false), Some(false));
        assert_eq!(resolve_read_filter(false, true), Some(true));
        assert_eq!(resolve_read_filter(false, false), None);
    }

    #[test]
    fn build_notification_setting_request_defaults_to_active() {
        let req =
            build_notification_setting_request(NotificationProvider::Slack, None, false).unwrap();
        assert_eq!(req.provider, "slack");
        assert!(req.active);
        assert!(req.email.is_none());
    }

    #[test]
    fn build_notification_setting_request_requires_email_for_active_email_provider() {
        let err = build_notification_setting_request(NotificationProvider::Email, None, false)
            .unwrap_err();
        assert!(err.to_string().contains("--email"));
    }

    #[test]
    fn build_notification_setting_request_allows_inactive_email_without_address() {
        let req =
            build_notification_setting_request(NotificationProvider::Email, None, true).unwrap();
        assert!(!req.active);
        assert!(req.email.is_none());
    }

    #[test]
    fn build_notification_setting_request_accepts_email_when_active() {
        let email = "user@example.com".to_string();
        let req =
            build_notification_setting_request(NotificationProvider::Email, Some(&email), false)
                .unwrap();
        assert!(req.active);
        assert_eq!(req.email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn format_notification_body_prefixes_done_kind() {
        assert_eq!(
            format_notification_body(NotificationKind::Done, "完了しました").unwrap(),
            "【作業完了通知】\n完了しました"
        );
    }

    #[test]
    fn format_notification_body_keeps_info_plain() {
        assert_eq!(
            format_notification_body(NotificationKind::Info, "確認お願いします").unwrap(),
            "確認お願いします"
        );
    }

    #[test]
    fn terminal_message_collapses_and_truncates() {
        let message = terminal_message("  作業が\n\n完了しました  ");

        assert_eq!(message, "作業が 完了しました");
    }

    #[test]
    fn terminal_message_strips_control_sequences() {
        let message = terminal_message("確認\x1b]9;bad\x07お願いします");

        assert_eq!(message, "確認]9 badお願いします");
    }
}
