use anyhow::{Result, bail};
use clap::Subcommand;
use std::io::{self, Read};

use crate::api::{ApiClient, CommentDetail, ListCommentsParams};
use crate::cli::output::print_comments_table;

#[derive(Subcommand)]
pub enum CommentCommands {
    /// List comments on a goal
    List {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Parent comment ID (list replies to this comment)
        #[arg(long)]
        parent: Option<String>,
        /// Filter by resolved status
        #[arg(long)]
        resolved: Option<bool>,
        /// Max number of comments to return (1-1000)
        #[arg(long)]
        limit: Option<u16>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u64>,
        /// Sort order: asc or desc
        #[arg(long)]
        sort: Option<String>,
        /// Include thread replies when supported by the API
        #[arg(long)]
        include_replies: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a comment and its replies
    Get {
        /// Comment ID
        id: String,
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
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read comment body from a file. Use '-' to read stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Parent comment ID for a thread reply
        #[arg(long)]
        parent: Option<String>,
        /// Mention member IDs (UUID), repeatable
        #[arg(long)]
        mention: Vec<String>,
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
        (Some(s), None) if s == "-" => read_stdin(),
        (Some(s), None) => Ok(s.clone()),
        (None, Some(p)) if p == "-" => read_stdin(),
        (None, Some(p)) => Ok(std::fs::read_to_string(p)?),
        (Some(_), Some(_)) => bail!("Specify only one of --body or --body-file"),
        (None, None) => bail!("Specify --body or --body-file"),
    }
}

fn read_stdin() -> Result<String> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

fn ensure_body(content: String) -> Result<String> {
    if content.trim().is_empty() {
        bail!("Comment body is empty. Specify --body, --body-file, or pipe content with --body -.");
    }
    if content.chars().count() > 10_000 {
        bail!("Comment body must be 10000 characters or less.");
    }
    Ok(content)
}

fn validate_sort(sort: Option<&String>) -> Result<Option<&str>> {
    match sort.map(|s| s.as_str()) {
        Some("asc" | "desc") => Ok(sort.map(|s| s.as_str())),
        Some(value) => bail!("Invalid --sort '{value}'. Use asc or desc."),
        None => Ok(None),
    }
}

fn validate_limit(limit: Option<u16>) -> Result<Option<u16>> {
    if let Some(value) = limit
        && !(1..=1000).contains(&value)
    {
        bail!("--limit must be between 1 and 1000.");
    }
    Ok(limit)
}

fn print_comment_detail(detail: &CommentDetail) {
    print_comments_table(std::slice::from_ref(&detail.comment));
    if !detail.replies.is_empty() {
        println!();
        println!("Replies:");
        print_comments_table(&detail.replies);
    }
}

pub async fn handle_comments(cmd: &CommentCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        CommentCommands::List {
            goal,
            parent,
            resolved,
            limit,
            offset,
            sort,
            include_replies,
            json,
        } => {
            let resp = client
                .list_comments_with_params(ListCommentsParams {
                    goal_id: goal,
                    parent_id: parent.as_deref(),
                    resolved: *resolved,
                    limit: validate_limit(*limit)?,
                    offset: *offset,
                    sort: validate_sort(sort.as_ref())?,
                    include_replies: *include_replies,
                })
                .await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.comments)?);
            } else {
                print_comments_table(&resp.comments);
            }
            Ok(())
        }
        CommentCommands::Get { id, json } => {
            let comment = client.get_comment(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&comment)?);
            } else {
                print_comment_detail(&comment);
            }
            Ok(())
        }
        CommentCommands::Create {
            goal,
            body,
            body_file,
            parent,
            mention,
            json,
        } => {
            let content = ensure_body(read_body(body.as_ref(), body_file.as_ref())?)?;
            let comment = client
                .create_comment_with_options(goal, &content, parent.clone(), mention.clone())
                .await?;

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
            let content = ensure_body(read_body(body.as_ref(), body_file.as_ref())?)?;
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
