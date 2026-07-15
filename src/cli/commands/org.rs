use anyhow::{Result, bail};
use clap::{ArgAction, Subcommand};

use crate::api::{
    ApiClient, CreateOrganizationParams, ListAllOrganizationsParams, MyAdSettingRequest,
    OrganizationsResponse,
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
    /// Update the organization's default timezone (IANA name; owner + admin only)
    SetTimezone {
        /// Organization ID
        id: String,
        /// IANA timezone name, e.g. Asia/Tokyo
        #[arg(long)]
        timezone: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Upload the organization's logo (owner + admin only)
    SetLogo {
        /// Organization ID
        id: String,
        /// Path to the image file to upload
        #[arg(long)]
        file: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Register a push notification token for the organization
    PushTokenRegister {
        /// Organization ID
        id: String,
        /// Push token to register
        #[arg(long)]
        token: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Read or change the onboarding billing state
    OnboardingBilling {
        #[command(subcommand)]
        command: OnboardingBillingCommands,
    },
    /// Read or toggle the organization's AI schedule master switch
    AiScheduleSettings {
        #[command(subcommand)]
        command: AiScheduleSettingsCommands,
    },
    /// Read or change in-app ad settings (organization-wide or your own)
    AdSettings {
        #[command(subcommand)]
        command: AdSettingsCommands,
    },
    /// Manage the organization's paid subscription
    Subscription {
        #[command(subcommand)]
        command: SubscriptionCommands,
    },
}

#[derive(Subcommand)]
pub enum OnboardingBillingCommands {
    /// Show the onboarding billing state
    State {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark onboarding billing as required
    Require {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Complete onboarding billing on the free plan
    Free {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AiScheduleSettingsCommands {
    /// Show the AI schedule master switch state
    Get {
        /// Organization ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Enable or disable the AI schedule master switch
    Set {
        /// Organization ID
        id: String,
        /// Whether the master switch is enabled (true/false)
        #[arg(long, action = ArgAction::Set)]
        enabled: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AdSettingsCommands {
    /// Show the organization-wide ad settings (or your own with --me)
    Get {
        /// Organization ID
        id: String,
        /// Read your own ad settings instead of the organization-wide switch
        #[arg(long)]
        me: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Change the organization-wide ad kill switch (owner + admin only)
    Set {
        /// Organization ID
        id: String,
        /// Whether ads are enabled organization-wide (true/false)
        #[arg(long, action = ArgAction::Set)]
        enabled: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Change your own ad settings (opt-out and/or a temporary hide window)
    SetMe {
        /// Organization ID
        id: String,
        /// Permanently opt out of ads (true/false)
        #[arg(long, action = ArgAction::Set)]
        enabled: Option<bool>,
        /// Hide ads until this RFC3339 timestamp (within 48 hours)
        #[arg(long)]
        hidden_until: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum SubscriptionCommands {
    /// Register a paid subscription from an Univapay subscription ID
    Register {
        /// Univapay subscription ID
        #[arg(long)]
        univapay_subscription_id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Cancel a subscription by its ID
    Cancel {
        /// Organization subscription ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the current subscription for the active organization
    Current {
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
        OrgCommands::SetTimezone {
            id,
            timezone,
            json: _,
        } => {
            let data = client_for_org(client, id)
                .update_organization_default_timezone(id, timezone)
                .await?;
            print_json_value(&data)
        }
        OrgCommands::SetLogo { id, file, json: _ } => {
            let bytes = std::fs::read(file)
                .map_err(|e| anyhow::anyhow!("Failed to read logo file '{file}': {e}"))?;
            let content_type = content_type_for_path(file);
            let data = client_for_org(client, id)
                .upload_organization_logo(id, bytes, content_type)
                .await?;
            print_json_value(&data)
        }
        OrgCommands::PushTokenRegister { id, token, json: _ } => {
            let data = client_for_org(client, id)
                .register_organization_push_token(id, token)
                .await?;
            print_json_value(&data)
        }
        OrgCommands::OnboardingBilling { command } => {
            handle_onboarding_billing(command, client).await
        }
        OrgCommands::AiScheduleSettings { command } => {
            handle_ai_schedule_settings(command, client).await
        }
        OrgCommands::AdSettings { command } => handle_ad_settings(command, client).await,
        OrgCommands::Subscription { command } => handle_subscription(command, client).await,
    }
}

async fn handle_onboarding_billing(
    cmd: &OnboardingBillingCommands,
    client: &ApiClient,
) -> Result<()> {
    match cmd {
        OnboardingBillingCommands::State { id, json: _ } => {
            let data = client_for_org(client, id)
                .get_organization_onboarding_billing_state(id)
                .await?;
            print_json_value(&data)
        }
        OnboardingBillingCommands::Require { id, json: _ } => {
            let data = client_for_org(client, id)
                .require_organization_onboarding_billing(id)
                .await?;
            print_json_value(&data)
        }
        OnboardingBillingCommands::Free { id, json: _ } => {
            let data = client_for_org(client, id)
                .complete_organization_onboarding_billing_free(id)
                .await?;
            print_json_value(&data)
        }
    }
}

async fn handle_ai_schedule_settings(
    cmd: &AiScheduleSettingsCommands,
    client: &ApiClient,
) -> Result<()> {
    match cmd {
        AiScheduleSettingsCommands::Get { id, json: _ } => {
            let data = client_for_org(client, id)
                .get_organization_ai_schedule_settings(id)
                .await?;
            print_json_value(&data)
        }
        AiScheduleSettingsCommands::Set {
            id,
            enabled,
            json: _,
        } => {
            let data = client_for_org(client, id)
                .set_organization_ai_schedule_settings(id, *enabled)
                .await?;
            print_json_value(&data)
        }
    }
}

async fn handle_ad_settings(cmd: &AdSettingsCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        AdSettingsCommands::Get { id, me, json: _ } => {
            let scoped = client_for_org(client, id);
            let data = if *me {
                scoped.get_my_organization_ad_settings(id).await?
            } else {
                scoped.get_organization_ad_settings(id).await?
            };
            print_json_value(&data)
        }
        AdSettingsCommands::Set {
            id,
            enabled,
            json: _,
        } => {
            let data = client_for_org(client, id)
                .set_organization_ad_settings(id, *enabled)
                .await?;
            print_json_value(&data)
        }
        AdSettingsCommands::SetMe {
            id,
            enabled,
            hidden_until,
            json: _,
        } => {
            if enabled.is_none() && hidden_until.is_none() {
                bail!("Specify at least one of --enabled or --hidden-until");
            }
            let body = MyAdSettingRequest {
                enabled: *enabled,
                hidden_until: hidden_until.clone(),
            };
            let data = client_for_org(client, id)
                .set_my_organization_ad_settings(id, &body)
                .await?;
            print_json_value(&data)
        }
    }
}

async fn handle_subscription(cmd: &SubscriptionCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        SubscriptionCommands::Register {
            univapay_subscription_id,
            json: _,
        } => {
            let data = client
                .register_organization_subscription(univapay_subscription_id)
                .await?;
            print_json_value(&data)
        }
        SubscriptionCommands::Cancel { id, force, json: _ } => {
            if !*force && !crate::cli::commands::confirm(&format!("Cancel subscription {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let data = client.cancel_organization_subscription(id).await?;
            print_json_value(&data)
        }
        SubscriptionCommands::Current { json: _ } => {
            let data = client.get_current_organization_subscription().await?;
            print_json_value(&data)
        }
    }
}

/// Best-effort MIME type for an image upload based on the file extension.
/// Falls back to `application/octet-stream`; the backend also sniffs the
/// actual bytes, so this only needs to be a reasonable hint. Shared with
/// `chat::` room icon uploads (same image-file convention).
pub(super) fn content_type_for_path(path: &str) -> &'static str {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
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

#[cfg(test)]
mod tests {
    use super::content_type_for_path;

    #[test]
    fn content_type_for_path_maps_known_image_extensions() {
        assert_eq!(content_type_for_path("logo.png"), "image/png");
        assert_eq!(content_type_for_path("LOGO.PNG"), "image/png");
        assert_eq!(content_type_for_path("a/b/logo.jpg"), "image/jpeg");
        assert_eq!(content_type_for_path("logo.jpeg"), "image/jpeg");
        assert_eq!(content_type_for_path("logo.gif"), "image/gif");
        assert_eq!(content_type_for_path("logo.webp"), "image/webp");
        assert_eq!(content_type_for_path("logo.svg"), "image/svg+xml");
    }

    #[test]
    fn content_type_for_path_falls_back_for_unknown_or_missing_extension() {
        assert_eq!(
            content_type_for_path("logo.bin"),
            "application/octet-stream"
        );
        assert_eq!(content_type_for_path("logo"), "application/octet-stream");
    }
}
