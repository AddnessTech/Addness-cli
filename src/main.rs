mod api;
mod cli;
mod config;
mod debug_log;
mod tui;
mod update_check;

use anyhow::{Result, bail};
use clap::{CommandFactory, Parser, Subcommand};

use crate::config::{Credentials, DEFAULT_API_URL, Settings};
use api::ApiClient;
use cli::commands::{
    activity, assignment, comment, configure, deliverable, detect, goal, invitation, kpi, link,
    login, member, notification, org, skills, streak, summary, today, update, user,
};

#[derive(Parser)]
#[command(
    name = "addness",
    about = "Addness CLI - Manage your goals from the terminal",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Log in via browser (recommended for first setup)
    Login {
        /// API base URL
        #[arg(long, default_value = DEFAULT_API_URL)]
        api_url: String,
        /// Frontend URL (for local dev with ngrok)
        #[arg(long)]
        frontend_url: Option<String>,
    },
    /// Configure API Key, URL, and default organization manually
    Configure,
    /// Show current configuration status
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove saved credentials
    Logout,
    /// Manage organizations
    Org {
        #[command(subcommand)]
        command: org::OrgCommands,
    },
    /// Manage goals
    Goal {
        #[command(subcommand)]
        command: goal::GoalCommands,
    },
    /// Manage comments on goals
    Comment {
        #[command(subcommand)]
        command: comment::CommentCommands,
    },
    /// Link PRs/URLs to goals and track progress
    Link {
        #[command(subcommand)]
        command: link::LinkCommands,
    },
    /// Manage deliverables (text/markdown content or file uploads) on goals
    Deliverable {
        #[command(subcommand)]
        command: deliverable::DeliverableCommands,
    },
    /// Manage goal assignments (member roles: OWNER/EDITOR/MEMBER)
    Assignment {
        #[command(subcommand)]
        command: assignment::AssignmentCommands,
    },
    /// Manage KPIs on goals
    Kpi {
        #[command(subcommand)]
        command: kpi::KpiCommands,
    },
    /// Manage organization members
    Member {
        #[command(subcommand)]
        command: member::MemberCommands,
    },
    /// Manage your Addness user profile, settings, and account
    User {
        #[command(subcommand)]
        command: user::UserCommands,
    },
    /// Send notifications, manage read status, and manage subscription channels
    Notification {
        #[command(subcommand)]
        command: notification::NotificationCommands,
    },
    /// Manage invitations and invite links
    Invitation {
        #[command(subcommand)]
        command: invitation::InvitationCommands,
    },
    /// Read activity logs (per-member, per-goal, and organization/goal summaries)
    Activity {
        #[command(subcommand)]
        command: activity::ActivityCommands,
    },
    /// View and manage streaks (daily completion streaks, freeze, revive, sharing)
    Streak {
        #[command(subcommand)]
        command: streak::StreakCommands,
    },
    /// Read and write today's todos (today's goals)
    Today {
        #[command(subcommand)]
        command: Option<today::TodayCommands>,
    },
    /// Show progress summary of all goals
    Summary {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Tree depth (default: 5)
        #[arg(long, default_value = "5")]
        depth: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Detect goal ID from current git branch name
    DetectGoal {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update addness to the latest version via the official installer
    Update {
        /// Only check whether an update is available; do not install
        #[arg(long)]
        check: bool,
    },
    /// Output AI skills prompt for this CLI
    Skills,
    /// Generate shell completions
    Completions {
        /// Shell: bash, zsh, fish, powershell
        shell: clap_complete::Shell,
    },
}

impl Cli {
    fn should_check_for_update(&self) -> bool {
        match &self.command {
            Some(Commands::Completions { .. }) => false,
            Some(command) => !command_outputs_json(command),
            None => true,
        }
    }
}

fn command_outputs_json(command: &Commands) -> bool {
    match command {
        Commands::Status { json }
        | Commands::Summary { json, .. }
        | Commands::DetectGoal { json } => *json,
        Commands::Org { command } => org_outputs_json(command),
        Commands::Goal { command } => goal_outputs_json(command),
        Commands::Comment { command } => comment_outputs_json(command),
        Commands::Link { command } => link_outputs_json(command),
        Commands::Deliverable { command } => deliverable_outputs_json(command),
        Commands::Assignment { command } => assignment_outputs_json(command),
        Commands::Kpi { command } => kpi_outputs_json(command),
        Commands::Member { command } => member_outputs_json(command),
        Commands::User { command } => user_outputs_json(command),
        Commands::Notification { command } => notification_outputs_json(command),
        Commands::Invitation { command } => invitation_outputs_json(command),
        Commands::Activity { command } => activity_outputs_json(command),
        Commands::Streak { command } => streak_outputs_json(command),
        Commands::Today { command } => command.as_ref().is_some_and(today_outputs_json),
        Commands::Login { .. }
        | Commands::Configure
        | Commands::Logout
        | Commands::Update { .. }
        | Commands::Skills
        | Commands::Completions { .. } => false,
    }
}

fn org_outputs_json(command: &org::OrgCommands) -> bool {
    match command {
        org::OrgCommands::List { json }
        | org::OrgCommands::Current { json }
        | org::OrgCommands::Create { json, .. }
        | org::OrgCommands::Update { json, .. }
        | org::OrgCommands::SetContext { json, .. }
        | org::OrgCommands::Get { json, .. }
        | org::OrgCommands::ListAll { json, .. }
        | org::OrgCommands::RootOwner { json, .. }
        | org::OrgCommands::AccessibleRoot { json, .. }
        | org::OrgCommands::AiAgentMember { json, .. }
        | org::OrgCommands::AccessState { json, .. }
        | org::OrgCommands::CurrentMember { json, .. }
        | org::OrgCommands::AdminCheck { json, .. }
        | org::OrgCommands::GetContext { json, .. }
        | org::OrgCommands::ContextRevisions { json, .. }
        | org::OrgCommands::SetTimezone { json, .. }
        | org::OrgCommands::SetLogo { json, .. }
        | org::OrgCommands::PushTokenRegister { json, .. } => *json,
        org::OrgCommands::OnboardingBilling { command } => match command {
            org::OnboardingBillingCommands::State { json, .. }
            | org::OnboardingBillingCommands::Require { json, .. }
            | org::OnboardingBillingCommands::Free { json, .. } => *json,
        },
        org::OrgCommands::AiScheduleSettings { command } => match command {
            org::AiScheduleSettingsCommands::Get { json, .. }
            | org::AiScheduleSettingsCommands::Set { json, .. } => *json,
        },
        org::OrgCommands::AdSettings { command } => match command {
            org::AdSettingsCommands::Get { json, .. }
            | org::AdSettingsCommands::Set { json, .. }
            | org::AdSettingsCommands::SetMe { json, .. } => *json,
        },
        org::OrgCommands::Subscription { command } => match command {
            org::SubscriptionCommands::Register { json, .. }
            | org::SubscriptionCommands::Current { json, .. } => *json,
            org::SubscriptionCommands::Cancel { json, force, .. } => *json && *force,
        },
        org::OrgCommands::Switch { .. } | org::OrgCommands::Rm { .. } => false,
    }
}

fn goal_outputs_json(command: &goal::GoalCommands) -> bool {
    match command {
        goal::GoalCommands::List { json, .. }
        | goal::GoalCommands::Get { json, .. }
        | goal::GoalCommands::Children { json, .. }
        | goal::GoalCommands::Tree { json, .. }
        | goal::GoalCommands::Siblings { json, .. }
        | goal::GoalCommands::Search { json, .. }
        | goal::GoalCommands::Create { json, .. }
        | goal::GoalCommands::Update { json, .. }
        | goal::GoalCommands::Delete { json, .. }
        | goal::GoalCommands::Archive { json, .. }
        | goal::GoalCommands::Unarchive { json, .. }
        | goal::GoalCommands::Restore { json, .. }
        | goal::GoalCommands::Duplicate { json, .. }
        | goal::GoalCommands::Move { json, .. } => *json,
        goal::GoalCommands::Share { command } => match command {
            goal::ShareCommands::Create { json, .. } => *json,
            goal::ShareCommands::Revoke { .. } => false,
        },
        goal::GoalCommands::Alias { command } => match command {
            goal::AliasCommands::Add { json, .. } => *json,
            goal::AliasCommands::Rm { .. } | goal::AliasCommands::Reorder { .. } => false,
        },
        goal::GoalCommands::Recurring { command } => match command {
            goal::RecurringCommands::Get { json, .. }
            | goal::RecurringCommands::Set { json, .. }
            | goal::RecurringCommands::Remove { json, .. } => *json,
        },
    }
}

fn comment_outputs_json(command: &comment::CommentCommands) -> bool {
    match command {
        comment::CommentCommands::List { json, .. }
        | comment::CommentCommands::Get { json, .. }
        | comment::CommentCommands::Create { json, .. }
        | comment::CommentCommands::Update { json, .. }
        | comment::CommentCommands::Resolve { json, .. }
        | comment::CommentCommands::Unresolve { json, .. } => *json,
        comment::CommentCommands::Delete { .. }
        | comment::CommentCommands::React { .. }
        | comment::CommentCommands::Attachment { .. } => false,
    }
}

fn link_outputs_json(command: &link::LinkCommands) -> bool {
    match command {
        link::LinkCommands::Pr { json, .. } | link::LinkCommands::Progress { json, .. } => *json,
    }
}

fn deliverable_outputs_json(command: &deliverable::DeliverableCommands) -> bool {
    match command {
        deliverable::DeliverableCommands::Add { json, .. }
        | deliverable::DeliverableCommands::List { json, .. }
        | deliverable::DeliverableCommands::Update { json, .. }
        | deliverable::DeliverableCommands::Rename { json, .. }
        | deliverable::DeliverableCommands::Move { json, .. }
        | deliverable::DeliverableCommands::BatchMove { json, .. } => *json,
        deliverable::DeliverableCommands::Rm { .. }
        | deliverable::DeliverableCommands::BatchRm { .. } => false,
    }
}

fn assignment_outputs_json(command: &assignment::AssignmentCommands) -> bool {
    match command {
        assignment::AssignmentCommands::Add { json, .. }
        | assignment::AssignmentCommands::Update { json, .. }
        | assignment::AssignmentCommands::Transfer { json, .. } => *json,
        assignment::AssignmentCommands::Rm { .. } => false,
    }
}

fn kpi_outputs_json(command: &kpi::KpiCommands) -> bool {
    match command {
        kpi::KpiCommands::Add { json, .. } | kpi::KpiCommands::Update { json, .. } => *json,
        kpi::KpiCommands::Rm { .. } => false,
    }
}

fn member_outputs_json(command: &member::MemberCommands) -> bool {
    matches!(command, member::MemberCommands::List { json: true, .. })
}

fn user_outputs_json(command: &user::UserCommands) -> bool {
    match command {
        user::UserCommands::Me { json }
        | user::UserCommands::Get { json, .. }
        | user::UserCommands::Update { json, .. }
        | user::UserCommands::List { json, .. }
        | user::UserCommands::Create { json, .. }
        | user::UserCommands::Memberships { json } => *json,
        user::UserCommands::Rm { .. } => false,
        user::UserCommands::Settings { command } => match command {
            user::UserSettingsCommands::Get { json }
            | user::UserSettingsCommands::Update { json, .. } => *json,
        },
    }
}

fn notification_outputs_json(command: &notification::NotificationCommands) -> bool {
    match command {
        notification::NotificationCommands::Send { json, .. }
        | notification::NotificationCommands::List { json, .. }
        | notification::NotificationCommands::Count { json, .. }
        | notification::NotificationCommands::CountsByGoal { json, .. }
        | notification::NotificationCommands::MarkRead { json, .. }
        | notification::NotificationCommands::MarkUnread { json, .. }
        | notification::NotificationCommands::MarkAllRead { json, .. } => *json,
        notification::NotificationCommands::Subscription { command } => match command {
            notification::SubscriptionCommands::List { json }
            | notification::SubscriptionCommands::Add { json, .. }
            | notification::SubscriptionCommands::Update { json, .. }
            | notification::SubscriptionCommands::EmailDestinations { json } => *json,
        },
    }
}

fn invitation_outputs_json(command: &invitation::InvitationCommands) -> bool {
    match command {
        invitation::InvitationCommands::Create { json, .. }
        | invitation::InvitationCommands::Resend { json, .. }
        | invitation::InvitationCommands::Accept { json, .. } => *json,
        invitation::InvitationCommands::Link { command } => match command {
            invitation::InviteLinkCommands::Create { json, .. } => *json,
            invitation::InviteLinkCommands::Deactivate { .. } => false,
        },
        invitation::InvitationCommands::Revoke { .. } => false,
    }
}

fn activity_outputs_json(command: &activity::ActivityCommands) -> bool {
    match command {
        activity::ActivityCommands::List { json, .. }
        | activity::ActivityCommands::Goal { json, .. }
        | activity::ActivityCommands::Summary { json, .. }
        | activity::ActivityCommands::GoalSummary { json, .. } => *json,
    }
}

fn streak_outputs_json(command: &streak::StreakCommands) -> bool {
    match command {
        streak::StreakCommands::Get { json, .. }
        | streak::StreakCommands::Freeze { json, .. }
        | streak::StreakCommands::Revive { json, .. }
        | streak::StreakCommands::Public { json, .. } => *json,
        streak::StreakCommands::Unfreeze { .. } => false,
        streak::StreakCommands::Share { command } => match command {
            streak::StreakShareCommands::Status { json, .. }
            | streak::StreakShareCommands::Create { json, .. } => *json,
            streak::StreakShareCommands::Revoke { .. } => false,
        },
    }
}

fn today_outputs_json(command: &today::TodayCommands) -> bool {
    match command {
        today::TodayCommands::List { json, .. }
        | today::TodayCommands::Add { json, .. }
        | today::TodayCommands::Done { json, .. }
        | today::TodayCommands::Reopen { json, .. }
        | today::TodayCommands::Status { json, .. } => *json,
    }
}

fn build_client() -> Result<ApiClient> {
    let creds = Credentials::load()?;
    let settings = Settings::load()?;
    match creds {
        Some(c) => {
            let org_id = settings.current_organization_id();
            let token = match org_id {
                Some(id) => c.token_for_org(id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "No API key stored for organization '{id}'. Run `addness login` to authenticate this org, or `addness configure` if you have a key for it."
                    )
                })?,
                None => c.any_token().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Not configured. Run: addness login (or 'addness configure' if you already have an API key)."
                    )
                })?,
            };
            Ok(ApiClient::new(token, c.api_url())?.with_org_id(org_id.map(|id| id.to_string())))
        }
        None => bail!(
            "Not configured. Run: addness login (or 'addness configure' if you already have an API key)."
        ),
    }
}

/// Build a client for org-level commands (org list, org current, etc.)
/// Falls back to any available token when the current org has no key stored.
fn build_client_for_org_commands() -> Result<ApiClient> {
    let creds = Credentials::load()?;
    let settings = Settings::load()?;
    match creds {
        Some(c) => {
            let org_id = settings.current_organization_id();
            let token = match org_id {
                Some(id) => c.token_for_org(id).or_else(|| c.any_token()),
                None => c.any_token(),
            }
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Not configured. Run: addness login (or 'addness configure' if you already have an API key)."
                )
            })?;
            Ok(ApiClient::new(token, c.api_url())?.with_org_id(org_id.map(|id| id.to_string())))
        }
        None => bail!(
            "Not configured. Run: addness login (or 'addness configure' if you already have an API key)."
        ),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let update_handle = cli
        .should_check_for_update()
        .then(|| tokio::spawn(update_check::check_for_update()));

    let result = match &cli.command {
        None => {
            let client = build_client()?;
            tui::run(client)
        }
        Some(Commands::Login {
            api_url,
            frontend_url,
        }) => login::handle_login(api_url, frontend_url.as_deref()).await,
        Some(Commands::Configure) => configure::handle_configure(),
        Some(Commands::Status { json }) => configure::handle_status(*json),
        Some(Commands::Logout) => configure::handle_logout(),
        Some(Commands::Org { command }) => {
            let client = build_client_for_org_commands()?;
            org::handle_org(command, &client).await
        }
        Some(Commands::Goal { command }) => {
            let client = build_client()?;
            goal::handle_goals(command, &client).await
        }
        Some(Commands::Comment { command }) => {
            let client = build_client()?;
            comment::handle_comments(command, &client).await
        }
        Some(Commands::Link { command }) => {
            let client = build_client()?;
            link::handle_link(command, &client).await
        }
        Some(Commands::Deliverable { command }) => {
            let client = build_client()?;
            deliverable::handle_deliverable(command, &client).await
        }
        Some(Commands::Assignment { command }) => {
            let client = build_client()?;
            assignment::handle_assignment(command, &client).await
        }
        Some(Commands::Kpi { command }) => {
            let client = build_client()?;
            kpi::handle_kpi(command, &client).await
        }
        Some(Commands::Member { command }) => {
            let client = build_client()?;
            member::handle_member(command, &client).await
        }
        Some(Commands::User { command }) => {
            let client = build_client()?;
            user::handle_user(command, &client).await
        }
        Some(Commands::Notification { command }) => {
            let client = build_client()?;
            notification::handle_notification(command, &client).await
        }
        Some(Commands::Invitation { command }) => {
            let client = build_client()?;
            invitation::handle_invitation(command, &client).await
        }
        Some(Commands::Activity { command }) => {
            let client = build_client()?;
            activity::handle_activity(command, &client).await
        }
        Some(Commands::Streak { command }) => {
            let client = build_client()?;
            streak::handle_streak(command, &client).await
        }
        Some(Commands::Today { command }) => {
            let client = build_client()?;
            today::handle_today(command.as_ref(), &client).await
        }
        Some(Commands::Summary { org, depth, json }) => {
            let client = build_client()?;
            summary::handle_summary(org.as_deref(), *depth, *json, &client).await
        }
        Some(Commands::DetectGoal { json }) => detect::handle_detect_goal(*json),
        Some(Commands::Update { check }) => update::handle_update(*check).await,
        Some(Commands::Skills) => skills::handle_skills(),
        Some(Commands::Completions { shell }) => {
            clap_complete::generate(
                *shell,
                &mut Cli::command(),
                "addness",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    };

    if let Some(handle) = update_handle {
        let _ = handle.await;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_check_runs_for_tui_and_human_commands() {
        assert!(Cli { command: None }.should_check_for_update());
        assert!(
            Cli {
                command: Some(Commands::Status { json: false })
            }
            .should_check_for_update()
        );
        assert!(
            Cli {
                command: Some(Commands::Today { command: None })
            }
            .should_check_for_update()
        );
    }

    #[test]
    fn update_check_skips_json_commands() {
        assert!(
            !Cli {
                command: Some(Commands::Status { json: true })
            }
            .should_check_for_update()
        );
        assert!(
            !Cli {
                command: Some(Commands::Goal {
                    command: goal::GoalCommands::List {
                        org: None,
                        depth: 3,
                        assigned_to: None,
                        status: None,
                        json: true,
                    }
                })
            }
            .should_check_for_update()
        );
    }

    #[test]
    fn update_check_skips_completions() {
        assert!(
            !Cli {
                command: Some(Commands::Completions {
                    shell: clap_complete::Shell::Bash,
                })
            }
            .should_check_for_update()
        );
    }
}
