use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::ApiClient;

#[derive(Subcommand)]
pub enum AssignmentCommands {
    /// Assign an organization member to a goal
    Add {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Organization member ID (UUID) to assign
        #[arg(long)]
        member: String,
        /// Role: OWNER, EDITOR, or MEMBER (default MEMBER)
        #[arg(long)]
        role: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update an assignment's role
    Update {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Assignment ID
        id: String,
        /// New role: OWNER, EDITOR, or MEMBER
        #[arg(long)]
        role: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove an assignment
    Rm {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Assignment ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Transfer goal ownership to another member
    Transfer {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// New owner organization member ID (UUID)
        #[arg(long)]
        to: String,
        /// Demote current owner to EDITOR (default keeps prior role)
        #[arg(long)]
        actor_as_editor: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

fn validate_role(role: &str) -> Result<String> {
    let upper = role.to_uppercase();
    match upper.as_str() {
        "OWNER" | "EDITOR" | "MEMBER" => Ok(upper),
        _ => bail!("Invalid role '{role}'. Use OWNER, EDITOR, or MEMBER."),
    }
}

pub async fn handle_assignment(cmd: &AssignmentCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        AssignmentCommands::Add {
            goal,
            member,
            role,
            json,
        } => {
            let validated_role = match role {
                Some(r) => Some(validate_role(r)?),
                None => None,
            };
            let resp = client
                .create_assignment(goal, member, validated_role.clone())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                let role_label = validated_role.as_deref().unwrap_or("MEMBER (default)");
                println!("Assigned member {member} to goal {goal} as {role_label}");
            }
            Ok(())
        }
        AssignmentCommands::Update {
            goal,
            id,
            role,
            json,
        } => {
            let validated_role = validate_role(role)?;
            let resp = client
                .update_assignment(goal, id, Some(validated_role.clone()))
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Assignment {id} role updated to {validated_role}");
            }
            Ok(())
        }
        AssignmentCommands::Rm { goal, id, force } => {
            if !*force
                && !crate::cli::commands::confirm(&format!(
                    "Remove assignment {id} from goal {goal}?"
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_assignment(goal, id).await?;
            println!("Assignment {id} removed");
            Ok(())
        }
        AssignmentCommands::Transfer {
            goal,
            to,
            actor_as_editor,
            json,
        } => {
            let resp = client
                .transfer_ownership(goal, to, *actor_as_editor)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Ownership of goal {goal} transferred to member {to}");
            }
            Ok(())
        }
    }
}
