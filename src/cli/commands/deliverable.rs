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
        DeliverableCommands::List { goal, json } => {
            let resp = client.get_goal_deliverables(goal).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
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
            }
            Ok(())
        }
    }
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
