use crate::api::ApiClient;
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

#[derive(Subcommand)]
pub enum PersonalCommands {
    /// Idempotently ensure your personal organization exists (Chat/Perfect Days billing target)
    EnsureOrganization {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show today's todos aggregated across every organization you belong to
    /// (org-independent; distinct from `today list`, which is scoped to one organization)
    TodayList {
        /// Date in YYYY-MM-DD (defaults to each organization's activity-day default)
        #[arg(long)]
        date: Option<String>,
        /// Exclude completed items (accepted for backward compatibility; currently a no-op backend-side)
        #[arg(long)]
        exclude_completed: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show your cross-organization planned/done activity counts per day (history heatmap)
    DailyActivity {
        /// Range start in YYYY-MM-DD (inclusive)
        #[arg(long)]
        start: String,
        /// Range end in YYYY-MM-DD (inclusive, max 400 days after --start)
        #[arg(long)]
        end: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_personal(cmd: &PersonalCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        PersonalCommands::EnsureOrganization { json } => {
            let resp = client.ensure_personal_organization().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Personal organization: {}", resp.organization_id);
                if let Some(balance) = &resp.balance {
                    println!(
                        "  Balance: {} / {} yen ({:.1}% remaining)",
                        balance.balance_yen, balance.cap_yen, balance.remaining_percentage
                    );
                }
            }
            Ok(())
        }
        PersonalCommands::TodayList {
            date,
            exclude_completed,
            json,
        } => {
            let items = client
                .get_personal_today_list(date.as_deref(), exclude_completed.then_some(true))
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else if items.is_empty() {
                println!("{}", "No today's todos across your organizations.".dimmed());
            } else {
                for item in &items {
                    let title = item.today_todo.title.as_deref().unwrap_or("");
                    println!(
                        "[{}] {} {}",
                        item.organization_name,
                        title,
                        item.today_todo
                            .objective_id
                            .as_deref()
                            .unwrap_or("")
                            .dimmed()
                    );
                }
            }
            Ok(())
        }
        PersonalCommands::DailyActivity { start, end, json } => {
            let counts = client.get_personal_daily_activity(start, end).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&counts)?);
            } else if counts.is_empty() {
                println!("{}", "No activity in this range.".dimmed());
            } else {
                for count in &counts {
                    println!(
                        "{} — planned: {}, done: {}",
                        count.date, count.planned, count.done
                    );
                }
            }
            Ok(())
        }
    }
}
