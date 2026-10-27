use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use serde_json::json;

use crate::api::ApiClient;
use crate::cli::commands::org::resolve_org_id;

#[derive(Clone, Copy, ValueEnum)]
pub enum Platform {
    Pc,
    Mobile,
}
impl Platform {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pc => "pc",
            Self::Mobile => "mobile",
        }
    }
}

#[derive(Args)]
pub struct Common {
    /// Organization ID (defaults to current organization)
    #[arg(long)]
    org: Option<String>,
    /// Challenge audience platform
    #[arg(long, value_enum, default_value = "pc")]
    platform: Platform,
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
pub enum ChallengeCommands {
    /// List your challenges, steps, state, and any open run
    List {
        #[command(flatten)]
        common: Common,
        #[arg(long, default_value_t = 3)]
        defs: u32,
    },
    /// Show your challenge summary
    Summary {
        #[command(flatten)]
        common: Common,
        #[arg(long, default_value_t = 3)]
        defs: u32,
    },
    /// Show feature locks
    Locks {
        #[command(flatten)]
        common: Common,
    },
    /// Show count roads (may award reached milestones)
    CountRoads {
        #[command(flatten)]
        common: Common,
    },
    /// Start a run; replaces an existing open run
    Start {
        key: String,
        #[command(flatten)]
        common: Common,
    },
    /// Acquire a challenge ticket
    Ticket {
        key: String,
        #[arg(long, value_parser = ["lock_window", "later", "convenience", "route", "ai_suggested"])]
        source: String,
        #[command(flatten)]
        common: Common,
    },
    /// Check the current run's steps and their server-verified progress
    Verify {
        key: String,
        run: String,
        #[command(flatten)]
        common: Common,
    },
    /// Watch the selected goal and AI activity for a run
    Watch {
        key: String,
        run: String,
        #[command(flatten)]
        common: Common,
    },
    /// Complete a run after its steps are satisfied; manually checked steps must be specified explicitly
    Complete {
        key: String,
        run: String,
        /// Key of a manual step actually completed; repeat for each
        #[arg(long = "manual-step")]
        manual_steps: Vec<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Set the goal and agent for a supported run
    Target {
        key: String,
        run: String,
        #[arg(long)]
        goal: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        note: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Skip a challenge, abandoning its open run
    Skip {
        key: String,
        #[command(flatten)]
        common: Common,
    },
    /// Undo a skip; some challenges return a newly created run
    Unskip {
        key: String,
        #[command(flatten)]
        common: Common,
    },
}

impl ChallengeCommands {
    fn common(&self) -> &Common {
        match self {
            Self::List { common, .. }
            | Self::Summary { common, .. }
            | Self::Locks { common }
            | Self::CountRoads { common }
            | Self::Start { common, .. }
            | Self::Ticket { common, .. }
            | Self::Verify { common, .. }
            | Self::Watch { common, .. }
            | Self::Complete { common, .. }
            | Self::Target { common, .. }
            | Self::Skip { common, .. }
            | Self::Unskip { common, .. } => common,
        }
    }
    pub fn org(&self) -> Option<&str> {
        self.common().org.as_deref()
    }
    pub fn outputs_json(&self) -> bool {
        self.common().json
    }
}

pub async fn handle_challenge(command: &ChallengeCommands, client: &ApiClient) -> Result<()> {
    let common = command.common();
    let org = resolve_org_id(common.org.as_deref())?;
    let scoped = client.clone().with_org_id(Some(org.clone()));
    let platform = common.platform.as_str();
    let result = match command {
        ChallengeCommands::List { defs, .. } => {
            scoped.challenge_list(&org, platform, *defs, false).await?
        }
        ChallengeCommands::Summary { defs, .. } => {
            scoped.challenge_list(&org, platform, *defs, true).await?
        }
        ChallengeCommands::Locks { .. } => {
            scoped
                .challenge_read(&org, platform, "feature-locks")
                .await?
        }
        ChallengeCommands::CountRoads { .. } => {
            scoped.challenge_read(&org, platform, "count-roads").await?
        }
        ChallengeCommands::Start { key, .. } => scoped.challenge_start(&org, key, platform).await?,
        ChallengeCommands::Ticket { key, source, .. } => {
            scoped.challenge_ticket(&org, key, platform, source).await?
        }
        ChallengeCommands::Verify { key, run, .. } => {
            scoped
                .challenge_verify(&org, key, run, platform, false)
                .await?
        }
        ChallengeCommands::Watch { key, run, .. } => {
            scoped
                .challenge_verify(&org, key, run, platform, true)
                .await?
        }
        ChallengeCommands::Complete {
            key,
            run,
            manual_steps,
            ..
        } => {
            scoped
                .challenge_complete(&org, key, run, platform, manual_steps)
                .await?
        }
        ChallengeCommands::Target {
            key,
            run,
            goal,
            agent,
            note,
            ..
        } => {
            let mut target = json!({"targetObjectiveId": goal, "agent": agent});
            if let Some(note) = note {
                target["note"] = json!(note);
            }
            scoped
                .challenge_target(&org, key, run, platform, &target)
                .await?
        }
        ChallengeCommands::Skip { key, .. } => {
            scoped.challenge_skip(&org, key, platform, false).await?
        }
        ChallengeCommands::Unskip { key, .. } => {
            scoped.challenge_skip(&org, key, platform, true).await?
        }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
