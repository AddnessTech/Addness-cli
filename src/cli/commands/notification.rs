use anyhow::{Result, bail};
use clap::{Subcommand, ValueEnum};
use serde::Serialize;
use std::io::{self, Read, Write};

use crate::api::{ApiClient, Comment};

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
    }
}

#[cfg(test)]
mod tests {
    use super::{NotificationKind, format_notification_body, resolve_goal_id, terminal_message};

    #[test]
    fn resolve_goal_id_prefers_explicit_goal() {
        assert_eq!(
            resolve_goal_id(Some(&"goal-explicit".to_string())).unwrap(),
            "goal-explicit"
        );
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
