use anyhow::{Context, Result, bail};
use clap::Subcommand;
use colored::Colorize;
use serde_json::{Map, Value};

use crate::api::{
    ApiClient, SkillCreateRequest, SkillResourceCreateRequest, SkillResourceUpdateRequest,
    SkillUpdateRequest,
};
use crate::cli::commands::confirm;
use crate::cli::commands::org::resolve_org_id;

/// Build a client whose `X-Organization-ID` header targets `org_id`.
/// Mirrors `meeting::client_for_org` / `execution::client_for_org`.
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

/// Parse a `--examples-json`/`--executor-config-json`-style flag value as a
/// JSON object (the backend's `map[string]interface{}` fields require an
/// object, not an array or scalar).
fn parse_json_object(raw: &str, flag_name: &str) -> Result<Map<String, Value>> {
    let value: Value =
        serde_json::from_str(raw).with_context(|| format!("{flag_name} must be valid JSON"))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => bail!("{flag_name} must be a JSON object, e.g. '{{\"key\":\"value\"}}'"),
    }
}

#[derive(Subcommand)]
pub enum SkillCommands {
    /// Create a skill (a reusable AI prompt template with preferred tools)
    Create {
        /// Skill name
        #[arg(long)]
        name: String,
        /// Skill description
        #[arg(long)]
        description: String,
        /// Prompt template used when the skill is invoked
        #[arg(long)]
        prompt_template: String,
        /// Ordered thinking step (repeatable)
        #[arg(long = "thinking-step")]
        thinking_step: Vec<String>,
        /// Preferred tool name/ID for this skill (repeatable)
        #[arg(long = "preferred-tool")]
        preferred_tool: Vec<String>,
        /// Tool execution order hint (free-form)
        #[arg(long)]
        tool_execution_order: Option<String>,
        /// Require user confirmation before running this skill
        #[arg(long)]
        requires_confirmation: bool,
        /// Raw JSON object of example input/output pairs
        #[arg(long)]
        examples_json: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List skills in the organization
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
    /// List general (platform-provided) skills available to the organization
    General {
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
    /// Search skills by keyword
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
    /// Get a single skill
    Get {
        /// Skill ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a skill (partial update; only creator or root owner may edit)
    Update {
        /// Skill ID
        id: String,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// New prompt template
        #[arg(long)]
        prompt_template: Option<String>,
        /// Replace the thinking steps list (repeatable; omit to leave unchanged)
        #[arg(long = "thinking-step")]
        thinking_step: Vec<String>,
        /// Replace the preferred tools list (repeatable; omit to leave unchanged)
        #[arg(long = "preferred-tool")]
        preferred_tool: Vec<String>,
        /// New tool execution order hint
        #[arg(long)]
        tool_execution_order: Option<String>,
        /// Require user confirmation before running this skill
        #[arg(long)]
        requires_confirmation: Option<bool>,
        /// Raw JSON object of example input/output pairs
        #[arg(long)]
        examples_json: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a skill
    Delete {
        /// Skill ID
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
    /// Show usage/success-rate performance for a skill
    Performance {
        /// Skill ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage supplementary skill resources (reference text attached to a skill)
    Resource {
        #[command(subcommand)]
        command: SkillResourceCommands,
    },
    /// Accept or reject AI-suggested skill improvements
    Refinement {
        #[command(subcommand)]
        command: SkillRefinementCommands,
    },
}

#[derive(Subcommand)]
pub enum SkillResourceCommands {
    /// Create a skill resource
    Create {
        /// Skill ID
        skill_id: String,
        /// Resource name
        #[arg(long)]
        name: String,
        /// Content type (free-form, e.g. "text/markdown")
        #[arg(long)]
        content_type: Option<String>,
        /// Resource content
        #[arg(long)]
        content: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List a skill's resources (content omitted; use `get` for full content)
    List {
        /// Skill ID
        skill_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a single skill resource (with full content)
    Get {
        /// Skill ID
        skill_id: String,
        /// Resource ID
        resource_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a skill resource (partial update)
    Update {
        /// Skill ID
        skill_id: String,
        /// Resource ID
        resource_id: String,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// New content
        #[arg(long)]
        content: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a skill resource
    Delete {
        /// Skill ID
        skill_id: String,
        /// Resource ID
        resource_id: String,
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
}

#[derive(Subcommand)]
pub enum SkillRefinementCommands {
    /// Accept an AI-suggested skill refinement (updates the skill in place)
    Accept {
        /// Refinement ID
        refinement_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Reject an AI-suggested skill refinement
    Reject {
        /// Refinement ID
        refinement_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_skill(cmd: &SkillCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        SkillCommands::Create {
            name,
            description,
            prompt_template,
            thinking_step,
            preferred_tool,
            tool_execution_order,
            requires_confirmation,
            examples_json,
            org,
            json,
        } => {
            let examples = examples_json
                .as_deref()
                .map(|raw| parse_json_object(raw, "--examples-json"))
                .transpose()?;
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = SkillCreateRequest {
                name: name.clone(),
                description: description.clone(),
                prompt_template: prompt_template.clone(),
                thinking_steps: thinking_step.clone(),
                preferred_tools: preferred_tool.clone(),
                tool_execution_order: tool_execution_order.clone(),
                requires_confirmation: *requires_confirmation,
                examples,
            };
            let skill = scoped.create_skill(&org_id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&skill)?);
            } else {
                println!("Created skill {} — {}", skill.id, skill.name);
            }
            Ok(())
        }
        SkillCommands::List {
            limit,
            offset,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.list_skills(&org_id, *limit, *offset).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.skills.is_empty() {
                println!("{}", "No skills found.".dimmed());
            } else {
                for skill in &resp.skills {
                    println!("{} — {}", skill.id, skill.name);
                }
                println!("{} total", resp.total);
            }
            Ok(())
        }
        SkillCommands::General {
            limit,
            offset,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.list_general_skills(&org_id, *limit, *offset).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.skills.is_empty() {
                println!("{}", "No general skills found.".dimmed());
            } else {
                for skill in &resp.skills {
                    println!("{} — {}", skill.id, skill.name);
                }
                println!("{} total", resp.total);
            }
            Ok(())
        }
        SkillCommands::Search {
            query,
            limit,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.search_skills(&org_id, query, *limit).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.skills.is_empty() {
                println!("{}", "No matching skills found.".dimmed());
            } else {
                for skill in &resp.skills {
                    println!("{} — {}", skill.id, skill.name);
                }
            }
            Ok(())
        }
        SkillCommands::Get { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let skill = scoped.get_skill(&org_id, id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&skill)?);
            } else {
                println!("{} — {}", skill.id, skill.name);
                println!("{}", skill.description);
            }
            Ok(())
        }
        SkillCommands::Update {
            id,
            name,
            description,
            prompt_template,
            thinking_step,
            preferred_tool,
            tool_execution_order,
            requires_confirmation,
            examples_json,
            org,
            json,
        } => {
            if name.is_none()
                && description.is_none()
                && prompt_template.is_none()
                && thinking_step.is_empty()
                && preferred_tool.is_empty()
                && tool_execution_order.is_none()
                && requires_confirmation.is_none()
                && examples_json.is_none()
            {
                bail!("At least one field to update must be specified");
            }
            let examples = examples_json
                .as_deref()
                .map(|raw| parse_json_object(raw, "--examples-json"))
                .transpose()?;
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = SkillUpdateRequest {
                name: name.clone(),
                description: description.clone(),
                prompt_template: prompt_template.clone(),
                thinking_steps: (!thinking_step.is_empty()).then(|| thinking_step.clone()),
                preferred_tools: (!preferred_tool.is_empty()).then(|| preferred_tool.clone()),
                tool_execution_order: tool_execution_order.clone(),
                requires_confirmation: *requires_confirmation,
                examples,
            };
            let skill = scoped.update_skill(&org_id, id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&skill)?);
            } else {
                println!("Updated skill {} — {}", skill.id, skill.name);
            }
            Ok(())
        }
        SkillCommands::Delete {
            id,
            org,
            force,
            json,
        } => {
            if !*force && !confirm(&format!("Delete skill {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped.delete_skill(&org_id, id).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"deleted": true, "id": id}))?
                );
            } else {
                println!("Deleted skill {id}");
            }
            Ok(())
        }
        SkillCommands::Performance { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let perf = scoped.get_skill_performance(&org_id, id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&perf)?);
            } else {
                println!(
                    "{} — used {} time(s), {:.1}% success (v{})",
                    perf.skill_name, perf.usage_count, perf.success_rate, perf.version
                );
            }
            Ok(())
        }
        SkillCommands::Resource { command } => handle_resource(command, client).await,
        SkillCommands::Refinement { command } => handle_refinement(command, client).await,
    }
}

async fn handle_resource(cmd: &SkillResourceCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        SkillResourceCommands::Create {
            skill_id,
            name,
            content_type,
            content,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = SkillResourceCreateRequest {
                name: name.clone(),
                content_type: content_type.clone(),
                content: content.clone(),
            };
            let resource = scoped
                .create_skill_resource(&org_id, skill_id, &req)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resource)?);
            } else {
                println!("Created resource {} — {}", resource.id, resource.name);
            }
            Ok(())
        }
        SkillResourceCommands::List {
            skill_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resources = scoped.list_skill_resources(&org_id, skill_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resources)?);
            } else if resources.is_empty() {
                println!("{}", "No resources found.".dimmed());
            } else {
                for resource in &resources {
                    println!("{} — {}", resource.id, resource.name);
                }
            }
            Ok(())
        }
        SkillResourceCommands::Get {
            skill_id,
            resource_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resource = scoped
                .get_skill_resource(&org_id, skill_id, resource_id)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resource)?);
            } else {
                println!("{} — {}", resource.id, resource.name);
                println!("{}", resource.content);
            }
            Ok(())
        }
        SkillResourceCommands::Update {
            skill_id,
            resource_id,
            name,
            content,
            org,
            json,
        } => {
            if name.is_none() && content.is_none() {
                bail!("At least one of --name/--content is required");
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = SkillResourceUpdateRequest {
                name: name.clone(),
                content: content.clone(),
            };
            let resource = scoped
                .update_skill_resource(&org_id, skill_id, resource_id, &req)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resource)?);
            } else {
                println!("Updated resource {} — {}", resource.id, resource.name);
            }
            Ok(())
        }
        SkillResourceCommands::Delete {
            skill_id,
            resource_id,
            org,
            force,
            json,
        } => {
            if !*force && !confirm(&format!("Delete resource {resource_id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped
                .delete_skill_resource(&org_id, skill_id, resource_id)
                .await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"deleted": true, "id": resource_id})
                    )?
                );
            } else {
                println!("Deleted resource {resource_id}");
            }
            Ok(())
        }
    }
}

async fn handle_refinement(cmd: &SkillRefinementCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        SkillRefinementCommands::Accept {
            refinement_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let skill = scoped
                .accept_skill_refinement(&org_id, refinement_id)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&skill)?);
            } else {
                println!(
                    "Accepted refinement {refinement_id} — skill {} is now v{}",
                    skill.name, skill.version
                );
            }
            Ok(())
        }
        SkillRefinementCommands::Reject {
            refinement_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            scoped
                .reject_skill_refinement(&org_id, refinement_id)
                .await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"rejected": true, "id": refinement_id})
                    )?
                );
            } else {
                println!("Rejected refinement {refinement_id}");
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_json_object;

    #[test]
    fn parse_json_object_accepts_valid_object() {
        let map = parse_json_object(r#"{"a":1,"b":"two"}"#, "--examples-json").unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["b"], "two");
    }

    #[test]
    fn parse_json_object_rejects_non_object() {
        let err = parse_json_object("[1,2,3]", "--examples-json").unwrap_err();
        assert!(err.to_string().contains("--examples-json"));
    }

    #[test]
    fn parse_json_object_rejects_invalid_json() {
        let err = parse_json_object("not json", "--examples-json").unwrap_err();
        assert!(err.to_string().contains("--examples-json"));
    }
}
