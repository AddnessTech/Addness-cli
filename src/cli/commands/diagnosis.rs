use std::collections::HashMap;

use anyhow::{Result, bail};
use clap::{Subcommand, ValueEnum};
use colored::Colorize;

use crate::api::{ApiClient, DiagnosisMemberProfile};
use crate::cli::commands::org::resolve_org_id;

/// Diagnosis kind (`domain/models/diagnosis`). `GoalStyle`/`Values` require a
/// `type_code`; `CoreValues`/`MasterPlan` are free-form markdown results.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DiagnosisKind {
    GoalStyle,
    Values,
    CoreValues,
    MasterPlan,
}

impl DiagnosisKind {
    fn as_str(self) -> &'static str {
        match self {
            DiagnosisKind::GoalStyle => "goal_style",
            DiagnosisKind::Values => "values",
            DiagnosisKind::CoreValues => "core_values",
            DiagnosisKind::MasterPlan => "master_plan",
        }
    }

    /// Only these kinds accept anonymous-stats aggregation
    /// (`RequiresTypeCode` in the backend).
    fn supports_stats(self) -> bool {
        matches!(self, DiagnosisKind::GoalStyle | DiagnosisKind::Values)
    }
}

/// Build a client whose `X-Organization-ID` header targets `org_id`.
/// Mirrors `member::client_for_org`.
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

#[derive(Subcommand)]
pub enum DiagnosisCommands {
    /// Save (or append) a diagnosis result for yourself
    Save {
        /// Diagnosis kind
        #[arg(long, value_enum)]
        kind: DiagnosisKind,
        /// Schema version tag for the result payload (max 20 chars)
        #[arg(long)]
        schema_version: String,
        /// Result payload as a JSON object (inline string)
        #[arg(long, conflicts_with = "result_file")]
        result: Option<String>,
        /// Read the result JSON object from a file. Use '-' to read stdin.
        #[arg(long, conflicts_with = "result")]
        result_file: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List your latest diagnosis result per kind
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get your full diagnosis result for one kind
    Get {
        /// Diagnosis kind
        #[arg(long, value_enum)]
        kind: DiagnosisKind,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show anonymous aggregate stats for a diagnosis kind (no auth required)
    Stats {
        /// Diagnosis kind (only goal-style/values support aggregate stats)
        #[arg(long, value_enum)]
        kind: DiagnosisKind,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage which diagnosis kinds are visible to other org members
    Visibility {
        #[command(subcommand)]
        command: VisibilityCommands,
    },
    /// List diagnosis profiles for multiple org members
    Profiles {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Member ID (repeatable)
        #[arg(long = "member", required = true)]
        member_ids: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get one org member's diagnosis profile
    Profile {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Member ID
        member_id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum VisibilityCommands {
    /// Show your current diagnosis visibility settings
    Get {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Set diagnosis visibility for one or more kinds
    Set {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// `<kind>=<true|false>` pair, repeatable (e.g. --visibility goal_style=true)
        #[arg(long = "visibility", required = true)]
        visibilities: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

fn parse_result_json(inline: Option<&String>, file: Option<&String>) -> Result<serde_json::Value> {
    let raw = match (inline, file) {
        (Some(s), None) => s.clone(),
        (None, Some(path)) if path == "-" => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
        (None, Some(path)) => std::fs::read_to_string(path)?,
        (Some(_), Some(_)) => bail!("Specify only one of --result or --result-file"),
        (None, None) => bail!("Specify --result or --result-file with a JSON object payload"),
    };
    let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("--result must be a valid JSON object: {e}"))?;
    if !value.is_object() {
        bail!("--result must be a JSON object (e.g. {{\"typeCode\":\"ENFJ\"}})");
    }
    Ok(value)
}

/// Parse `<kind>=<true|false>` visibility pairs into the wire map.
fn parse_visibility_pairs(pairs: &[String]) -> Result<HashMap<String, bool>> {
    let mut map = HashMap::new();
    for pair in pairs {
        let (kind, value) = pair.split_once('=').ok_or_else(|| {
            anyhow::anyhow!("--visibility must be in `<kind>=<true|false>` form, got '{pair}'")
        })?;
        let value: bool = value
            .parse()
            .map_err(|_| anyhow::anyhow!("--visibility value must be true or false: '{pair}'"))?;
        map.insert(kind.to_string(), value);
    }
    Ok(map)
}

fn print_member_profile(profile: &DiagnosisMemberProfile) {
    println!("{}", profile.member_id.bold());
    if profile.results.is_empty() {
        println!("  {}", "No diagnosis results.".dimmed());
        return;
    }
    for result in &profile.results {
        println!("  {:<12} {}", result.diagnosis_kind, result.status);
    }
}

pub async fn handle_diagnosis(cmd: &DiagnosisCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        DiagnosisCommands::Save {
            kind,
            schema_version,
            result,
            result_file,
            json,
        } => {
            let payload = parse_result_json(result.as_ref(), result_file.as_ref())?;
            let saved = client
                .save_diagnosis_result(kind.as_str(), schema_version, payload)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&saved)?);
            } else {
                println!(
                    "Diagnosis result saved: {} ({})",
                    saved.id, saved.diagnosis_kind
                );
            }
            Ok(())
        }
        DiagnosisCommands::List { json } => {
            let results = client.list_my_diagnosis_results().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else if results.is_empty() {
                println!("{}", "No diagnosis results found.".dimmed());
            } else {
                for result in &results {
                    println!(
                        "{:<12} schema={:<10} updated={}",
                        result.diagnosis_kind, result.schema_version, result.updated_at
                    );
                }
            }
            Ok(())
        }
        DiagnosisCommands::Get { kind, json } => {
            let result = client.get_my_diagnosis_result(kind.as_str()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&result.result)?);
            }
            Ok(())
        }
        DiagnosisCommands::Stats { kind, json } => {
            if !kind.supports_stats() {
                bail!(
                    "--kind {} does not support aggregate stats (only goal-style/values do)",
                    kind.as_str()
                );
            }
            let stats = client.get_diagnosis_stats(kind.as_str()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("{} — total: {}", stats.diagnosis_kind, stats.total);
                for bucket in &stats.distribution {
                    println!("  {:<10} {}", bucket.type_code, bucket.count);
                }
            }
            Ok(())
        }
        DiagnosisCommands::Visibility { command } => handle_visibility(command, client).await,
        DiagnosisCommands::Profiles {
            org,
            member_ids,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let profiles = scoped
                .list_member_diagnosis_profiles(&org_id, member_ids)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&profiles)?);
            } else if profiles.is_empty() {
                println!("{}", "No diagnosis profiles found.".dimmed());
            } else {
                for profile in &profiles {
                    print_member_profile(profile);
                }
            }
            Ok(())
        }
        DiagnosisCommands::Profile {
            org,
            member_id,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let profile = scoped
                .get_member_diagnosis_profile(&org_id, member_id)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&profile)?);
            } else {
                print_member_profile(&profile);
            }
            Ok(())
        }
    }
}

async fn handle_visibility(cmd: &VisibilityCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        VisibilityCommands::Get { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let visibility = scoped.get_diagnosis_visibility(&org_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&visibility)?);
            } else {
                println!("default_public: {}", visibility.default_public);
                for (kind, visible) in &visibility.visibilities {
                    println!("  {kind:<12} {visible}");
                }
            }
            Ok(())
        }
        VisibilityCommands::Set {
            org,
            visibilities,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let map = parse_visibility_pairs(visibilities)?;
            let scoped = client_for_org(client, &org_id);
            let visibility = scoped.update_diagnosis_visibility(&org_id, map).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&visibility)?);
            } else {
                println!("Diagnosis visibility updated.");
                for (kind, visible) in &visibility.visibilities {
                    println!("  {kind:<12} {visible}");
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DiagnosisKind, parse_result_json, parse_visibility_pairs};

    #[test]
    fn diagnosis_kind_as_str_maps_to_backend_values() {
        assert_eq!(DiagnosisKind::GoalStyle.as_str(), "goal_style");
        assert_eq!(DiagnosisKind::Values.as_str(), "values");
        assert_eq!(DiagnosisKind::CoreValues.as_str(), "core_values");
        assert_eq!(DiagnosisKind::MasterPlan.as_str(), "master_plan");
    }

    #[test]
    fn diagnosis_kind_supports_stats_only_for_typed_kinds() {
        assert!(DiagnosisKind::GoalStyle.supports_stats());
        assert!(DiagnosisKind::Values.supports_stats());
        assert!(!DiagnosisKind::CoreValues.supports_stats());
        assert!(!DiagnosisKind::MasterPlan.supports_stats());
    }

    #[test]
    fn parse_result_json_accepts_inline_object() {
        let value = parse_result_json(Some(&"{\"typeCode\":\"ENFJ\"}".to_string()), None).unwrap();
        assert_eq!(value["typeCode"], "ENFJ");
    }

    #[test]
    fn parse_result_json_rejects_non_object() {
        let err = parse_result_json(Some(&"[1,2,3]".to_string()), None).unwrap_err();
        assert!(err.to_string().contains("JSON object"));
    }

    #[test]
    fn parse_result_json_rejects_invalid_json() {
        let err = parse_result_json(Some(&"not json".to_string()), None).unwrap_err();
        assert!(err.to_string().contains("valid JSON"));
    }

    #[test]
    fn parse_visibility_pairs_parses_bool_values() {
        let map =
            parse_visibility_pairs(&["goal_style=true".to_string(), "values=false".to_string()])
                .unwrap();
        assert_eq!(map.get("goal_style"), Some(&true));
        assert_eq!(map.get("values"), Some(&false));
    }

    #[test]
    fn parse_visibility_pairs_rejects_missing_equals() {
        let err = parse_visibility_pairs(&["goal_style".to_string()]).unwrap_err();
        assert!(err.to_string().contains("<kind>=<true|false>"));
    }

    #[test]
    fn parse_visibility_pairs_rejects_non_bool_value() {
        let err = parse_visibility_pairs(&["goal_style=maybe".to_string()]).unwrap_err();
        assert!(err.to_string().contains("true or false"));
    }
}
