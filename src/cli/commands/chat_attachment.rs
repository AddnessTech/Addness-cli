use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Args;
use tokio::io::AsyncWriteExt;

use crate::api::ApiClient;

const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Args)]
pub struct AttachmentArgs {
    #[arg(long)]
    room: uuid::Uuid,
    #[arg(long)]
    message: uuid::Uuid,
    #[arg(long)]
    attachment: uuid::Uuid,
    /// Save to a new file; existing files are never overwritten (maximum 64 MiB)
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    pub json: bool,
}

pub async fn handle_attachment(args: &AttachmentArgs, client: &ApiClient) -> Result<()> {
    // 保存先が既存なら、署名URLの発行も行わない。
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.output)
        .await
        .context("Could not create attachment output (it must be a new file)")?;
    let result: Result<u64> = async {
        let url = client
            .get_chat_attachment_url(
                &args.room.to_string(),
                &args.message.to_string(),
                &args.attachment.to_string(),
            )
            .await?;
        download(&url, &mut file).await
    }
    .await;
    drop(file);
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = tokio::fs::remove_file(&args.output).await;
            return Err(error);
        }
    };
    let path = absolute_output(&args.output)?;
    if args.json {
        println!(
            "{}",
            serde_json::json!({"path":path,"bytes":bytes,"attachment_id":args.attachment.to_string()})
        );
    } else {
        println!("Saved {bytes} bytes to {}", path.display());
    }
    Ok(())
}

fn absolute_output(path: &Path) -> Result<PathBuf> {
    Ok(std::fs::canonicalize(path)?)
}

async fn download(url: &str, file: &mut tokio::fs::File) -> Result<u64> {
    let parsed = reqwest::Url::parse(url).context("Invalid attachment URL")?;
    let local_http = parsed.scheme() == "http"
        && parsed
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
    if parsed.scheme() != "https" && !local_http {
        bail!("Attachment URL must use HTTPS");
    }
    // 添付先へ Addness の Authorization / 組織ヘッダーを送らない。
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut response = client
        .get(parsed)
        .send()
        .await
        .map_err(reqwest::Error::without_url)?;
    if !response.status().is_success() {
        bail!("Attachment download failed ({})", response.status());
    }
    if response
        .content_length()
        .is_some_and(|bytes| bytes > MAX_BYTES)
    {
        bail!("Attachment exceeds the 64 MiB download limit");
    }
    let mut bytes = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        bytes += chunk.len() as u64;
        if bytes > MAX_BYTES {
            bail!("Attachment exceeds the 64 MiB download limit");
        }
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(bytes)
}
