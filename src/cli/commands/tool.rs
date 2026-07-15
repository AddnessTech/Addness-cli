use anyhow::{Context, Result, bail};
use clap::Subcommand;
use colored::Colorize;
use serde_json::{Map, Value};

use crate::api::{
    ApiClient, ToolCreateRequest, ToolExecuteRequest, ToolExecutor, ToolParameterRequest,
    ToolUpdateRequest,
};
use crate::cli::commands::confirm;
use crate::cli::commands::org::resolve_org_id;

/// Build a client whose `X-Organization-ID` header targets `org_id`.
/// Mirrors `skill::client_for_org` / `meeting::client_for_org`.
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

/// Parse a raw JSON object flag value (`--executor-config-json`/
/// `--parameters-json`'s object form is not used here, but the map-shaped
/// flags share this parser with `skill.rs`'s `--examples-json`).
fn parse_json_object(raw: &str, flag_name: &str) -> Result<Map<String, Value>> {
    let value: Value =
        serde_json::from_str(raw).with_context(|| format!("{flag_name} must be valid JSON"))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => bail!("{flag_name} must be a JSON object, e.g. '{{\"key\":\"value\"}}'"),
    }
}

/// Parse `--parameters-json`: a JSON array of
/// `{"name":..,"type":..,"required":..,"description":..,"default":..,"enum":[..]}`.
fn parse_tool_parameters_json(raw: &str) -> Result<Vec<ToolParameterRequest>> {
    serde_json::from_str(raw).context(
        "--parameters-json must be a JSON array of \
         {name, type, description, required?, default?, enum?}",
    )
}

#[derive(Subcommand)]
pub enum ToolCommands {
    /// Create a tool (an executable action an AI skill can invoke)
    Create {
        /// Tool name
        #[arg(long)]
        name: String,
        /// Tool description
        #[arg(long)]
        description: String,
        /// Execution backend
        #[arg(long, value_enum)]
        executor: ToolExecutor,
        /// Raw JSON object with executor-specific configuration
        #[arg(long)]
        executor_config_json: String,
        /// Raw JSON array of parameter definitions (see `addness tool create --help`)
        #[arg(long)]
        parameters_json: Option<String>,
        /// Require user confirmation before executing this tool
        #[arg(long)]
        requires_confirmation: bool,
        /// Environment this tool may run in (repeatable, e.g. "development")
        #[arg(long = "allowed-environment")]
        allowed_environment: Vec<String>,
        /// Maximum execution time in seconds
        #[arg(long)]
        max_execution_time: Option<i64>,
        /// Make this tool visible to the whole organization (default: creator only)
        #[arg(long)]
        is_public: bool,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List tools in the organization
    List {
        /// Max number of results (default: 20)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u32>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Search tools by keyword
    Search {
        /// Search keyword
        query: String,
        /// Max number of results (default: 30)
        #[arg(long)]
        limit: Option<u32>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a single tool
    Get {
        /// Tool ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a tool (partial update; executor backend cannot be changed)
    Update {
        /// Tool ID
        id: String,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// Replace the executor-specific configuration (raw JSON object)
        #[arg(long)]
        executor_config_json: Option<String>,
        /// Replace the parameter definitions (raw JSON array)
        #[arg(long)]
        parameters_json: Option<String>,
        /// Require user confirmation before executing this tool
        #[arg(long)]
        requires_confirmation: Option<bool>,
        /// Replace the allowed environments list (repeatable; omit to leave unchanged)
        #[arg(long = "allowed-environment")]
        allowed_environment: Vec<String>,
        /// Maximum execution time in seconds
        #[arg(long)]
        max_execution_time: Option<i64>,
        /// Make this tool visible to the whole organization
        #[arg(long)]
        is_public: Option<bool>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a tool
    Delete {
        /// Tool ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip the confirmation prompt
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Execute a tool
    Execute {
        /// Tool ID
        id: String,
        /// Raw JSON object of parameter values to pass to the tool
        #[arg(long)]
        parameters_json: String,
        /// Target environment (must be one of the tool's allowed environments, if set)
        #[arg(long)]
        environment: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_tool(cmd: &ToolCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ToolCommands::Create {
            name,
            description,
            executor,
            executor_config_json,
            parameters_json,
            requires_confirmation,
            allowed_environment,
            max_execution_time,
            is_public,
            org,
            json,
        } => {
            let executor_config =
                parse_json_object(executor_config_json, "--executor-config-json")?;
            let parameters = parameters_json
                .as_deref()
                .map(parse_tool_parameters_json)
                .transpose()?
                .unwrap_or_default();
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = ToolCreateRequest {
                name: name.clone(),
                description: description.clone(),
                executor: *executor,
                executor_config,
                parameters,
                requires_confirmation: *requires_confirmation,
                allowed_environments: allowed_environment.clone(),
                max_execution_time: *max_execution_time,
                is_public: *is_public,
            };
            let tool = scoped.create_tool(&org_id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&tool)?);
            } else {
                println!("Created tool {} — {}", tool.id, tool.name);
            }
            Ok(())
        }
        ToolCommands::List {
            limit,
            offset,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.list_tools(&org_id, *limit, *offset).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.tools.is_empty() {
                println!("{}", "No tools found.".dimmed());
            } else {
                for tool in &resp.tools {
                    println!("{} — {} [{}]", tool.id, tool.name, tool.executor);
                }
                println!("{} total", resp.total);
            }
            Ok(())
        }
        ToolCommands::Search {
            query,
            limit,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.search_tools(&org_id, query, *limit).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.tools.is_empty() {
                println!("{}", "No matching tools found.".dimmed());
            } else {
                for tool in &resp.tools {
                    println!("{} — {} [{}]", tool.id, tool.name, tool.executor);
                }
            }
            Ok(())
        }
        ToolCommands::Get { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let tool = scoped.get_tool(&org_id, id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&tool)?);
            } else {
                println!("{} — {} [{}]", tool.id, tool.name, tool.executor);
                println!("{}", tool.description);
            }
            Ok(())
        }
        ToolCommands::Update {
            id,
            name,
            description,
            executor_config_json,
            parameters_json,
            requires_confirmation,
            allowed_environment,
            max_execution_time,
            is_public,
            org,
            json,
        } => {
            if name.is_none()
                && description.is_none()
                && executor_config_json.is_none()
                && parameters_json.is_none()
                && requires_confirmation.is_none()
                && allowed_environment.is_empty()
                && max_execution_time.is_none()
                && is_public.is_none()
            {
                bail!("At least one field to update must be specified");
            }
            let executor_config = executor_config_json
                .as_deref()
                .map(|raw| parse_json_object(raw, "--executor-config-json"))
                .transpose()?;
            let parameters = parameters_json
                .as_deref()
                .map(parse_tool_parameters_json)
                .transpose()?;
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = ToolUpdateRequest {
                name: name.clone(),
                description: description.clone(),
                executor_config,
                parameters,
                requires_confirmation: *requires_confirmation,
                allowed_environments: (!allowed_environment.is_empty())
                    .then(|| allowed_environment.clone()),
                max_execution_time: *max_execution_time,
                is_public: *is_public,
            };
            let tool = scoped.update_tool(&org_id, id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&tool)?);
            } else {
                println!("Updated tool {} — {}", tool.id, tool.name);
            }
            Ok(())
        }
        ToolCommands::Delete {
            id,
            org,
            force,
            json,
        } => {
            if !*force && !confirm(&format!("Delete tool {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped.delete_tool(&org_id, id).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"deleted": true, "id": id}))?
                );
            } else {
                println!("Deleted tool {id}");
            }
            Ok(())
        }
        ToolCommands::Execute {
            id,
            parameters_json,
            environment,
            org,
            json,
        } => {
            let parameters = parse_json_object(parameters_json, "--parameters-json")?;
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = ToolExecuteRequest {
                parameters,
                environment: environment.clone().unwrap_or_default(),
            };
            let resp = scoped.execute_tool(&org_id, id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("outcome: {} ({}ms)", resp.outcome, resp.execution_time_ms);
                if !resp.stdout.is_empty() {
                    println!("{}", resp.stdout);
                }
                if !resp.stderr.is_empty() {
                    eprintln!("{}", resp.stderr);
                }
                if !resp.error.is_empty() {
                    println!("error: {}", resp.error);
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_json_object, parse_tool_parameters_json};

    #[test]
    fn parse_json_object_accepts_valid_object() {
        let map = parse_json_object(r#"{"url":"https://example.com"}"#, "--executor-config-json")
            .unwrap();
        assert_eq!(map["url"], "https://example.com");
    }

    #[test]
    fn parse_json_object_rejects_non_object() {
        let err = parse_json_object("[1,2,3]", "--executor-config-json").unwrap_err();
        assert!(err.to_string().contains("--executor-config-json"));
    }

    #[test]
    fn parse_tool_parameters_json_accepts_valid_array() {
        let params = parse_tool_parameters_json(
            r#"[{"name":"path","type":"string","description":"file path","required":true}]"#,
        )
        .unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "path");
        assert!(params[0].required);
    }

    #[test]
    fn parse_tool_parameters_json_rejects_invalid_json() {
        let err = parse_tool_parameters_json("not json").unwrap_err();
        assert!(err.to_string().contains("--parameters-json"));
    }
}
