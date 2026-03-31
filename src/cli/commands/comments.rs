use anyhow::Result;
use clap::Subcommand;

use crate::api::ApiClient;
use crate::cli::output::print_comments_table;

#[derive(Subcommand)]
pub enum CommentsCommands {
    /// List comments on a goal
    List {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a comment on a goal
    Create {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Comment body
        #[arg(long)]
        body: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_comments(cmd: &CommentsCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        CommentsCommands::List { goal, json } => {
            let resp = client.list_comments(goal).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.comments)?);
            } else {
                print_comments_table(&resp.comments);
            }
            Ok(())
        }
        CommentsCommands::Create { goal, body, json } => {
            let comment = client.create_comment(goal, body).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&comment)?);
            } else {
                println!("Comment created: {}", comment.id);
            }
            Ok(())
        }
    }
}
