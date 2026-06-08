use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Subcommand;

use crate::api::{ApiClient, DeliverableType};

#[derive(Subcommand)]
pub enum DeliverableCommands {
    /// Add a deliverable (text/markdown content or a file like an image) to a goal
    Add {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Inline text/markdown content (mutually exclusive with --file and --content-file)
        #[arg(long, conflicts_with_all = ["file", "content_file"])]
        content: Option<String>,
        /// Read text/markdown content from a local file (mutually exclusive with --content and --file)
        #[arg(long, conflicts_with_all = ["content", "file"])]
        content_file: Option<PathBuf>,
        /// Upload a local file (image, pdf, etc.) as a file deliverable
        #[arg(long, conflicts_with_all = ["content", "content_file"])]
        file: Option<PathBuf>,
        /// Display name (required for inline --content; auto-derived from filename otherwise)
        #[arg(long)]
        name: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List deliverables on a goal
    List {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a deliverable's content (document type)
    Update {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Deliverable ID
        id: String,
        /// New content (markdown)
        #[arg(long)]
        content: Option<String>,
        /// New content from a file
        #[arg(long)]
        content_file: Option<PathBuf>,
        /// Mention member IDs (UUID), repeatable
        #[arg(long)]
        mention: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Rename a deliverable
    Rename {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Deliverable ID
        id: String,
        /// New display name
        #[arg(long)]
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Move a deliverable under a different parent (or to root with --root)
    Move {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Deliverable ID to move
        id: String,
        /// New parent deliverable ID (folder)
        #[arg(long, conflicts_with = "root")]
        parent: Option<String>,
        /// Move to root of the goal's deliverable tree
        #[arg(long, conflicts_with = "parent")]
        root: bool,
        /// Display order (default 0.0)
        #[arg(long, default_value = "0.0")]
        order: f64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a deliverable
    Rm {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Deliverable ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Batch-move multiple deliverables
    BatchMove {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Comma-separated deliverable IDs to move
        #[arg(long)]
        ids: String,
        /// New parent deliverable ID
        #[arg(long, conflicts_with = "root")]
        parent: Option<String>,
        /// Move to root (clear parent)
        #[arg(long, conflicts_with = "parent")]
        root: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Batch-delete multiple deliverables
    BatchRm {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Comma-separated deliverable IDs to delete
        #[arg(long)]
        ids: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

fn split_ids(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn read_content(inline: &Option<String>, file: &Option<PathBuf>) -> Result<String> {
    match (inline, file) {
        (Some(s), None) => Ok(s.clone()),
        (None, Some(p)) => {
            std::fs::read_to_string(p).with_context(|| format!("Failed to read {}", p.display()))
        }
        (Some(_), Some(_)) => bail!("Specify only one of --content or --content-file"),
        (None, None) => bail!("Specify --content or --content-file"),
    }
}

pub async fn handle_deliverable(cmd: &DeliverableCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        DeliverableCommands::Add {
            goal,
            content,
            content_file,
            file,
            name,
            json,
        } => add_deliverable(client, goal, content, content_file, file, name, *json).await,
        DeliverableCommands::List { goal, json } => list_deliverables(client, goal, *json).await,
        DeliverableCommands::Update {
            goal,
            id,
            content,
            content_file,
            mention,
            json,
        } => {
            let body = read_content(content, content_file)?;
            let resp = client
                .update_deliverable(goal, id, &body, mention.clone())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Deliverable {id} updated");
            }
            Ok(())
        }
        DeliverableCommands::Rename {
            goal,
            id,
            name,
            json,
        } => {
            let resp = client.rename_deliverable(goal, id, name).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Renamed deliverable {id} to {name}");
            }
            Ok(())
        }
        DeliverableCommands::Move {
            goal,
            id,
            parent,
            root,
            order,
            json,
        } => {
            if parent.is_none() && !*root {
                bail!("Specify --parent <ID> or --root.");
            }
            let target = if *root { None } else { parent.clone() };
            let resp = client
                .move_deliverable(goal, id, target.clone(), *order)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                let dest = target.as_deref().unwrap_or("(root)");
                println!("Moved deliverable {id} to {dest}");
            }
            Ok(())
        }
        DeliverableCommands::Rm { goal, id, force } => {
            if !*force && !crate::cli::commands::confirm(&format!("Delete deliverable {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_deliverable(goal, id).await?;
            println!("Deliverable {id} deleted");
            Ok(())
        }
        DeliverableCommands::BatchMove {
            goal,
            ids,
            parent,
            root,
            json,
        } => {
            let id_list = split_ids(ids);
            if id_list.is_empty() {
                bail!("--ids must contain at least one ID");
            }
            if parent.is_none() && !*root {
                bail!("Specify --parent <ID> or --root.");
            }
            let target = if *root { None } else { parent.clone() };
            let resp = client
                .batch_move_deliverables(goal, id_list.clone(), target.clone())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                let dest = target.as_deref().unwrap_or("(root)");
                println!("Moved {} deliverables to {dest}", id_list.len());
            }
            Ok(())
        }
        DeliverableCommands::BatchRm { goal, ids, force } => {
            let id_list = split_ids(ids);
            if id_list.is_empty() {
                bail!("--ids must contain at least one ID");
            }
            if !*force
                && !crate::cli::commands::confirm(&format!(
                    "Delete {} deliverables?",
                    id_list.len()
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            client
                .batch_delete_deliverables(goal, id_list.clone())
                .await?;
            println!("Deleted {} deliverables", id_list.len());
            Ok(())
        }
    }
}

async fn list_deliverables(client: &ApiClient, goal: &str, json: bool) -> Result<()> {
    let resp = client.get_goal_deliverables(goal).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp.data)?);
        return Ok(());
    }
    if resp.data.deliverables.is_empty() {
        println!("No deliverables on goal {goal}");
        return Ok(());
    }
    println!("Deliverables on goal {goal} (total: {}):", resp.data.total);
    for d in &resp.data.deliverables {
        let kind = match d.node_type {
            DeliverableType::Folder => "folder",
            DeliverableType::Document => "document",
            DeliverableType::File => "file",
            DeliverableType::Link => "link",
        };
        println!("  [{kind}] {} ({})", d.display_name, d.id);
    }
    Ok(())
}

async fn add_deliverable(
    client: &ApiClient,
    goal: &str,
    content: &Option<String>,
    content_file: &Option<PathBuf>,
    file: &Option<PathBuf>,
    name: &Option<String>,
    json: bool,
) -> Result<()> {
    if let Some(text) = content {
        let display = name
            .clone()
            .ok_or_else(|| anyhow::anyhow!("--name is required when using --content"))?;
        let resp = client
            .create_document_deliverable(goal, &display, text)
            .await?;
        emit_create_result(&resp.data, json, &display, "document")?;
        return Ok(());
    }

    if let Some(path) = content_file {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read content file {}", path.display()))?;
        let display = name
            .clone()
            .or_else(|| filename_string(path))
            .ok_or_else(|| anyhow::anyhow!("Cannot derive --name from path {}", path.display()))?;
        let resp = client
            .create_document_deliverable(goal, &display, &text)
            .await?;
        emit_create_result(&resp.data, json, &display, "document")?;
        return Ok(());
    }

    if let Some(path) = file {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("Failed to stat file {}", path.display()))?;
        if !metadata.is_file() {
            bail!("{} is not a regular file", path.display());
        }
        let file_size = metadata.len() as i64;
        let file_name = filename_string(path)
            .ok_or_else(|| anyhow::anyhow!("Cannot derive file name from {}", path.display()))?;
        let display = name.clone().unwrap_or_else(|| file_name.clone());
        let content_type = guess_content_type(path)?;

        let resp = client
            .create_file_deliverable(goal, &display, &file_name, &content_type, file_size)
            .await?;

        let deliverable_id = resp.data.id.clone();
        let upload = resp.data.upload_request.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Server did not return an upload URL for the file deliverable")
        })?;

        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read file {}", path.display()))?;
        // アップロード失敗時はサーバ側に空のdeliverableが残るため、IDを案内する。
        if let Err(e) = client
            .upload_attachment(
                &upload.url,
                &upload.values,
                bytes,
                &file_name,
                &content_type,
            )
            .await
        {
            bail!(
                "{e}\n\nNote: a placeholder deliverable was created on the server with id={deliverable_id}. \
                 You may want to remove it from the web UI."
            );
        }

        emit_create_result(&resp.data, json, &display, "file")?;
        return Ok(());
    }

    bail!("Specify one of --content, --content-file, or --file");
}

fn emit_create_result(
    data: &crate::api::DeliverableCreateData,
    json: bool,
    display: &str,
    kind: &str,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(data)?);
    } else {
        println!("Added {kind} deliverable: {display} ({})", data.id);
    }
    Ok(())
}

fn filename_string(path: &Path) -> Option<String> {
    path.file_name().and_then(|s| s.to_str()).map(String::from)
}

/// 拡張子から Content-Type を推定する。サーバー側で許可されているタイプに合わせる。
fn guess_content_type(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());

    let ct = match ext.as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        Some("pdf") => "application/pdf",
        Some("csv") => "text/csv",
        Some("txt") => "text/plain",
        Some("md" | "markdown") => "text/markdown",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xls") => "application/vnd.ms-excel",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("ppt") => "application/vnd.ms-powerpoint",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => bail!(
            "Unsupported file extension: {}. Supported: jpg/jpeg/png/gif/webp/mp4/mov/webm/pdf/csv/txt/md/doc/docx/xls/xlsx/ppt/pptx",
            path.display()
        ),
    };
    Ok(ct.to_string())
}
