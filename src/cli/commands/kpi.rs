use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::{ApiClient, CreateKpiRequest, UpdateKpiRequest};

#[derive(Subcommand)]
pub enum KpiCommands {
    /// Add a KPI to a goal
    Add {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// KPI title
        #[arg(long)]
        title: String,
        /// Unit (e.g. "件", "%", "円")
        #[arg(long)]
        unit: String,
        /// Target value (integer, must be > 0)
        #[arg(long)]
        target: i32,
        /// Current actual value (optional)
        #[arg(long)]
        actual: Option<i32>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a KPI
    Update {
        /// KPI ID
        id: String,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New unit
        #[arg(long)]
        unit: Option<String>,
        /// New target value
        #[arg(long)]
        target: Option<i32>,
        /// New actual value
        #[arg(long)]
        actual: Option<i32>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a KPI
    Rm {
        /// KPI ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

pub async fn handle_kpi(cmd: &KpiCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        KpiCommands::Add {
            goal,
            title,
            unit,
            target,
            actual,
            json,
        } => {
            if *target <= 0 {
                bail!(
                    "--target must be greater than 0 (backend rejects 0 due to binding:required)"
                );
            }
            let req = CreateKpiRequest {
                title: title.clone(),
                unit: unit.clone(),
                target_value: *target,
                actual_value: *actual,
            };
            let resp = client.create_kpi(goal, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                let kpi_id = resp.data.id.as_deref().unwrap_or("(unknown)");
                println!("KPI created: {title} ({kpi_id}) target={target}{unit}");
            }
            Ok(())
        }
        KpiCommands::Update {
            id,
            title,
            unit,
            target,
            actual,
            json,
        } => {
            if title.is_none() && unit.is_none() && target.is_none() && actual.is_none() {
                bail!("Nothing to update. Specify --title, --unit, --target, or --actual.");
            }
            let req = UpdateKpiRequest {
                title: title.clone(),
                unit: unit.clone(),
                target_value: *target,
                actual_value: *actual,
            };
            let resp = client.update_kpi(id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("KPI {id} updated");
            }
            Ok(())
        }
        KpiCommands::Rm { id, force } => {
            if !*force && !crate::cli::commands::confirm(&format!("Delete KPI {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_kpi(id).await?;
            println!("KPI {id} deleted");
            Ok(())
        }
    }
}
