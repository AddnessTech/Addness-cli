mod api;
mod cli;
mod config;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

use crate::config::{Credentials, Settings};
use api::ApiClient;
use cli::commands::{auth, configure, goals, login, org};

#[derive(Parser)]
#[command(
    name = "addness",
    about = "Addness CLI - Manage your goals from the terminal"
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
        #[arg(long, default_value = "https://api.addness.app")]
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
    /// Manage authentication (legacy)
    Auth {
        #[command(subcommand)]
        command: auth::AuthCommands,
    },
    /// Manage organizations
    Org {
        #[command(subcommand)]
        command: org::OrgCommands,
    },
    /// Manage goals
    Goals {
        #[command(subcommand)]
        command: goals::GoalsCommands,
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
        Commands::Login { api_url, frontend_url } => {
            login::handle_login(api_url, frontend_url.as_deref()).await
        }
        Commands::Configure => configure::handle_configure(),
        Commands::Status => configure::handle_status(),
        Commands::Logout => configure::handle_logout(),
        Commands::Auth { command } => auth::handle_auth(command),
        Commands::Org { command } => {
            let client = build_client()?;
            org::handle_org(command, &client).await
        }
        Commands::Goals { command } => {
            let client = build_client()?;
            goals::handle_goals(command, &client).await
        }
    }
}
