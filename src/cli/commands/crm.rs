use anyhow::Result;
use clap::{Args, Subcommand};

use crate::api::{ApiClient, CrmListParams, CrmRowsParams};

#[derive(Args)]
pub struct CrmArgs {
    /// Organization for this invocation
    #[arg(long, global = true)]
    pub org: Option<String>,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    command: CrmCommands,
}

#[derive(Subcommand)]
pub enum CrmCommands {
    /// List accessible CRM lists, one page per request
    List {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        owner: Option<String>,
        /// Empty string selects lists outside folders
        #[arg(long)]
        folder_path: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, default_value = "50", value_parser = clap::value_parser!(u16).range(1..=100))]
        limit: u16,
    },
    /// Read list settings with one request; use `rows` for customer rows
    Get { id: uuid::Uuid },
    /// Read one page of customer rows without fetching list settings again
    Rows {
        id: uuid::Uuid,
        #[arg(long, default_value = "all", value_parser = ["all", "today", "stale", "overdue", "duplicate_candidate", "unreachable", "no_goal"])]
        filter: String,
        #[arg(long)]
        stage_key: Option<String>,
        #[arg(long)]
        assignee_member_id: Option<uuid::Uuid>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, default_value = "100", value_parser = clap::value_parser!(u16).range(1..=200))]
        limit: u16,
    },
}

pub async fn handle_crm(args: &CrmArgs, client: &ApiClient) -> Result<()> {
    let result = match &args.command {
        CrmCommands::List {
            query,
            owner,
            folder_path,
            tag,
            cursor,
            limit,
        } => {
            client
                .list_crm_lists(&CrmListParams {
                    query: query.as_deref(),
                    owner: owner.as_deref(),
                    folder_path: folder_path.as_deref(),
                    tag_names: tag,
                    cursor: cursor.as_deref(),
                    limit: *limit,
                })
                .await?
        }
        CrmCommands::Get { id } => client.get_crm_list(&id.to_string()).await?,
        CrmCommands::Rows {
            id,
            filter,
            stage_key,
            assignee_member_id,
            cursor,
            limit,
        } => {
            let assignee = assignee_member_id.map(|id| id.to_string());
            client
                .get_crm_rows(
                    &id.to_string(),
                    &CrmRowsParams {
                        filter,
                        stage_key: stage_key.as_deref(),
                        assignee_member_id: assignee.as_deref(),
                        cursor: cursor.as_deref(),
                        limit: *limit,
                    },
                )
                .await?
        }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
