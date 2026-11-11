use anyhow::Result;
use clap::{Args, Subcommand};

use crate::api::ApiClient;

#[derive(Args)]
pub struct QuickMemoArgs {
    /// Organization for this invocation
    #[arg(long, global = true)]
    pub org: Option<String>,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    command: QuickMemoCommands,
}

#[derive(Subcommand)]
pub enum QuickMemoCommands {
    /// Read one page of your own saved quick memos
    List {
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, default_value = "30", value_parser = clap::value_parser!(u16).range(1..=50))]
        limit: u16,
    },
}

pub async fn handle_quick_memo(args: &QuickMemoArgs, client: &ApiClient) -> Result<()> {
    let result = match &args.command {
        QuickMemoCommands::List { cursor, limit } => {
            client.list_quick_memos(cursor.as_deref(), *limit).await?
        }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
