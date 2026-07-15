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
    activity, api_key, assignment, chat, codex_job, comment, configure, deliverable, detect,
    diagnosis, execution, goal, invitation, invoice, issue, kpi, link, login, media, meeting,
    member, notification, org, personal, referral, search, sharetree, skill, skills, streak,
    summary, today, tool, update, user,
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
    /// Manage goal issues (v2 chat threads on goals) and goal sections
    Issue {
        #[command(subcommand)]
        command: issue::IssueCommands,
    },
    /// Manage organization chat (DM/group rooms, messages, invitations)
    Chat {
        #[command(subcommand)]
        command: chat::ChatCommands,
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
    /// Unified search across objectives/comments/members (distinct from `goal search`)
    Search {
        /// Search query (supports `#goal:`/`#member:`/`#comment:` filters)
        query: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Max number of results to return
        #[arg(long)]
        limit: Option<u16>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u16>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage your diagnosis results (goal style, values, core values, master plan)
    Diagnosis {
        #[command(subcommand)]
        command: diagnosis::DiagnosisCommands,
    },
    /// Create referral links and view your referral performance
    Referral {
        #[command(subcommand)]
        command: referral::ReferralCommands,
    },
    /// View organization invoices
    Invoice {
        #[command(subcommand)]
        command: invoice::InvoiceCommands,
    },
    /// Manage portable, cloneable goal-tree exports (distinct from `goal share`)
    ShareTree {
        #[command(subcommand)]
        command: sharetree::ShareTreeCommands,
    },
    /// Manage inline media (editor paste/drop images and videos)
    Media {
        #[command(subcommand)]
        command: media::MediaCommands,
    },
    /// Manage your personal space (now/today docs, Markdown editing, agent sessions, projects)
    Personal {
        #[command(subcommand)]
        command: personal::PersonalCommands,
    },
    /// Execution-tab reporting: summaries, execution-record generation/history, goal-collapse
    /// preferences, active huddles, and the Codex-agent today's-goals view/apply
    Execution {
        #[command(subcommand)]
        command: execution::ExecutionCommands,
    },
    /// Manage meeting features: Huddle voice calls (status/recording/invites,
    /// excluding live participation), Meeting Bot (Recall.ai) jobs, meeting-note
    /// transcription/summary/goal workflow, and minutes CRUD
    Meeting {
        #[command(subcommand)]
        command: meeting::MeetingCommands,
    },
    /// Manage skills (reusable AI prompt templates): CRUD, search, performance,
    /// supplementary resources, and improvement-suggestion accept/reject.
    /// Distinct from `addness skills` (this CLI's own usage prompt).
    Skill {
        #[command(subcommand)]
        command: skill::SkillCommands,
    },
    /// Manage tools (executable actions an AI skill can invoke): CRUD, search, and execution
    Tool {
        #[command(subcommand)]
        command: tool::ToolCommands,
    },
    /// Manage cloud Codex jobs (agent sessions): create, list, follow-up input,
    /// resume, cancel/close, delete, and the live event stream
    CodexJob {
        #[command(subcommand)]
        command: codex_job::CodexJobCommands,
    },
    /// Manage personal API keys (list, create, revoke). The plaintext key is
    /// only ever returned by `create` — it cannot be retrieved again
    ApiKey {
        #[command(subcommand)]
        command: api_key::ApiKeyCommands,
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
        | Commands::Search { json, .. }
        | Commands::DetectGoal { json } => *json,
        Commands::Diagnosis { command } => diagnosis_outputs_json(command),
        Commands::Referral { command } => referral_outputs_json(command),
        Commands::Invoice { command } => invoice_outputs_json(command),
        Commands::ShareTree { command } => sharetree_outputs_json(command),
        Commands::Media { command } => media_outputs_json(command),
        Commands::Personal { command } => personal_outputs_json(command),
        Commands::Execution { command } => execution_outputs_json(command),
        Commands::Meeting { command } => meeting_outputs_json(command),
        Commands::Skill { command } => skill_outputs_json(command),
        Commands::Tool { command } => tool_outputs_json(command),
        Commands::CodexJob { command } => codex_job_outputs_json(command),
        Commands::ApiKey { command } => api_key_outputs_json(command),
        Commands::Org { command } => org_outputs_json(command),
        Commands::Goal { command } => goal_outputs_json(command),
        Commands::Comment { command } => comment_outputs_json(command),
        Commands::Issue { command } => issue_outputs_json(command),
        Commands::Chat { command } => chat_outputs_json(command),
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
            goal::ShareCommands::Create { json, .. }
            | goal::ShareCommands::GetPublic { json, .. } => *json,
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
        goal::GoalCommands::ReportSchedule { command } => match command {
            goal::ReportScheduleCommands::Get { json, .. }
            | goal::ReportScheduleCommands::Set { json, .. } => *json,
            goal::ReportScheduleCommands::Rm { .. } => false,
        },
    }
}

fn diagnosis_outputs_json(command: &diagnosis::DiagnosisCommands) -> bool {
    match command {
        diagnosis::DiagnosisCommands::Save { json, .. }
        | diagnosis::DiagnosisCommands::List { json }
        | diagnosis::DiagnosisCommands::Get { json, .. }
        | diagnosis::DiagnosisCommands::Stats { json, .. }
        | diagnosis::DiagnosisCommands::Profiles { json, .. }
        | diagnosis::DiagnosisCommands::Profile { json, .. } => *json,
        diagnosis::DiagnosisCommands::Visibility { command } => match command {
            diagnosis::VisibilityCommands::Get { json, .. }
            | diagnosis::VisibilityCommands::Set { json, .. } => *json,
        },
    }
}

fn referral_outputs_json(command: &referral::ReferralCommands) -> bool {
    match command {
        referral::ReferralCommands::LinkCreate { json, .. }
        | referral::ReferralCommands::List { json, .. }
        | referral::ReferralCommands::Convert { json, .. } => *json,
    }
}

fn invoice_outputs_json(command: &invoice::InvoiceCommands) -> bool {
    match command {
        invoice::InvoiceCommands::List { json, .. } => *json,
    }
}

fn sharetree_outputs_json(command: &sharetree::ShareTreeCommands) -> bool {
    match command {
        sharetree::ShareTreeCommands::Create { json, .. }
        | sharetree::ShareTreeCommands::List { json, .. }
        | sharetree::ShareTreeCommands::Clone { json, .. }
        | sharetree::ShareTreeCommands::GetPublic { json, .. } => *json,
        sharetree::ShareTreeCommands::Revoke { .. } => false,
    }
}

fn media_outputs_json(command: &media::MediaCommands) -> bool {
    match command {
        media::MediaCommands::View { json, .. } | media::MediaCommands::Upload { json, .. } => {
            *json
        }
    }
}

fn personal_outputs_json(command: &personal::PersonalCommands) -> bool {
    match command {
        personal::PersonalCommands::Now { json }
        | personal::PersonalCommands::Today { json, .. }
        | personal::PersonalCommands::TodayAppend { json, .. }
        | personal::PersonalCommands::Day { json, .. }
        | personal::PersonalCommands::TextPatch { json, .. }
        | personal::PersonalCommands::EnsureOrganization { json }
        | personal::PersonalCommands::TodayList { json, .. }
        | personal::PersonalCommands::DailyActivity { json, .. } => *json,
        personal::PersonalCommands::Reset { json, force } => *json && *force,
        personal::PersonalCommands::Markdown { command } => match command {
            personal::MarkdownCommands::Analyze { json, .. }
            | personal::MarkdownCommands::ReplaceSection { json, .. }
            | personal::MarkdownCommands::UpsertSection { json, .. }
            | personal::MarkdownCommands::UpsertListItem { json, .. }
            | personal::MarkdownCommands::ReplaceDocument { json, .. }
            | personal::MarkdownCommands::AppendLogEntry { json, .. } => *json,
        },
        personal::PersonalCommands::AgentSession { command } => match command {
            personal::AgentSessionCommands::List { json, .. }
            | personal::AgentSessionCommands::Create { json, .. }
            | personal::AgentSessionCommands::Get { json, .. }
            | personal::AgentSessionCommands::Update { json, .. } => *json,
        },
        personal::PersonalCommands::Project { command } => match command {
            personal::ProjectCommands::List { json, .. }
            | personal::ProjectCommands::Create { json, .. }
            | personal::ProjectCommands::Get { json, .. }
            | personal::ProjectCommands::Update { json, .. } => *json,
        },
    }
}

fn execution_outputs_json(command: &execution::ExecutionCommands) -> bool {
    match command {
        execution::ExecutionCommands::Summary { json, .. }
        | execution::ExecutionCommands::MemberSummary { json, .. }
        | execution::ExecutionCommands::Generate { json, .. }
        | execution::ExecutionCommands::Update { json, .. }
        | execution::ExecutionCommands::History { json, .. }
        | execution::ExecutionCommands::ActiveHuddles { json, .. } => *json,
        execution::ExecutionCommands::Preference { command } => match command {
            execution::PreferenceCommands::Get { json, .. }
            | execution::PreferenceCommands::Set { json, .. } => *json,
        },
        execution::ExecutionCommands::Codex { command } => match command {
            execution::CodexCommands::View { json, .. }
            | execution::CodexCommands::Apply { json, .. } => *json,
        },
    }
}

fn meeting_outputs_json(command: &meeting::MeetingCommands) -> bool {
    match command {
        meeting::MeetingCommands::Huddle { command } => match command {
            meeting::HuddleCommands::Status { json, .. }
            | meeting::HuddleCommands::ActiveSubtree { json, .. }
            | meeting::HuddleCommands::SessionStatus { json, .. }
            | meeting::HuddleCommands::RecordingStart { json, .. }
            | meeting::HuddleCommands::RecordingStop { json, .. }
            | meeting::HuddleCommands::InviteableMembers { json, .. }
            | meeting::HuddleCommands::Active { json, .. }
            | meeting::HuddleCommands::TranscriptionProgress { json, .. }
            | meeting::HuddleCommands::Invite { json, .. } => *json,
        },
        meeting::MeetingCommands::Bot { command } => match command {
            meeting::BotCommands::List { json, .. }
            | meeting::BotCommands::Get { json, .. }
            | meeting::BotCommands::Create { json, .. }
            | meeting::BotCommands::Delete { json, .. } => *json,
        },
        meeting::MeetingCommands::Notes { command } => match command {
            meeting::NotesCommands::Transcribe { json, .. }
            | meeting::NotesCommands::Summarize { json, .. }
            | meeting::NotesCommands::PostMinutes { json, .. }
            | meeting::NotesCommands::SuggestGoals { json, .. }
            | meeting::NotesCommands::CreateGoals { json, .. } => *json,
        },
        meeting::MeetingCommands::Minutes { command } => match command {
            meeting::MinutesCommands::Create { json, .. }
            | meeting::MinutesCommands::List { json, .. }
            | meeting::MinutesCommands::Get { json, .. }
            | meeting::MinutesCommands::Update { json, .. }
            | meeting::MinutesCommands::Delete { json, .. } => *json,
        },
    }
}

fn skill_outputs_json(command: &skill::SkillCommands) -> bool {
    match command {
        skill::SkillCommands::Create { json, .. }
        | skill::SkillCommands::List { json, .. }
        | skill::SkillCommands::General { json, .. }
        | skill::SkillCommands::Search { json, .. }
        | skill::SkillCommands::Get { json, .. }
        | skill::SkillCommands::Update { json, .. }
        | skill::SkillCommands::Delete { json, .. }
        | skill::SkillCommands::Performance { json, .. } => *json,
        skill::SkillCommands::Resource { command } => match command {
            skill::SkillResourceCommands::Create { json, .. }
            | skill::SkillResourceCommands::List { json, .. }
            | skill::SkillResourceCommands::Get { json, .. }
            | skill::SkillResourceCommands::Update { json, .. }
            | skill::SkillResourceCommands::Delete { json, .. } => *json,
        },
        skill::SkillCommands::Refinement { command } => match command {
            skill::SkillRefinementCommands::Accept { json, .. }
            | skill::SkillRefinementCommands::Reject { json, .. } => *json,
        },
    }
}

fn codex_job_outputs_json(command: &codex_job::CodexJobCommands) -> bool {
    match command {
        codex_job::CodexJobCommands::List { json, .. }
        | codex_job::CodexJobCommands::Get { json, .. }
        | codex_job::CodexJobCommands::Create { json, .. }
        | codex_job::CodexJobCommands::Input { json, .. }
        | codex_job::CodexJobCommands::Resume { json, .. }
        | codex_job::CodexJobCommands::Cancel { json, .. }
        | codex_job::CodexJobCommands::Events { json, .. } => *json,
        codex_job::CodexJobCommands::Close { json, force, .. }
        | codex_job::CodexJobCommands::Delete { json, force, .. } => *json && *force,
    }
}

fn api_key_outputs_json(command: &api_key::ApiKeyCommands) -> bool {
    match command {
        api_key::ApiKeyCommands::List { json, .. }
        | api_key::ApiKeyCommands::Create { json, .. } => *json,
        api_key::ApiKeyCommands::Rm { json, force, .. } => *json && *force,
    }
}

fn tool_outputs_json(command: &tool::ToolCommands) -> bool {
    match command {
        tool::ToolCommands::Create { json, .. }
        | tool::ToolCommands::List { json, .. }
        | tool::ToolCommands::Search { json, .. }
        | tool::ToolCommands::Get { json, .. }
        | tool::ToolCommands::Update { json, .. }
        | tool::ToolCommands::Delete { json, .. }
        | tool::ToolCommands::Execute { json, .. } => *json,
    }
}

fn comment_outputs_json(command: &comment::CommentCommands) -> bool {
    match command {
        comment::CommentCommands::List { json, .. }
        | comment::CommentCommands::ListAll { json, .. }
        | comment::CommentCommands::Get { json, .. }
        | comment::CommentCommands::Context { json, .. }
        | comment::CommentCommands::Reactions { json, .. }
        | comment::CommentCommands::Create { json, .. }
        | comment::CommentCommands::Update { json, .. }
        | comment::CommentCommands::Resolve { json, .. }
        | comment::CommentCommands::Unresolve { json, .. } => *json,
        comment::CommentCommands::Delete { .. }
        | comment::CommentCommands::React { .. }
        | comment::CommentCommands::Attachment { .. } => false,
    }
}

fn issue_outputs_json(command: &issue::IssueCommands) -> bool {
    match command {
        issue::IssueCommands::List { json, .. }
        | issue::IssueCommands::ListAll { json, .. }
        | issue::IssueCommands::Create { json, .. }
        | issue::IssueCommands::Update { json, .. }
        | issue::IssueCommands::Messages { json, .. }
        | issue::IssueCommands::Reply { json, .. }
        | issue::IssueCommands::EditMessage { json, .. }
        | issue::IssueCommands::React { json, .. }
        | issue::IssueCommands::Reactions { json, .. }
        | issue::IssueCommands::Search { json, .. }
        | issue::IssueCommands::Preview { json, .. }
        | issue::IssueCommands::Resolve { json, .. }
        | issue::IssueCommands::Unresolve { json, .. } => *json,
        issue::IssueCommands::Read { .. } | issue::IssueCommands::Unreact { .. } => false,
        issue::IssueCommands::Sections { command } => match command {
            issue::SectionCommands::List { json, .. }
            | issue::SectionCommands::Pinned { json, .. }
            | issue::SectionCommands::UnreadCount { json, .. }
            | issue::SectionCommands::UnreadMentions { json, .. } => *json,
            issue::SectionCommands::Pin { .. } | issue::SectionCommands::Unpin { .. } => false,
        },
    }
}

fn chat_outputs_json(command: &chat::ChatCommands) -> bool {
    match command {
        chat::ChatCommands::Search { json, .. } => *json,
        chat::ChatCommands::Room { command } => match command {
            chat::RoomCommands::List { json, .. }
            | chat::RoomCommands::ListPublic { json, .. }
            | chat::RoomCommands::UnreadCount { json, .. }
            | chat::RoomCommands::Get { json, .. }
            | chat::RoomCommands::CreateDm { json, .. }
            | chat::RoomCommands::CreateGroup { json, .. }
            | chat::RoomCommands::Rename { json, .. }
            | chat::RoomCommands::Members { json, .. }
            | chat::RoomCommands::Join { json, .. }
            | chat::RoomCommands::Invite { json, .. }
            | chat::RoomCommands::SetIcon { json, .. } => *json,
            chat::RoomCommands::Rm { .. }
            | chat::RoomCommands::Leave { .. }
            | chat::RoomCommands::RemoveMember { .. }
            | chat::RoomCommands::RmIcon { .. }
            | chat::RoomCommands::Read { .. }
            | chat::RoomCommands::Hide { .. } => false,
        },
        chat::ChatCommands::Message { command } => match command {
            chat::MessageCommands::List { json, .. }
            | chat::MessageCommands::Post { json, .. }
            | chat::MessageCommands::Update { json, .. }
            | chat::MessageCommands::React { json, .. }
            | chat::MessageCommands::Reactions { json, .. } => *json,
            chat::MessageCommands::Rm { .. } | chat::MessageCommands::Unreact { .. } => false,
        },
        chat::ChatCommands::Invitation { command } => match command {
            chat::ChatInvitationCommands::ListPending { json }
            | chat::ChatInvitationCommands::Accept { json, .. } => *json,
            chat::ChatInvitationCommands::Decline { .. } => false,
        },
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
    match command {
        member::MemberCommands::List { json, .. }
        | member::MemberCommands::Search { json, .. }
        | member::MemberCommands::Children { json, .. }
        | member::MemberCommands::Admins { json, .. }
        | member::MemberCommands::DeletePreview { json, .. }
        | member::MemberCommands::Browse { json, .. }
        | member::MemberCommands::Objectives { json, .. }
        | member::MemberCommands::SetAvatar { json, .. }
        | member::MemberCommands::Get { json, .. } => *json,
        member::MemberCommands::Update { .. }
        | member::MemberCommands::Pin { .. }
        | member::MemberCommands::Unpin { .. }
        | member::MemberCommands::Rm { .. }
        | member::MemberCommands::SetSourceOrg { .. } => false,
        member::MemberCommands::Admin { command } => match command {
            member::AdminCommands::Grant { .. } | member::AdminCommands::Revoke { .. } => false,
        },
        member::MemberCommands::Tag { command } => match command {
            member::MemberTagCommands::List { json, .. }
            | member::MemberTagCommands::Create { json, .. }
            | member::MemberTagCommands::ListFor { json, .. } => *json,
            member::MemberTagCommands::Rm { .. }
            | member::MemberTagCommands::Assign { .. }
            | member::MemberTagCommands::Unassign { .. } => false,
        },
    }
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
        | invitation::InvitationCommands::Accept { json, .. }
        | invitation::InvitationCommands::LegacyAccept { json, .. }
        | invitation::InvitationCommands::CheckPlanUpgrade { json, .. }
        | invitation::InvitationCommands::Preview { json, .. }
        | invitation::InvitationCommands::AcceptToken { json, .. }
        | invitation::InvitationCommands::InvitedMembers { json, .. }
        | invitation::InvitationCommands::Overview { json, .. } => *json,
        invitation::InvitationCommands::Link { command } => match command {
            invitation::InviteLinkCommands::Create { json, .. }
            | invitation::InviteLinkCommands::List { json, .. }
            | invitation::InviteLinkCommands::Join { json, .. } => *json,
            invitation::InviteLinkCommands::Deactivate { .. } => false,
        },
        invitation::InvitationCommands::Pending { command } => match command {
            invitation::PendingCommands::List { json }
            | invitation::PendingCommands::Access { json, .. } => *json,
        },
        invitation::InvitationCommands::Revoke { .. }
        | invitation::InvitationCommands::Decline { .. } => false,
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
        today::TodayCommands::Todo { command } => today_todo_outputs_json(command),
        today::TodayCommands::Planned { command } => today_planned_outputs_json(command),
        today::TodayCommands::Calendar { command } => today_calendar_outputs_json(command),
    }
}

fn today_todo_outputs_json(command: &today::TodoCommands) -> bool {
    match command {
        today::TodoCommands::List { json, .. }
        | today::TodoCommands::Add { json, .. }
        | today::TodoCommands::Update { json, .. }
        | today::TodoCommands::Rm { json, .. }
        | today::TodoCommands::Activity { json, .. } => *json,
    }
}

fn today_planned_outputs_json(command: &today::PlannedCommands) -> bool {
    match command {
        today::PlannedCommands::List { json, .. }
        | today::PlannedCommands::Material { json, .. }
        | today::PlannedCommands::Add { json, .. }
        | today::PlannedCommands::Update { json, .. }
        | today::PlannedCommands::Rm { json, .. }
        | today::PlannedCommands::Adopt { json, .. } => *json,
    }
}

fn today_calendar_outputs_json(command: &today::CalendarCommands) -> bool {
    match command {
        today::CalendarCommands::Events { json, .. }
        | today::CalendarCommands::Complete { json, .. }
        | today::CalendarCommands::GoalCalendar { json, .. }
        | today::CalendarCommands::GoalHistory { json, .. } => *json,
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
        Some(Commands::Issue { command }) => {
            let client = build_client()?;
            issue::handle_issue(command, &client).await
        }
        Some(Commands::Chat { command }) => {
            let client = build_client()?;
            chat::handle_chat(command, &client).await
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
        Some(Commands::Search {
            query,
            org,
            limit,
            offset,
            json,
        }) => {
            let client = build_client()?;
            search::handle_search(query, org.as_deref(), *limit, *offset, *json, &client).await
        }
        Some(Commands::Diagnosis { command }) => {
            let client = build_client()?;
            diagnosis::handle_diagnosis(command, &client).await
        }
        Some(Commands::Referral { command }) => {
            let client = build_client()?;
            referral::handle_referral(command, &client).await
        }
        Some(Commands::Invoice { command }) => {
            let client = build_client()?;
            invoice::handle_invoice(command, &client).await
        }
        Some(Commands::ShareTree { command }) => {
            let client = build_client()?;
            sharetree::handle_sharetree(command, &client).await
        }
        Some(Commands::Media { command }) => {
            let client = build_client()?;
            media::handle_media(command, &client).await
        }
        Some(Commands::Personal { command }) => {
            let client = build_client()?;
            personal::handle_personal(command, &client).await
        }
        Some(Commands::Execution { command }) => {
            let client = build_client()?;
            execution::handle_execution(command, &client).await
        }
        Some(Commands::Meeting { command }) => {
            let client = build_client()?;
            meeting::handle_meeting(command, &client).await
        }
        Some(Commands::Skill { command }) => {
            let client = build_client()?;
            skill::handle_skill(command, &client).await
        }
        Some(Commands::Tool { command }) => {
            let client = build_client()?;
            tool::handle_tool(command, &client).await
        }
        Some(Commands::CodexJob { command }) => {
            let client = build_client()?;
            codex_job::handle_codex_job(command, &client).await
        }
        Some(Commands::ApiKey { command }) => {
            let client = build_client()?;
            api_key::handle_api_key(command, &client).await
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
