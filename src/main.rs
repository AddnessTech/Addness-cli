mod api;
mod cli;
mod config;
mod tui;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use crate::config::{Credentials, DEFAULT_API_URL, Settings};
use api::ApiClient;
use cli::commands::{comment, configure, goal, login, org};

#[derive(Parser)]
#[command(
    name = "addness",
    about = "Addness CLI - Manage your goals from the terminal"
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
    Status,
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
}

fn build_client() -> Result<ApiClient> {
    let creds = Credentials::load()?;
    let settings = Settings::load()?;
    match creds {
        Some(c) => Ok(ApiClient::new(c.token(), c.api_url())?
            .with_org_id(settings.current_organization_id().map(|id| id.to_string()))),
        None => bail!("Not configured. Run: addness login"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        None => tui::run(),
        Some(Commands::Login {
            api_url,
            frontend_url,
        }) => login::handle_login(api_url, frontend_url.as_deref()).await,
        Some(Commands::Configure) => configure::handle_configure(),
        Some(Commands::Status) => configure::handle_status(),
        Some(Commands::Logout) => configure::handle_logout(),
        Some(Commands::Org { command }) => {
            let client = build_client()?;
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
    }
}
