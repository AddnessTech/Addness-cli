use anyhow::Result;
use clap::Subcommand;

use crate::api::{ApiClient, DesktopAuthCompleteRequest, DesktopAuthRedeemRequest};

#[derive(Subcommand)]
pub enum DesktopAuthCommands {
    /// Redeem a browser desktop-auth start token into an auth intent
    Redeem {
        /// Start token from /desktop/browser-auth?start_token=...
        #[arg(long)]
        start_token: String,
        /// SHA-256 base64url hash of the browser nonce
        #[arg(long)]
        browser_nonce_hash: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Complete a desktop-auth intent after browser authentication
    Complete {
        /// Desktop auth intent ID
        intent_id: String,
        /// Browser nonce stored during redeem
        #[arg(long)]
        browser_nonce: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_desktop_auth(cmd: &DesktopAuthCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        DesktopAuthCommands::Redeem {
            start_token,
            browser_nonce_hash,
            json,
        } => {
            let req = DesktopAuthRedeemRequest {
                start_token: start_token.clone(),
                browser_nonce_hash: browser_nonce_hash.clone(),
            };
            let resp = client.redeem_desktop_auth_start_session(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Intent: {}", resp.intent_id);
                println!("Expires: {}", resp.expires_at);
                if let Some(auth_path) = resp.auth_path {
                    println!("Auth path: {auth_path}");
                }
                if let Some(referral_code) = resp.referral_code {
                    println!("Referral code: {referral_code}");
                }
            }
            Ok(())
        }
        DesktopAuthCommands::Complete {
            intent_id,
            browser_nonce,
            json,
        } => {
            let req = DesktopAuthCompleteRequest {
                browser_nonce: browser_nonce.clone(),
            };
            let resp = client.complete_desktop_auth_intent(intent_id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Handoff: {}", resp.handoff_id);
                println!("State: {}", resp.state);
                println!("Port: {}", resp.port);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopAuthCommands;

    fn command_outputs_json(command: &DesktopAuthCommands) -> bool {
        match command {
            DesktopAuthCommands::Redeem { json, .. }
            | DesktopAuthCommands::Complete { json, .. } => *json,
        }
    }

    #[test]
    fn json_flag_controls_update_check_skip() {
        assert!(command_outputs_json(&DesktopAuthCommands::Redeem {
            start_token: "start-token".to_string(),
            browser_nonce_hash: "hash".to_string(),
            json: true,
        }));
        assert!(!command_outputs_json(&DesktopAuthCommands::Complete {
            intent_id: "intent-1".to_string(),
            browser_nonce: "nonce".to_string(),
            json: false,
        }));
    }
}
