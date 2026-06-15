use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::{ApiClient, CreateOrganizationParams, OrganizationsResponse};
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
                            "Warning: No API key stored for this organization. Run `addness login` to authenticate."
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
