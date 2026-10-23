use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Subcommand;

use crate::api::ApiClient;
use crate::cli::commands::org::resolve_org_id;

/// Allowed MIME types for inline media (`internal/inlinemedia` — editor
/// paste/drop images and videos only, unlike the broader deliverable
/// upload flow).
fn guess_inline_media_content_type(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    let content_type = match ext.as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        other => bail!(
            "Unsupported file extension {:?}. Inline media only accepts images \
             (jpg/png/gif/webp) or videos (mp4/mov/webm).",
            other.unwrap_or("(none)")
        ),
    };
    Ok(content_type.to_string())
}

#[derive(Subcommand)]
pub enum MediaCommands {
    /// Get a temporary view URL for an inline media item
    View {
        /// Inline media ID
        id: String,
        /// Request a download-disposition URL instead of inline display
        #[arg(long)]
        download: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Upload a local image/video as inline media for a goal's editor content
    Upload {
        /// Local file path (image or video)
        path: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Goal ID the inline media belongs to
        #[arg(long)]
        goal: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Build a client whose `X-Organization-ID` header targets `org_id`.
/// Mirrors `member::client_for_org`.
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

pub async fn handle_media(cmd: &MediaCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        MediaCommands::View { id, download, json } => {
            let url = client.view_inline_media_url(id, *download).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({ "url": url }))?
                );
            } else {
                println!("{url}");
            }
            Ok(())
        }
        MediaCommands::Upload {
            path,
            org,
            goal,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let file_path = Path::new(path);
            let metadata = std::fs::metadata(file_path)
                .with_context(|| format!("Failed to stat file {path}"))?;
            if !metadata.is_file() {
                bail!("{path} is not a regular file");
            }
            let file_name = file_path
                .file_name()
                .and_then(|s| s.to_str())
                .map(String::from)
                .ok_or_else(|| anyhow::anyhow!("Cannot derive file name from {path}"))?;
            let content_type = guess_inline_media_content_type(file_path)?;

            let scoped = client_for_org(client, &org_id);
            let init = scoped
                .init_inline_media_upload(
                    &org_id,
                    goal,
                    &file_name,
                    &content_type,
                    metadata.len() as i64,
                )
                .await?;

            let bytes =
                std::fs::read(file_path).with_context(|| format!("Failed to read file {path}"))?;
            scoped
                .upload_attachment(
                    &init.upload.url,
                    &init.upload.values,
                    bytes,
                    &file_name,
                    &content_type,
                )
                .await
                .context("Failed to upload inline media")?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&init)?);
            } else {
                println!("Inline media uploaded: {}", init.inline_media_id);
                println!("  view URL: {}", init.view_url);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::guess_inline_media_content_type;
    use std::path::Path;

    #[test]
    fn guess_inline_media_content_type_maps_known_extensions() {
        assert_eq!(
            guess_inline_media_content_type(Path::new("a.png")).unwrap(),
            "image/png"
        );
        assert_eq!(
            guess_inline_media_content_type(Path::new("a.JPG")).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            guess_inline_media_content_type(Path::new("a.mp4")).unwrap(),
            "video/mp4"
        );
    }

    #[test]
    fn guess_inline_media_content_type_rejects_unsupported_extension() {
        let err = guess_inline_media_content_type(Path::new("a.pdf")).unwrap_err();
        assert!(err.to_string().contains("Unsupported file extension"));
    }
}
