use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::{
    ApiClient, CreateOrganizationParams, ListAllOrganizationsParams, OrganizationsResponse,
};
use crate::cli::output::print_organizations_table;
use crate::config::{Credentials, Settings};

#[derive(Subcommand)]
pub enum OrgCommands {
    /// List organizations you belong to
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Switch current organization
    Switch {
        /// Organization ID
        id: String,
    },
    /// Show current organization
    Current {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a new organization
    Create {
        /// Organization name
        #[arg(long)]
        name: String,
        /// Organization type: PERSONAL or BUSINESS (default PERSONAL)
        #[arg(long, default_value = "PERSONAL")]
        r#type: String,
        /// Team scale (required for BUSINESS): SOLO, 2_5, 6_20, 21_50, 50_PLUS
        #[arg(long)]
        team_scale: Option<String>,
        /// Plan type: FREE, PACKAGE, or SUBSCRIPTION
        #[arg(long)]
        plan_type: Option<String>,
        /// Industry text
        #[arg(long)]
        industry: Option<String>,
        /// Phone number
        #[arg(long)]
        phone_number: Option<String>,
        /// Browser timezone for initial organization settings (default Asia/Tokyo on API)
        #[arg(long = "timezone")]
        browser_timezone: Option<String>,
        /// Logo URL
        #[arg(long)]
        logo_url: Option<String>,
        /// Switch current organization to the created organization
        #[arg(long)]
        switch: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update an organization's name
    Update {
        /// Organization ID
        id: String,
        /// New name
        #[arg(long)]
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete an organization
    Rm {
        /// Organization ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Update the organization's context text (free-form AI context)
    SetContext {
        /// Organization ID
        id: String,
        /// Inline context text
        #[arg(long, conflicts_with = "text_file")]
        text: Option<String>,
        /// Read context text from a file
        #[arg(long, conflicts_with = "text")]
        text_file: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a single organization's details
    Get {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List all organizations (subscription-scoped, paginated) matching an optional name
    ListAll {
        /// Filter by organization name (partial match)
        #[arg(long)]
        name: Option<String>,
        /// Maximum number of results
        #[arg(long)]
        limit: Option<u16>,
        /// Number of results to skip
        #[arg(long)]
        offset: Option<u64>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the organization's root goal owner
    RootOwner {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the root goal you can access in the organization
    AccessibleRoot {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the organization's AI agent member
    AiAgentMember {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the organization's payment access state
    AccessState {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show your member record within the organization
    CurrentMember {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Check whether you are an admin of the organization
    AdminCheck {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the organization's context text
    GetContext {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List revisions of the organization's context text
    ContextRevisions {
        /// Organization ID
        id: String,
        /// Maximum number of revisions to return
        #[arg(long)]
        limit: Option<u16>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Build a client whose `X-Organization-ID` header targets `id`.
///
/// The v2 organization endpoints are guarded by `OrganizationMemberGuard` and
/// several handlers additionally require the header org to equal the path `:id`,
/// so id-scoped subcommands must send the header for the organization named in
/// the argument rather than the shell's current organization.
fn client_for_org(client: &ApiClient, id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(id.to_string()));
    scoped
}

/// Print a JSON value as pretty-printed text (used by the read-only subcommands
/// whose response shapes vary per endpoint and are surfaced verbatim).
fn print_json_value(value: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub async fn handle_org(cmd: &OrgCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        OrgCommands::List { json } => {
            let resp: OrganizationsResponse = client.list_organizations().await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&resp.data.organizations)?
                );
            } else {
                let settings = Settings::load()?;
                print_organizations_table(
                    &resp.data.organizations,
                    settings.current_organization_id(),
                );
            }
            Ok(())
        }
        OrgCommands::Switch { id } => {
            // 所属確認: APIで組織一覧を取得し、指定IDが含まれるか検証
            let resp: OrganizationsResponse = client.list_organizations().await?;
            let found = resp.data.organizations.iter().find(|org| org.id == *id);
            match found {
                Some(org) => {
                    let mut settings = Settings::load()?;
                    settings.set_current_organization_id(id.clone())?;
                    println!("Switched to organization: {} ({})", org.name, id);

                    // Warn if no API key stored for this org
                    if let Some(creds) = Credentials::load()?
                        && !creds.has_token_for_org(id)
                    {
                        println!();
                        println!(
                            "Warning: No API key stored for this organization. Run `addness login` to authenticate this org, or `addness configure` if you have a key for it."
                        );
                    }
                }
                None => {
                    bail!(
                        "Organization '{id}' not found in your account.\n\
                         Use `addness org list` to see available organizations."
                    );
                }
            }
            Ok(())
        }
        OrgCommands::Current { json } => {
            let settings = Settings::load()?;
            match settings.current_organization_id() {
                Some(id) => {
                    if *json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "organization_id": id
                            }))?
                        );
                    } else {
                        println!("{id}");
                    }
                }
                None => bail!("No current organization set. Run: addness org switch <id>"),
            }
            Ok(())
        }
        OrgCommands::Create {
            name,
            r#type,
            team_scale,
            plan_type,
            industry,
            phone_number,
            browser_timezone,
            logo_url,
            switch,
            json,
        } => {
            let upper_type = r#type.to_uppercase();
            match upper_type.as_str() {
                "PERSONAL" | "BUSINESS" => {}
                _ => bail!("Invalid --type '{type}'. Use PERSONAL or BUSINESS."),
            }
            let normalized_team_scale = team_scale
                .as_ref()
                .map(|scale| scale.trim().to_uppercase())
                .filter(|scale| !scale.is_empty());
            if upper_type == "BUSINESS" && normalized_team_scale.is_none() {
                bail!(
                    "--team-scale is required for BUSINESS (one of SOLO, 2_5, 6_20, 21_50, 50_PLUS)"
                );
            }
            if let Some(scale) = &normalized_team_scale {
                match scale.as_str() {
                    "SOLO" | "2_5" | "6_20" | "21_50" | "50_PLUS" => {}
                    _ => bail!(
                        "Invalid --team-scale '{scale}'. Use SOLO, 2_5, 6_20, 21_50, or 50_PLUS."
                    ),
                }
            }
            let normalized_plan_type = plan_type
                .as_ref()
                .map(|plan| plan.trim().to_uppercase())
                .filter(|plan| !plan.is_empty());
            if let Some(plan) = &normalized_plan_type {
                match plan.as_str() {
                    "FREE" | "PACKAGE" | "SUBSCRIPTION" => {}
                    _ => bail!("Invalid --plan-type '{plan}'. Use FREE, PACKAGE, or SUBSCRIPTION."),
                }
            }
            let resp = client
                .create_organization(CreateOrganizationParams {
                    name: name.clone(),
                    organization_type: upper_type,
                    team_scale: normalized_team_scale,
                    plan_type: normalized_plan_type,
                    industry: non_empty(industry),
                    phone_number: non_empty(phone_number),
                    browser_timezone: non_empty(browser_timezone),
                    logo_url: non_empty(logo_url),
                })
                .await?;

            if *switch {
                let created_org_id = resp.data.id.clone();
                copy_current_token_for_created_org(&created_org_id)?;
                let mut settings = Settings::load()?;
                settings.set_current_organization_id(created_org_id.clone())?;
            }

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Organization created: {name} ({})", resp.data.id);
                if *switch {
                    println!("Switched to organization: {}", resp.data.id);
                } else {
                    println!("Switch with: addness org switch {}", resp.data.id);
                }
            }
            Ok(())
        }
        OrgCommands::Update { id, name, json } => {
            let resp = client.update_organization(id, name).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Organization {id} renamed to {name}");
            }
            Ok(())
        }
        OrgCommands::Rm { id, force } => {
            if !*force && !crate::cli::commands::confirm(&format!("Delete organization {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_organization(id).await?;
            println!("Organization {id} deleted");
            Ok(())
        }
        OrgCommands::SetContext {
            id,
            text,
            text_file,
            json,
        } => {
            let body = match (text, text_file) {
                (Some(s), None) => s.clone(),
                (None, Some(p)) => std::fs::read_to_string(p)?,
                (Some(_), Some(_)) => bail!("Specify only one of --text or --text-file"),
                (None, None) => bail!("Specify --text or --text-file"),
            };
            let resp = client.update_organization_context(id, &body).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!(
                    "Organization {id} context updated ({} chars)",
                    body.chars().count()
                );
            }
            Ok(())
        }
        OrgCommands::Get { id, json: _ } => {
            let data = client_for_org(client, id).get_organization(id).await?;
            print_json_value(&data)
        }
        OrgCommands::ListAll {
            name,
            limit,
            offset,
            json: _,
        } => {
            let data = client
                .list_all_organizations(ListAllOrganizationsParams {
                    name: name.as_deref(),
                    limit: *limit,
                    offset: *offset,
                })
                .await?;
            print_json_value(&data)
        }
        OrgCommands::RootOwner { id, json: _ } => {
            let data = client_for_org(client, id)
                .get_organization_root_owner(id)
                .await?;
            print_json_value(&data)
        }
        OrgCommands::AccessibleRoot { id, json: _ } => {
            let data = client_for_org(client, id)
                .get_organization_accessible_root(id)
                .await?;
            print_json_value(&data)
        }
        OrgCommands::AiAgentMember { id, json: _ } => {
            let data = client_for_org(client, id)
                .get_organization_ai_agent_member(id)
                .await?;
            print_json_value(&data)
        }
        OrgCommands::AccessState { id, json: _ } => {
            let data = client_for_org(client, id)
                .get_organization_access_state(id)
                .await?;
            print_json_value(&data)
        }
        OrgCommands::CurrentMember { id, json: _ } => {
            let data = client_for_org(client, id)
                .get_organization_current_member(id)
                .await?;
            print_json_value(&data)
        }
        OrgCommands::AdminCheck { id, json: _ } => {
            let data = client_for_org(client, id)
                .check_organization_admin(id)
                .await?;
            print_json_value(&data)
        }
        OrgCommands::GetContext { id, json: _ } => {
            let data = client_for_org(client, id)
                .get_organization_context(id)
                .await?;
            print_json_value(&data)
        }
        OrgCommands::ContextRevisions { id, limit, json: _ } => {
            let data = client_for_org(client, id)
                .list_organization_context_revisions(id, *limit)
                .await?;
            print_json_value(&data)
        }
    }
}

fn non_empty(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn copy_current_token_for_created_org(created_org_id: &str) -> Result<()> {
    let Some(mut creds) = Credentials::load()? else {
        return Ok(());
    };
    if creds.has_token_for_org(created_org_id) {
        return Ok(());
    }

    let settings = Settings::load()?;
    let token = settings
        .current_organization_id()
        .and_then(|id| creds.token_for_org(id))
        .or_else(|| creds.any_token())
        .map(|token| token.to_string());

    if let Some(token) = token {
        creds.set_token(created_org_id.to_string(), token);
        creds.save()?;
    }
    Ok(())
}

pub fn resolve_org_id(flag: Option<&str>) -> Result<String> {
    if let Some(id) = flag {
        return Ok(id.to_string());
    }
    let settings = Settings::load()?;
    match settings.current_organization_id() {
        Some(id) => Ok(id.to_string()),
        None => bail!(
            "No organization specified.\n\
             Use --org <id> or set a current with: addness org switch <id>"
        ),
    }
}
