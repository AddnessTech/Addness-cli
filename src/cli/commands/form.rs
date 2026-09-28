use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Subcommand, ValueEnum};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::api::{ApiClient, FormListParams, FormResponseListParams};
use crate::cli::commands::org::resolve_org_id;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum FormCsvView {
    Raw,
    Labels,
}

impl FormCsvView {
    fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Labels => "labels",
        }
    }
}

#[derive(Subcommand)]
pub enum FormCommands {
    /// List forms accessible to you in the organization
    List {
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        goal: Option<String>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=100))]
        limit: Option<u8>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show a form definition, including settings and revision
    Get {
        id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Create a draft from a JSON definition file (requires title)
    Create {
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        definition_file: String,
        #[arg(long)]
        json: bool,
    },
    /// Replace the whole definition, guarded by its current revision
    Replace {
        id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        definition_file: String,
        #[arg(long)]
        json: bool,
    },
    /// Update only fields present in a JSON definition file
    Patch {
        id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        definition_file: String,
        #[arg(long)]
        json: bool,
    },
    /// Publish a draft or reopen a published form
    Publish {
        id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        json: bool,
    },
    /// Stop accepting responses while keeping the public page visible
    Close {
        id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        json: bool,
    },
    /// Hide the public page while preserving its public ID and responses
    Unpublish {
        id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        json: bool,
    },
    /// Delete a form and remove its responses from management APIs
    Delete {
        id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// List submitted responses and snapshots
    Responses {
        id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=100))]
        limit: Option<u8>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        submitted_at_from: Option<String>,
        #[arg(long)]
        submitted_at_before: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show a submitted response and its question snapshot
    Response {
        id: String,
        response_id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Permanently delete one submitted response
    DeleteResponse {
        id: String,
        response_id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// Permanently delete every response while keeping the form
    DeleteAllResponses {
        id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show counts and quiz score statistics
    Summary {
        id: String,
        #[arg(long)]
        org: Option<String>,
        /// IANA time zone for daily counts (default: UTC)
        #[arg(long)]
        time_zone: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Save response CSV to a new file without overwriting existing data
    ExportCsv {
        id: String,
        #[arg(long)]
        org: Option<String>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 268_435_456, value_parser = clap::value_parser!(u32).range(1..=268_435_456))]
        max_bytes: u32,
        /// raw preserves IDs; labels shows question and choice names
        #[arg(long, value_enum, default_value = "raw")]
        view: FormCsvView,
        /// Include responses submitted at or after this timezone-aware RFC3339 timestamp
        #[arg(long)]
        submitted_at_from: Option<String>,
        /// Include responses submitted before this timezone-aware RFC3339 timestamp
        #[arg(long)]
        submitted_at_before: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

impl FormCommands {
    pub fn outputs_json(&self) -> bool {
        match self {
            Self::List { json, .. }
            | Self::Get { json, .. }
            | Self::Create { json, .. }
            | Self::Replace { json, .. }
            | Self::Patch { json, .. }
            | Self::Publish { json, .. }
            | Self::Close { json, .. }
            | Self::Unpublish { json, .. }
            | Self::Delete { json, .. }
            | Self::Responses { json, .. }
            | Self::Response { json, .. }
            | Self::DeleteResponse { json, .. }
            | Self::DeleteAllResponses { json, .. }
            | Self::Summary { json, .. }
            | Self::ExportCsv { json, .. } => *json,
        }
    }
}

fn scoped_client(client: &ApiClient, org: Option<&str>) -> Result<(ApiClient, String)> {
    let org_id = resolve_org_id(org)?;
    checked_id(&org_id, "organization ID")?;
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.clone()));
    Ok((scoped, org_id))
}

fn checked_id<'a>(id: &'a str, name: &str) -> Result<&'a str> {
    Uuid::parse_str(id).with_context(|| format!("Invalid {name}: {id}"))?;
    Ok(id)
}

fn read_json_object(path: &str) -> Result<Value> {
    let content = fs::read_to_string(Path::new(path))
        .with_context(|| format!("Failed to read definition file: {path}"))?;
    let value: Value = serde_json::from_str(&content)
        .with_context(|| format!("Invalid JSON in definition file: {path}"))?;
    if !value.is_object() {
        bail!("Definition file must contain a JSON object: {path}");
    }
    Ok(value)
}

fn with_revision(mut value: Value, revision: u64) -> Result<Value> {
    if revision == 0 {
        bail!("Revision must be at least 1");
    }
    let object = value
        .as_object_mut()
        .expect("read_json_object ensures object");
    if object.contains_key("revision") {
        bail!("Definition file must omit revision; supply it with --revision");
    }
    object.insert("revision".to_string(), json!(revision));
    Ok(value)
}

fn print_form(data: &Value, json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(data)?);
        return Ok(());
    }
    let form = data.get("form").context("API response has no form")?;
    println!(
        "{}  {}  {}  revision {}",
        form["id"].as_str().unwrap_or("?"),
        form["title"].as_str().unwrap_or("?"),
        form["status"].as_str().unwrap_or("?"),
        form["revision"].as_u64().unwrap_or(0)
    );
    if let Some(public_id) = form["publicId"].as_str() {
        println!("public ID: {public_id}");
    }
    Ok(())
}

async fn save_csv_response(
    response: reqwest::Response,
    output: &Path,
    max_bytes: u32,
) -> Result<u64> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(output)
        .await
        .with_context(|| format!("Failed to create output file: {}", output.display()))?;
    let write_result = async {
        let mut stream = response.bytes_stream();
        let mut written = 0_u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read CSV response")?;
            written += chunk.len() as u64;
            if written > u64::from(max_bytes) {
                bail!("CSV response exceeds --max-bytes ({max_bytes})");
            }
            file.write_all(&chunk)
                .await
                .with_context(|| format!("Failed to write CSV to {}", output.display()))?;
        }
        file.flush().await.context("Failed to flush CSV output")?;
        Ok(written)
    }
    .await;
    drop(file);
    match write_result {
        Ok(bytes) => Ok(bytes),
        Err(err) => {
            if let Err(cleanup_err) = tokio::fs::remove_file(output).await {
                return Err(err.context(format!(
                    "Failed to remove incomplete CSV at {}: {cleanup_err}",
                    output.display()
                )));
            }
            Err(err)
        }
    }
}

pub async fn handle_form(command: &FormCommands, client: &ApiClient) -> Result<()> {
    match command {
        FormCommands::List {
            org,
            goal,
            query,
            limit,
            cursor,
            json,
        } => {
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            if let Some(goal_id) = goal {
                checked_id(goal_id, "goal ID")?;
            }
            let data = client
                .list_forms(
                    &org_id,
                    FormListParams {
                        goal_id: goal.as_deref(),
                        query: query.as_deref(),
                        limit: limit.map(u16::from),
                        cursor: cursor.as_deref(),
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                for form in data["forms"]
                    .as_array()
                    .context("API response has no forms")?
                {
                    println!(
                        "{}  {}  {}",
                        form["id"].as_str().unwrap_or("?"),
                        form["status"].as_str().unwrap_or("?"),
                        form["title"].as_str().unwrap_or("?")
                    );
                }
                if let Some(next) = data["nextCursor"].as_str() {
                    println!("next cursor: {next}");
                }
            }
        }
        FormCommands::Get { id, org, json: _ } => {
            checked_id(id, "form ID")?;
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            let data = client.get_form(&org_id, id).await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        FormCommands::Create {
            org,
            definition_file,
            json,
        } => {
            let definition = read_json_object(definition_file)?;
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            print_form(&client.create_form(&org_id, &definition).await?, *json)?;
        }
        FormCommands::Replace {
            id,
            org,
            revision,
            definition_file,
            json,
        }
        | FormCommands::Patch {
            id,
            org,
            revision,
            definition_file,
            json,
        } => {
            checked_id(id, "form ID")?;
            let body = with_revision(read_json_object(definition_file)?, *revision)?;
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            let data = if matches!(command, FormCommands::Replace { .. }) {
                client.replace_form(&org_id, id, &body).await?
            } else {
                client.patch_form(&org_id, id, &body).await?
            };
            print_form(&data, *json)?;
        }
        FormCommands::Publish {
            id,
            org,
            revision,
            json,
        }
        | FormCommands::Close {
            id,
            org,
            revision,
            json,
        }
        | FormCommands::Unpublish {
            id,
            org,
            revision,
            json,
        } => {
            checked_id(id, "form ID")?;
            if *revision == 0 {
                bail!("Revision must be at least 1");
            }
            let action = match command {
                FormCommands::Publish { .. } => "publish",
                FormCommands::Close { .. } => "close",
                _ => "unpublish",
            };
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            let data = client
                .change_form_state(&org_id, id, action, *revision)
                .await?;
            print_form(&data, *json)?;
        }
        FormCommands::Delete {
            id,
            org,
            force,
            json,
        } => {
            checked_id(id, "form ID")?;
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            if !force && !super::confirm(&format!("Delete form {id} and its responses?"))? {
                bail!("Cancelled");
            }
            client.delete_form(&org_id, id).await?;
            if *json {
                println!("{}", json!({ "deleted": true, "id": id }));
            } else {
                println!("Form {id} deleted");
            }
        }
        FormCommands::Responses {
            id,
            org,
            limit,
            cursor,
            submitted_at_from,
            submitted_at_before,
            json: _,
        } => {
            checked_id(id, "form ID")?;
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            let data = client
                .list_form_responses(
                    &org_id,
                    id,
                    FormResponseListParams {
                        limit: limit.map(u16::from),
                        cursor: cursor.as_deref(),
                        submitted_at_from: submitted_at_from.as_deref(),
                        submitted_at_before: submitted_at_before.as_deref(),
                    },
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        FormCommands::Response {
            id,
            response_id,
            org,
            json: _,
        } => {
            checked_id(id, "form ID")?;
            checked_id(response_id, "response ID")?;
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            let data = client.get_form_response(&org_id, id, response_id).await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        FormCommands::DeleteResponse {
            id,
            response_id,
            org,
            force,
            json,
        } => {
            checked_id(id, "form ID")?;
            checked_id(response_id, "response ID")?;
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            if !force
                && !super::confirm(&format!(
                    "Permanently delete response {response_id} from form {id}?"
                ))?
            {
                bail!("Cancelled");
            }
            client
                .delete_form_response(&org_id, id, response_id)
                .await?;
            if *json {
                println!(
                    "{}",
                    json!({ "deleted": true, "formId": id, "responseId": response_id })
                );
            } else {
                println!("Deleted response {response_id} from form {id}");
            }
        }
        FormCommands::DeleteAllResponses {
            id,
            org,
            force,
            json,
        } => {
            checked_id(id, "form ID")?;
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            if !force
                && !super::confirm(&format!(
                    "Permanently delete every response from form {id}?"
                ))?
            {
                bail!("Cancelled");
            }
            client.delete_all_form_responses(&org_id, id).await?;
            if *json {
                println!("{}", json!({ "deletedAll": true, "formId": id }));
            } else {
                println!("Deleted all responses from form {id}");
            }
        }
        FormCommands::Summary {
            id,
            org,
            time_zone,
            json: _,
        } => {
            checked_id(id, "form ID")?;
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            let data = client
                .get_form_summary(&org_id, id, time_zone.as_deref())
                .await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        FormCommands::ExportCsv {
            id,
            org,
            output,
            max_bytes,
            view,
            submitted_at_from,
            submitted_at_before,
            json,
        } => {
            checked_id(id, "form ID")?;
            if output.exists() {
                bail!("Output file already exists: {}", output.display());
            }
            let (client, org_id) = scoped_client(client, org.as_deref())?;
            let response = client
                .get_form_responses_csv(
                    &org_id,
                    id,
                    *max_bytes,
                    view.as_str(),
                    submitted_at_from.as_deref(),
                    submitted_at_before.as_deref(),
                )
                .await?;
            let bytes = save_csv_response(response, output, *max_bytes).await?;
            if *json {
                let mut result =
                    json!({ "formId": id, "path": output, "bytes": bytes, "view": view.as_str() });
                if let Some(from) = submitted_at_from {
                    result["submittedAtFrom"] = json!(from);
                }
                if let Some(before) = submitted_at_before {
                    result["submittedAtBefore"] = json!(before);
                }
                println!("{result}");
            } else {
                println!("Saved {bytes} bytes to {}", output.display());
            }
        }
    }
    Ok(())
}
