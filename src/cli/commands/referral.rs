use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use colored::Colorize;

use crate::api::ApiClient;

/// Referral share channel (`domain/models/referral`).
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ReferralChannel {
    Copy,
    Line,
    Sns,
}

impl ReferralChannel {
    fn as_str(self) -> &'static str {
        match self {
            ReferralChannel::Copy => "copy",
            ReferralChannel::Line => "line",
            ReferralChannel::Sns => "sns",
        }
    }
}

#[derive(Subcommand)]
pub enum ReferralCommands {
    /// Create a referral link for a share channel
    LinkCreate {
        /// Share channel
        #[arg(long, value_enum)]
        channel: ReferralChannel,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List your referral links, conversions, and reward summary
    List {
        /// Max number of items to return (default 20)
        #[arg(long)]
        limit: Option<u16>,
        /// Pagination offset (default 0)
        #[arg(long)]
        offset: Option<u16>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Record a referral conversion for a signup (the backend attributes it
    /// to the calling user; --referral-code is the only meaningful input)
    Convert {
        /// Referral code
        #[arg(long)]
        referral_code: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_referral(cmd: &ReferralCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ReferralCommands::LinkCreate { channel, json } => {
            let link = client.create_referral_link(channel.as_str()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&link)?);
            } else {
                println!("Referral link created: {}", link.share_url);
                println!("  code: {}", link.referral_code);
            }
            Ok(())
        }
        ReferralCommands::List {
            limit,
            offset,
            json,
        } => {
            let list = client.list_my_referrals(*limit, *offset).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else {
                println!(
                    "Links: {}  Conversions: {}  Rewards: {}",
                    list.summary.total_links,
                    list.summary.total_conversions,
                    list.summary.total_rewards
                );
                if list.items.is_empty() {
                    println!("{}", "No conversions found.".dimmed());
                } else {
                    println!();
                    println!(
                        "{:<38} {:<10} {:<12} {}",
                        "ID".bold(),
                        "STATUS".bold(),
                        "CODE".bold(),
                        "CREATED".bold()
                    );
                    println!("{}", "─".repeat(90));
                    for item in &list.items {
                        println!(
                            "{:<38} {:<10} {:<12} {}",
                            item.id.dimmed(),
                            item.status,
                            item.referral_code,
                            item.created_at.dimmed()
                        );
                    }
                }
            }
            Ok(())
        }
        ReferralCommands::Convert {
            referral_code,
            json,
        } => {
            let result = client.convert_referral_signup(referral_code).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                match &result.reason {
                    Some(reason) => println!("Conversion status: {} ({reason})", result.status),
                    None => println!("Conversion status: {}", result.status),
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReferralChannel;

    #[test]
    fn referral_channel_as_str_maps_to_backend_values() {
        assert_eq!(ReferralChannel::Copy.as_str(), "copy");
        assert_eq!(ReferralChannel::Line.as_str(), "line");
        assert_eq!(ReferralChannel::Sns.as_str(), "sns");
    }
}
