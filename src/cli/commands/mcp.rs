use std::io::Read;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};

use crate::api::{ApiClient, McpScope};

#[derive(Args)]
pub struct McpArgs {
    /// Addness MCP catalog (the token must be authorized for this scope)
    #[arg(long, value_enum, default_value = "organization", global = true)]
    scope: McpScope,
    /// Override the current organization for this invocation
    #[arg(long, global = true)]
    pub org: Option<String>,
    /// Output the full MCP result as JSON, including structured content and errors
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    command: McpCommands,
}

#[derive(Subcommand)]
pub enum McpCommands {
    /// List every available tool, fetching all catalog pages
    #[command(alias = "tools")]
    List {
        /// Filter by tool name or description (case insensitive)
        #[arg(long)]
        query: Option<String>,
    },
    /// Show the current input schema and annotations of a tool
    Schema { name: String },
    /// Invoke a tool once; writes take effect immediately and are never retried
    Call {
        name: String,
        /// Tool arguments as a JSON object
        #[arg(long, conflicts_with = "args_file")]
        args: Option<String>,
        /// Read a JSON object from a file, or '-' for stdin
        #[arg(long)]
        args_file: Option<String>,
    },
    /// Show server capabilities, instructions, and negotiated protocol
    Info,
}

fn read_arguments(args: Option<&str>, file: Option<&str>) -> Result<Value> {
    let text = if let Some(file) = file {
        if file == "-" {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            text
        } else {
            std::fs::read_to_string(file).with_context(|| format!("Could not read {file}"))?
        }
    } else {
        args.unwrap_or("{}").to_string()
    };
    let arguments: Value = serde_json::from_str(&text).context("Arguments must be valid JSON")?;
    if !arguments.is_object() {
        bail!("Arguments must be a JSON object");
    }
    Ok(arguments)
}

pub async fn handle_mcp(args: &McpArgs, client: &ApiClient) -> Result<()> {
    // 入力エラーは接続より先に検出する。
    let arguments = match &args.command {
        McpCommands::Call {
            args, args_file, ..
        } => read_arguments(args.as_deref(), args_file.as_deref())?,
        _ => json!({}),
    };
    let mut client = client.clone();
    if let Some(org) = &args.org {
        client.set_org_id(Some(org.clone()));
    }
    let mut session = client.connect_mcp(args.scope).await?;
    let result: Result<Value> = async {
        match &args.command {
            McpCommands::Info => Ok(session.information.clone()),
            McpCommands::Call { name, .. } => session.call_tool(name, arguments).await,
            McpCommands::List { query } => {
                let mut result = session.list_tools().await?;
                if let Some(query) = query {
                    let query = query.to_lowercase();
                    result["tools"]
                        .as_array_mut()
                        .expect("validated tool list")
                        .retain(|tool| {
                            ["name", "description", "title"].iter().any(|field| {
                                tool[field]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_lowercase()
                                    .contains(&query)
                            })
                        });
                }
                Ok(result)
            }
            McpCommands::Schema { name } => session.list_tools().await?["tools"]
                .as_array()
                .expect("validated tool list")
                .iter()
                .find(|tool| tool["name"] == *name)
                .cloned()
                .with_context(|| {
                    format!("Tool '{name}' is not in this catalog. Use addness mcp list --json")
                }),
        }
    }
    .await;
    session.close().await;
    let result = result?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if let McpCommands::List { .. } = &args.command {
        for tool in result["tools"].as_array().expect("validated tool list") {
            println!(
                "{}\t{}",
                tool["name"].as_str().unwrap_or(""),
                tool["description"]
                    .as_str()
                    .unwrap_or("")
                    .lines()
                    .next()
                    .unwrap_or("")
            );
        }
    } else if let Some(content) = result["content"].as_array() {
        for item in content {
            if let Some(text) = item["text"].as_str() {
                println!("{text}");
            } else {
                println!("{}", serde_json::to_string_pretty(item)?);
            }
        }
        if content.is_empty()
            && let Some(structured) = result.get("structuredContent")
        {
            println!("{}", serde_json::to_string_pretty(structured)?);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    if result["isError"] == true {
        bail!("MCP tool returned an error (see result above)");
    }
    Ok(())
}
