use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::ApiClient;
use crate::cli::output::print_comments_table;

#[derive(Subcommand)]
pub enum CommentCommands {
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
    /// Update a comment's content
    Update {
        /// Comment ID
        id: String,
        /// New comment body
        #[arg(long)]
        body: Option<String>,
        /// New comment body from a file
        #[arg(long)]
        body_file: Option<String>,
        /// Mention member IDs (UUID), repeatable
        #[arg(long)]
        mention: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a comment
    Delete {
        /// Comment ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Mark a comment as resolved
    Resolve {
        /// Comment ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark a comment as unresolved
    Unresolve {
        /// Comment ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Add an emoji reaction to a comment
    React {
        /// Comment ID
        id: String,
        /// Emoji (e.g. 👍 or :+1:)
        #[arg(long)]
        emoji: String,
    },
    /// Manage comment attachments
    Attachment {
        #[command(subcommand)]
        command: AttachmentCommands,
    },
}

#[derive(Subcommand)]
pub enum AttachmentCommands {
    /// Remove an attachment from a comment
    Rm {
        /// Comment ID
        comment_id: String,
        /// Attachment ID
        attachment_id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

fn read_body(inline: Option<&String>, file: Option<&String>) -> Result<String> {
    match (inline, file) {
        (Some(s), None) => Ok(s.clone()),
        (None, Some(p)) => Ok(std::fs::read_to_string(p)?),
        (Some(_), Some(_)) => bail!("Specify only one of --body or --body-file"),
        (None, None) => bail!("Specify --body or --body-file"),
    }
}

pub async fn handle_comments(cmd: &CommentCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        CommentCommands::List { goal, json } => {
            let resp = client.list_comments(goal).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.comments)?);
            } else {
                print_comments_table(&resp.comments);
            }
            Ok(())
        }
        CommentCommands::Create { goal, body, json } => {
            let comment = client.create_comment(goal, body).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&comment)?);
            } else {
                println!("Comment created: {}", comment.id);
            }
            Ok(())
        }
        CommentCommands::Update {
            id,
            body,
            body_file,
            mention,
            json,
        } => {
            let content = read_body(body.as_ref(), body_file.as_ref())?;
            let comment = client.update_comment(id, &content, mention.clone()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&comment)?);
            } else {
                println!("Comment updated: {}", comment.id);
            }
            Ok(())
        }
        CommentCommands::Delete { id, force } => {
            if !*force && !crate::cli::commands::confirm(&format!("Delete comment {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_comment(id).await?;
            println!("Comment {id} deleted");
            Ok(())
        }
        CommentCommands::Resolve { id, json } => {
            let comment = client.resolve_comment(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&comment)?);
            } else {
                println!("Comment {id} resolved");
            }
            Ok(())
        }
        CommentCommands::Unresolve { id, json } => {
            let comment = client.unresolve_comment(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&comment)?);
            } else {
                println!("Comment {id} unresolved");
            }
            Ok(())
        }
        CommentCommands::React { id, emoji } => {
            client.add_reaction(id, emoji).await?;
            println!("Reacted {emoji} on comment {id}");
            Ok(())
        }
        CommentCommands::Attachment { command } => match command {
            AttachmentCommands::Rm {
                comment_id,
                attachment_id,
                force,
            } => {
                if !*force
                    && !crate::cli::commands::confirm(&format!(
                        "Delete attachment {attachment_id} from comment {comment_id}?"
                    ))?
                {
                    println!("Cancelled.");
                    return Ok(());
                }
                client
                    .delete_comment_attachment(comment_id, attachment_id)
                    .await?;
                println!("Attachment {attachment_id} deleted");
                Ok(())
            }
        },
    }
}
