use anyhow::Result;
use clap::Subcommand;

use crate::config::{Credentials, DEFAULT_API_URL};

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Set authentication token (Clerk JWT or API Key)
    SetToken {
        /// The token to save
        token: String,
        /// API base URL
        #[arg(long, default_value = DEFAULT_API_URL)]
        api_url: String,
    },
    /// Show current authentication status
    Status,
    /// Remove saved credentials
    Logout,
}

pub fn handle_auth(cmd: &AuthCommands) -> Result<()> {
    match cmd {
        AuthCommands::SetToken { token, api_url } => {
            let creds = Credentials::new(token.clone(), api_url.clone());
            creds.save()?;

            let masked = if token.len() > 10 {
                format!("{}...{}", &token[..6], &token[token.len() - 4..])
            } else {
                "***".to_string()
            };
            println!("Token saved: {}", masked);
            println!("API URL: {}", api_url);
            Ok(())
        }
        AuthCommands::Status => {
            match Credentials::load()? {
                Some(creds) => {
                    let masked = if creds.token().len() > 10 {
                        format!(
                            "{}...{}",
                            &creds.token()[..6],
                            &creds.token()[creds.token().len() - 4..]
                        )
                    } else {
                        "***".to_string()
                    };
                    println!("Authenticated");
                    println!("  Token: {}", masked);
                    println!("  API URL: {}", creds.api_url());
                }
                None => {
                    println!("Not authenticated. Run: addness auth set-token <token>");
                }
            }
            Ok(())
        }
        AuthCommands::Logout => {
            Credentials::delete()?;
            println!("Logged out.");
            Ok(())
        }
    }
}
