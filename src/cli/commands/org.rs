use anyhow::{bail, Result};
use clap::Subcommand;

use crate::api::{ApiClient, OrganizationsResponse};
use crate::cli::output::print_organizations_table;
use crate::config::{load_settings, save_settings};

#[derive(Subcommand)]
pub enum OrgCommands {
    /// List organizations you belong to
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Switch default organization
    Switch {
        /// Organization ID
        id: String,
    },
    /// Show current default organization
    Current,
}

pub async fn handle_org(cmd: &OrgCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        OrgCommands::List { json } => {
            let resp: OrganizationsResponse =
                client.get("/api/v1/team/organizations/my_organizations").await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                let settings = load_settings()?;
                print_organizations_table(
                    &resp.data,
                    settings.default_organization_id.as_deref(),
                );
            }
            Ok(())
        }
        OrgCommands::Switch { id } => {
            let mut settings = load_settings()?;
            settings.default_organization_id = Some(id.clone());
            save_settings(&settings)?;
            println!("Switched to organization: {}", id);
            Ok(())
        }
        OrgCommands::Current => {
            let settings = load_settings()?;
            match settings.default_organization_id {
                Some(id) => println!("{}", id),
                None => bail!("No default organization set. Run: addness org switch <id>"),
            }
            Ok(())
        }
    }
}

pub fn resolve_org_id(flag: Option<&str>) -> Result<String> {
    if let Some(id) = flag {
        return Ok(id.to_string());
    }
    let settings = load_settings()?;
    match settings.default_organization_id {
        Some(id) => Ok(id),
        None => bail!(
            "No organization specified.\n\
             Use --org <id> or set a default with: addness org switch <id>"
        ),
    }
}
