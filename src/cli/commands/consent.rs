use anyhow::Result;
use clap::{Subcommand, ValueEnum};

use crate::api::ApiClient;

/// Consent types recognized by the backend
/// (`domain/models/userconsent/consent.go` `Type` constants). Currently only
/// the two "telecommunications secrecy" disclosures exist — there is no
/// marketing/terms-of-service consent type on this endpoint.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ConsentType {
    /// `chat_communication_secrecy_dm` — consent for admins to view your DMs
    ChatSecrecyDm,
    /// `chat_communication_secrecy_group` — consent for admins to view your group chats
    ChatSecrecyGroup,
}

impl ConsentType {
    fn as_str(self) -> &'static str {
        match self {
            ConsentType::ChatSecrecyDm => "chat_communication_secrecy_dm",
            ConsentType::ChatSecrecyGroup => "chat_communication_secrecy_group",
        }
    }
}

#[derive(Subcommand)]
pub enum ConsentCommands {
    /// Show whether you have agreed to a consent type (e.g. admins viewing
    /// your DM/group chat messages). There is no `set`/`record` subcommand:
    /// the backend requires a Clerk browser session to record consent and
    /// rejects API-key auth outright, so the CLI cannot record it on your
    /// behalf — record consent from the web app instead.
    Get {
        /// Consent type to check
        #[arg(value_enum)]
        consent_type: ConsentType,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_consent(cmd: &ConsentCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ConsentCommands::Get { consent_type, json } => {
            let status = client.get_consent(consent_type.as_str()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else if status.agreed {
                println!(
                    "{}: agreed (version {}, at {})",
                    status.consent_type,
                    status.version,
                    status.agreed_at.as_deref().unwrap_or("unknown")
                );
            } else {
                println!("{}: not agreed", status.consent_type);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ConsentType;

    #[test]
    fn consent_type_as_str_maps_to_backend_values() {
        assert_eq!(
            ConsentType::ChatSecrecyDm.as_str(),
            "chat_communication_secrecy_dm"
        );
        assert_eq!(
            ConsentType::ChatSecrecyGroup.as_str(),
            "chat_communication_secrecy_group"
        );
    }
}
