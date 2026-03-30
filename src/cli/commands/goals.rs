use anyhow::Result;
use clap::Subcommand;

use crate::api::{ApiClient, ApiResponse, TreeData};
use crate::cli::commands::org::resolve_org_id;
use crate::cli::output::print_goals_table;

#[derive(Subcommand)]
pub enum GoalsCommands {
    /// List goals in the organization tree
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Tree depth (default: 3)
        #[arg(long, default_value = "3")]
        depth: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_goals(cmd: &GoalsCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        GoalsCommands::List { org, depth, json } => {
            let org_id = resolve_org_id(org.as_deref())?;

            let resp: ApiResponse<TreeData> = client.get_goal_tree(&org_id, *depth).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.items)?);
            } else {
                print_goals_table(&resp.data.items);
            }
            Ok(())
        }
    }
}
