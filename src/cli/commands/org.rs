use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::{ApiClient, OrganizationsResponse};
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
            // 所属確認: APIで組織一覧を取得し、指定IDが含まれるか検証
            let resp: OrganizationsResponse = client.list_organizations().await?;
            let found = resp.data.iter().find(|org| org.id == *id);
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
