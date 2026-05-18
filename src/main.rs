mod api;
mod cli;
mod config;
mod update_check;

use anyhow::{Result, bail};
use clap::{CommandFactory, Parser, Subcommand};

use crate::config::{Credentials, DEFAULT_API_URL, Settings};
use api::ApiClient;
use cli::commands::{
    comment, configure, deliverable, detect, goal, link, login, org, skills, summary,
};

#[derive(Parser)]
#[command(
    name = "addness",
    about = "Addness CLI - Manage your goals from the terminal",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
    /// Output AI skills prompt for this CLI
    Skills,
    /// Generate shell completions
    Completions {
        /// Shell: bash, zsh, fish, powershell
        shell: clap_complete::Shell,
    },
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
                        "No API key stored for organization '{id}'. Run `addness login` to authenticate."
                    )
                })?,
                None => c.any_token().ok_or_else(|| {
                    anyhow::anyhow!("Not configured. Run: addness login")
                })?,
            };
            Ok(ApiClient::new(token, c.api_url())?.with_org_id(org_id.map(|id| id.to_string())))
        }
        None => bail!("Not configured. Run: addness login"),
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
            .ok_or_else(|| anyhow::anyhow!("Not configured. Run: addness login"))?;
            Ok(ApiClient::new(token, c.api_url())?.with_org_id(org_id.map(|id| id.to_string())))
        }
        None => bail!("Not configured. Run: addness login"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let update_handle = tokio::spawn(update_check::check_for_update());

    let result = match &cli.command {
        Commands::Login {
            api_url,
            frontend_url,
        } => login::handle_login(api_url, frontend_url.as_deref()).await,
        Commands::Configure => configure::handle_configure(),
        Commands::Status { json } => configure::handle_status(*json),
        Commands::Logout => configure::handle_logout(),
        Commands::Org { command } => {
            let client = build_client_for_org_commands()?;
            org::handle_org(command, &client).await
        }
        Commands::Goal { command } => {
            let client = build_client()?;
            goal::handle_goals(command, &client).await
        }
        Commands::Comment { command } => {
            let client = build_client()?;
            comment::handle_comments(command, &client).await
        }
        Commands::Link { command } => {
            let client = build_client()?;
            link::handle_link(command, &client).await
        }
        Commands::Deliverable { command } => {
            let client = build_client()?;
            deliverable::handle_deliverable(command, &client).await
        }
        Commands::Summary { org, depth, json } => {
            let client = build_client()?;
            summary::handle_summary(org.as_deref(), *depth, *json, &client).await
        }
        Commands::DetectGoal { json } => detect::handle_detect_goal(*json),
        Commands::Skills => skills::handle_skills(),
        Commands::Completions { shell } => {
            clap_complete::generate(
                *shell,
                &mut Cli::command(),
                "addness",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    };

    let _ = update_handle.await;

    result
}
