use anyhow::{Result, bail};
use clap::Subcommand;
use std::io::{self, Read};

use crate::api::{ApiClient, CommentDetail, ListAllCommentsParams, ListCommentsParams};
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
    /// List comments across goals (global feed with filters)
    ListAll {
        /// Filter by goal ID
        #[arg(long)]
        goal: Option<String>,
        /// Filter by author (organization member ID)
        #[arg(long)]
        author: Option<String>,
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
    /// Get a comment with its surrounding comments (notification highlight)
    Context {
        /// Comment ID
        id: String,
        /// Number of comments before/after the target (1-50, default 5)
        #[arg(long)]
        radius: Option<u8>,
        /// Filter surrounding comments by resolved status
        #[arg(long)]
        resolved: Option<bool>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List members who reacted to a comment with a given emoji
    Reactions {
        /// Comment ID
        id: String,
        /// Emoji to look up
        #[arg(long)]
        emoji: String,
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

/// Read a message body from an inline flag, a file, or stdin (`-`).
/// Shared with the goal-issue commands (`issue.rs`).
pub(super) fn read_body(inline: Option<&String>, file: Option<&String>) -> Result<String> {
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

/// Radius accepted by GET /comments/:id/context (backend rejects outside 1-50).
fn validate_radius(radius: Option<u8>) -> Result<Option<u8>> {
    if let Some(value) = radius
        && !(1..=50).contains(&value)
    {
        bail!("--radius must be between 1 and 50.");
    }
    Ok(radius)
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
        CommentCommands::ListAll {
            goal,
            author,
            parent,
            resolved,
            limit,
            offset,
            sort,
            include_replies,
            json,
        } => {
            let resp = client
                .list_all_comments(ListAllCommentsParams {
                    goal_id: goal.as_deref(),
                    author_id: author.as_deref(),
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
                let shown = resp.comments.len() as i64;
                if shown < resp.total_count {
                    println!(
                        "Showing {shown} of {} comments (use --offset or --limit).",
                        resp.total_count
                    );
                }
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
        CommentCommands::Context {
            id,
            radius,
            resolved,
            json,
        } => {
            let context = client
                .get_comment_context(id, validate_radius(*radius)?, *resolved)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&context)?);
            } else {
                if let Some(parent) = &context.parent_comment {
                    println!("Parent comment:");
                    print_comments_table(std::slice::from_ref(parent));
                    println!();
                }
                print_comments_table(&context.comments);
                let target = context
                    .comments
                    .get(usize::try_from(context.target_index).unwrap_or(usize::MAX))
                    .map(|c| c.id.as_str())
                    .unwrap_or("-");
                println!();
                println!(
                    "Target: {target} (more above: {}, more below: {}, total: {})",
                    context.has_above, context.has_below, context.total_count
                );
            }
            Ok(())
        }
        CommentCommands::Reactions { id, emoji, json } => {
            let users = client.get_comment_reaction_users(id, emoji).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&users)?);
            } else {
                match users.as_array() {
                    Some(members) if !members.is_empty() => {
                        for member in members {
                            let member_id =
                                member.get("id").and_then(|v| v.as_str()).unwrap_or("-");
                            let name = member.get("name").and_then(|v| v.as_str()).unwrap_or("-");
                            println!("{member_id} — {name}");
                        }
                    }
                    _ => println!("No reactions with {emoji} on comment {id}."),
                }
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

#[cfg(test)]
mod tests {
    use super::{ensure_body, validate_limit, validate_radius, validate_sort};

    #[test]
    fn validate_sort_accepts_asc_and_desc() {
        assert_eq!(
            validate_sort(Some(&"asc".to_string())).unwrap(),
            Some("asc")
        );
        assert_eq!(
            validate_sort(Some(&"desc".to_string())).unwrap(),
            Some("desc")
        );
        assert_eq!(validate_sort(None).unwrap(), None);
    }

    #[test]
    fn validate_sort_rejects_other_values() {
        assert!(validate_sort(Some(&"newest".to_string())).is_err());
    }

    #[test]
    fn validate_limit_accepts_range_bounds() {
        assert_eq!(validate_limit(Some(1)).unwrap(), Some(1));
        assert_eq!(validate_limit(Some(1000)).unwrap(), Some(1000));
        assert_eq!(validate_limit(None).unwrap(), None);
    }

    #[test]
    fn validate_limit_rejects_out_of_range() {
        assert!(validate_limit(Some(0)).is_err());
        assert!(validate_limit(Some(1001)).is_err());
    }

    #[test]
    fn validate_radius_accepts_range_bounds() {
        assert_eq!(validate_radius(Some(1)).unwrap(), Some(1));
        assert_eq!(validate_radius(Some(50)).unwrap(), Some(50));
        assert_eq!(validate_radius(None).unwrap(), None);
    }

    #[test]
    fn validate_radius_rejects_out_of_range() {
        assert!(validate_radius(Some(0)).is_err());
        assert!(validate_radius(Some(51)).is_err());
    }

    #[test]
    fn ensure_body_rejects_empty_and_oversized_content() {
        assert!(ensure_body("  \n".to_string()).is_err());
        assert!(ensure_body("a".repeat(10_001)).is_err());
        assert_eq!(ensure_body("hello".to_string()).unwrap(), "hello");
    }
}
