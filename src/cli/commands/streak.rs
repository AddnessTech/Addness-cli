use anyhow::Result;
use clap::Subcommand;

use crate::api::ApiClient;
use crate::cli::commands::member::resolve_self_member_id;
use crate::cli::commands::org::resolve_org_id;

#[derive(Subcommand)]
pub enum StreakCommands {
    /// Show a member's streak (defaults to your own)
    Get {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Member ID (defaults to yourself)
        #[arg(long)]
        member: Option<String>,
        /// Number of days in the window to show (1-90, default 7)
        #[arg(long)]
        days: Option<u32>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage your streak share link (public read-only streak page)
    Share {
        #[command(subcommand)]
        command: StreakShareCommands,
    },
    /// Freeze today's streak (protects against a miss today)
    Freeze {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Member ID (defaults to yourself; only self is allowed by the API)
        #[arg(long)]
        member: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove today's freeze
    Unfreeze {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Member ID (defaults to yourself; only self is allowed by the API)
        #[arg(long)]
        member: Option<String>,
    },
    /// Revive yesterday's broken streak (once per calendar month, no rollover)
    Revive {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Member ID (defaults to yourself; only self is allowed by the API)
        #[arg(long)]
        member: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Fetch a publicly-shared streak by its share token
    Public {
        /// Share token
        token: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum StreakShareCommands {
    /// Show your streak share status
    Status {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Member ID (defaults to yourself; only self is allowed by the API)
        #[arg(long)]
        member: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create (or re-enable) your streak share link
    Create {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Member ID (defaults to yourself; only self is allowed by the API)
        #[arg(long)]
        member: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Revoke your streak share link
    Revoke {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Member ID (defaults to yourself; only self is allowed by the API)
        #[arg(long)]
        member: Option<String>,
    },
}

/// `--member` 未指定時は自分自身のメンバーIDにフォールバックする
/// （streak/freeze, streak/share, streak/revive はAPI側でも自分専用のため）。
async fn resolve_member(
    client: &ApiClient,
    org_id: &str,
    member: Option<&String>,
) -> Result<String> {
    match member {
        Some(id) => Ok(id.clone()),
        None => resolve_self_member_id(client, org_id).await,
    }
}

pub async fn handle_streak(cmd: &StreakCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        StreakCommands::Get {
            org,
            member,
            days,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let member_id = resolve_member(client, &org_id, member.as_ref()).await?;
            let streak = client.get_streak(&org_id, &member_id, *days).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&streak)?);
            } else {
                println!(
                    "Streak for {member_id}: {} days (working days counted: {})",
                    streak.streak_count, streak.total_working_days
                );
                for day in &streak.days {
                    println!("  {} - {}", day.date, day.state.as_str());
                }
            }
            Ok(())
        }
        StreakCommands::Share { command } => handle_streak_share(command, client).await,
        StreakCommands::Freeze { org, member, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let member_id = resolve_member(client, &org_id, member.as_ref()).await?;
            let result = client.freeze_streak(&org_id, &member_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Froze streak for {member_id} on {}", result.date);
            }
            Ok(())
        }
        StreakCommands::Unfreeze { org, member } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let member_id = resolve_member(client, &org_id, member.as_ref()).await?;
            client.unfreeze_streak(&org_id, &member_id).await?;
            println!("Removed today's freeze for {member_id}");
            Ok(())
        }
        StreakCommands::Revive { org, member, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let member_id = resolve_member(client, &org_id, member.as_ref()).await?;
            let result = client.revive_streak(&org_id, &member_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!(
                    "Revived streak for {member_id}: {} restored",
                    result.revived_date
                );
            }
            Ok(())
        }
        StreakCommands::Public { token, json } => {
            let streak = client.get_public_streak(token).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&streak)?);
            } else {
                println!(
                    "{}'s streak: {} days (working days counted: {})",
                    streak.member_name, streak.streak_count, streak.total_working_days
                );
                for day in &streak.week_days {
                    println!(
                        "  {} - {}",
                        day.date,
                        public_day_state_label(day.completed, day.frozen)
                    );
                }
            }
            Ok(())
        }
    }
}

/// 公開ストリーク(`WeekDayStatus`)の completed/frozen フラグから表示ラベルを決める。
/// バックエンドの契約上 completed と frozen は同時にtrueにならないが、
/// 表示側では completed を優先する。
fn public_day_state_label(completed: bool, frozen: bool) -> &'static str {
    if completed {
        "completed"
    } else if frozen {
        "frozen"
    } else {
        "none"
    }
}

pub async fn handle_streak_share(cmd: &StreakShareCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        StreakShareCommands::Status { org, member, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let member_id = resolve_member(client, &org_id, member.as_ref()).await?;
            let status = client.get_streak_share_status(&org_id, &member_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else if status.is_public {
                println!(
                    "Streak sharing is ON for {member_id} (token: {})",
                    status.share_token.as_deref().unwrap_or("-")
                );
            } else {
                println!("Streak sharing is OFF for {member_id}");
            }
            Ok(())
        }
        StreakShareCommands::Create { org, member, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let member_id = resolve_member(client, &org_id, member.as_ref()).await?;
            let link = client.create_streak_share_link(&org_id, &member_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&link)?);
            } else {
                println!(
                    "Streak sharing enabled for {member_id}. Token: {}",
                    link.share_token
                );
            }
            Ok(())
        }
        StreakShareCommands::Revoke { org, member } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let member_id = resolve_member(client, &org_id, member.as_ref()).await?;
            client.revoke_streak_share_link(&org_id, &member_id).await?;
            println!("Streak sharing disabled for {member_id}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::public_day_state_label;

    #[test]
    fn public_day_state_label_prefers_completed() {
        assert_eq!(public_day_state_label(true, false), "completed");
        // 契約上は同時にtrueにならないが、表示側はcompletedを優先することを確認する。
        assert_eq!(public_day_state_label(true, true), "completed");
    }

    #[test]
    fn public_day_state_label_reports_frozen() {
        assert_eq!(public_day_state_label(false, true), "frozen");
    }

    #[test]
    fn public_day_state_label_defaults_to_none() {
        assert_eq!(public_day_state_label(false, false), "none");
    }
}
