use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::{ApiClient, OrganizationsResponse};
use crate::cli::output::print_organizations_table;
use crate::config::Settings;

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
}

pub async fn handle_org(cmd: &OrgCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        OrgCommands::List { json } => {
            let resp: OrganizationsResponse = client.list_organizations().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                let settings = Settings::load()?;
                print_organizations_table(&resp.data, settings.current_organization_id());
            }
            Ok(())
        }
        OrgCommands::Switch { id } => {
            let mut settings = Settings::load()?;
            settings.set_current_organization_id(id.clone())?;
            println!("Switched to organization: {}", id);
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
    }
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
