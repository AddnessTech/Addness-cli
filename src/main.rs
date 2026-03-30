mod api;
mod cli;
mod config;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

use api::ApiClient;
use cli::commands::{auth, goals, org};
use config::{load_credentials, load_settings};

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
    /// Manage authentication
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
    let creds = load_credentials()?;
    let settings = load_settings()?;
    match creds {
        Some(c) => {
            Ok(ApiClient::new(&c.token, &c.api_url)?.with_org_id(settings.current_organization_id))
        }
        None => bail!("Not authenticated. Run: addness auth set-token <token>"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
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
