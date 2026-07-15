use anyhow::{Result, bail};
use clap::{Subcommand, ValueEnum};

use crate::api::{
    ApiClient, ListUsersParams, UserCreateRequest, UserSettingUpdateRequest, UserUpdateRequest,
};

/// Gender values accepted by the Addness backend for a user profile.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum UserGender {
    Male,
    Female,
    Other,
    NotSpecified,
}

impl UserGender {
    fn as_str(self) -> &'static str {
        match self {
            UserGender::Male => "MALE",
            UserGender::Female => "FEMALE",
            UserGender::Other => "OTHER",
            UserGender::NotSpecified => "NOT_SPECIFIED",
        }
    }
}

#[derive(Subcommand)]
pub enum UserCommands {
    /// Show your own user profile
    Me {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a user by ID. Only works for your own user ID; the server enforces
    /// self-only access (403 Forbidden for any other user's ID).
    Get {
        /// User ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update your own profile (name, avatar, gender, date of birth)
    Update {
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New avatar URL
        #[arg(long)]
        avatar_url: Option<String>,
        /// New gender
        #[arg(long, value_enum)]
        gender: Option<UserGender>,
        /// New date of birth (YYYY-MM-DD)
        #[arg(long, conflicts_with = "clear_date_of_birth", value_name = "DATE")]
        date_of_birth: Option<String>,
        /// Clear the date of birth (set to null)
        #[arg(long, conflicts_with = "date_of_birth")]
        clear_date_of_birth: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Search/list Addness users (requires an active subscription)
    List {
        /// Filter by name (substring match on backend)
        #[arg(long)]
        name: Option<String>,
        /// Filter by email
        #[arg(long)]
        email: Option<String>,
        /// Max users to return (1-100, default 10)
        #[arg(long)]
        limit: Option<u16>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u64>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a new Addness user account
    Create {
        /// Display name (1-100 chars)
        #[arg(long)]
        name: String,
        /// Email address
        #[arg(long)]
        email: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete your own Addness user account (destructive, irreversible)
    Rm {
        /// User ID (must be your own ID; the server enforces self-only access)
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// List the organizations you belong to (one row per membership)
    Memberships {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage your user settings
    Settings {
        #[command(subcommand)]
        command: UserSettingsCommands,
    },
}

#[derive(Subcommand)]
pub enum UserSettingsCommands {
    /// Show your user settings
    Get {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update your user settings
    Update {
        /// Receive calendar events
        #[arg(long)]
        receive_calendar_events: Option<bool>,
        /// Calendar organization member ID (UUID)
        #[arg(long)]
        calendar_organization_member: Option<String>,
        /// Enable/disable goal decomposition assistance
        #[arg(long)]
        goal_decompose_enabled: Option<bool>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// `--date-of-birth`/`--clear-date-of-birth` から実際に送信するリクエスト値を決定する
/// （goal.rsの `--due-date`/`--clear-due-date` パターンに合わせる）。
/// `dateOfBirth` はサーバー側で `*string` なので、空文字はクリア、Noneは「変更しない」を意味する。
fn resolve_date_of_birth_update(date_of_birth: Option<&str>, clear: bool) -> Option<String> {
    if clear {
        Some(String::new())
    } else {
        date_of_birth.map(|s| s.to_string())
    }
}

/// プロフィール更新に最低1つのフィールドが指定されているか検証する。
fn ensure_user_update_has_fields(
    name: Option<&str>,
    avatar_url: Option<&str>,
    gender: Option<UserGender>,
    date_of_birth: Option<&str>,
    clear_date_of_birth: bool,
) -> Result<()> {
    if name.is_none()
        && avatar_url.is_none()
        && gender.is_none()
        && date_of_birth.is_none()
        && !clear_date_of_birth
    {
        bail!("Specify at least one field to update.");
    }
    Ok(())
}

/// 設定更新に最低1つのフィールドが指定されているか検証する。
fn ensure_settings_update_has_fields(
    receive_calendar_events: Option<bool>,
    calendar_organization_member: Option<&str>,
    goal_decompose_enabled: Option<bool>,
) -> Result<()> {
    if receive_calendar_events.is_none()
        && calendar_organization_member.is_none()
        && goal_decompose_enabled.is_none()
    {
        bail!("Specify at least one field to update.");
    }
    Ok(())
}

fn print_user_summary(user: &crate::api::User) {
    println!("{} — {}", user.id, user.name);
    if !user.email.is_empty() {
        println!("  Email: {}", user.email);
    }
    if !user.gender.is_empty() {
        println!("  Gender: {}", user.gender);
    }
    if let Some(dob) = &user.date_of_birth {
        println!("  Date of birth: {dob}");
    }
}

pub async fn handle_user(cmd: &UserCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        UserCommands::Me { json } => {
            let user = client.get_current_user().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&user)?);
            } else {
                print_user_summary(&user);
            }
            Ok(())
        }
        UserCommands::Get { id, json } => {
            let user = client.get_user(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&user)?);
            } else {
                print_user_summary(&user);
            }
            Ok(())
        }
        UserCommands::Update {
            name,
            avatar_url,
            gender,
            date_of_birth,
            clear_date_of_birth,
            json,
        } => {
            ensure_user_update_has_fields(
                name.as_deref(),
                avatar_url.as_deref(),
                *gender,
                date_of_birth.as_deref(),
                *clear_date_of_birth,
            )?;

            let own_id = client.get_current_user().await?.id;
            let req = UserUpdateRequest {
                name: name.clone(),
                avatar_url: avatar_url.clone(),
                gender: gender.map(|g| g.as_str().to_string()),
                date_of_birth: resolve_date_of_birth_update(
                    date_of_birth.as_deref(),
                    *clear_date_of_birth,
                ),
            };
            let user = client.update_user(&own_id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&user)?);
            } else {
                println!("Updated user {}", user.id);
                print_user_summary(&user);
            }
            Ok(())
        }
        UserCommands::List {
            name,
            email,
            limit,
            offset,
            json,
        } => {
            let resp = client
                .list_users(ListUsersParams {
                    name: name.as_deref(),
                    email: email.as_deref(),
                    limit: *limit,
                    offset: *offset,
                })
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.data.is_empty() {
                println!("No users found.");
            } else {
                for u in &resp.data {
                    println!("{} — {} ({})", u.id, u.name, u.gender);
                }
                let shown = resp.pagination.offset + resp.data.len() as i64;
                if shown < resp.pagination.total {
                    println!("More users available (use --offset or --limit).");
                }
            }
            Ok(())
        }
        UserCommands::Create { name, email, json } => {
            let req = UserCreateRequest {
                name: name.clone(),
                email: email.clone(),
            };
            let user = client.create_user(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&user)?);
            } else {
                println!("Created user {} ({})", user.id, user.name);
            }
            Ok(())
        }
        UserCommands::Rm { id, force } => {
            if !*force
                && !crate::cli::commands::confirm(&format!(
                    "Delete your Addness user account {id}? This cannot be undone."
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_user(id).await?;
            println!("Deleted user {id}");
            Ok(())
        }
        UserCommands::Memberships { json } => {
            let memberships = client.list_user_organization_memberships().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&memberships)?);
            } else if memberships.is_empty() {
                println!("No organization memberships.");
            } else {
                for m in &memberships {
                    let marker = if m.organization.is_my_organization {
                        " (you)"
                    } else {
                        ""
                    };
                    println!("{} — {}{marker}", m.organization.id, m.organization.name);
                }
            }
            Ok(())
        }
        UserCommands::Settings { command } => handle_user_settings(command, client).await,
    }
}

pub async fn handle_user_settings(cmd: &UserSettingsCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        UserSettingsCommands::Get { json } => {
            let settings = client.get_user_settings().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&settings)?);
            } else {
                println!("Settings {}", settings.id);
                println!(
                    "  Receive calendar events: {}",
                    settings.receive_calendar_events
                );
                println!(
                    "  Goal decompose enabled: {}",
                    settings.goal_decompose_enabled
                );
                if let Some(member_id) = &settings.calendar_organization_member_id {
                    println!("  Calendar organization member: {member_id}");
                }
            }
            Ok(())
        }
        UserSettingsCommands::Update {
            receive_calendar_events,
            calendar_organization_member,
            goal_decompose_enabled,
            json,
        } => {
            ensure_settings_update_has_fields(
                *receive_calendar_events,
                calendar_organization_member.as_deref(),
                *goal_decompose_enabled,
            )?;
            let req = UserSettingUpdateRequest {
                receive_calendar_events: *receive_calendar_events,
                calendar_organization_member_id: calendar_organization_member.clone(),
                goal_decompose_enabled: *goal_decompose_enabled,
            };
            let settings = client.update_user_settings(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&settings)?);
            } else {
                println!("Updated settings {}", settings.id);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        UserGender, ensure_settings_update_has_fields, ensure_user_update_has_fields,
        resolve_date_of_birth_update,
    };

    #[test]
    fn user_gender_as_str_maps_to_backend_values() {
        assert_eq!(UserGender::Male.as_str(), "MALE");
        assert_eq!(UserGender::Female.as_str(), "FEMALE");
        assert_eq!(UserGender::Other.as_str(), "OTHER");
        assert_eq!(UserGender::NotSpecified.as_str(), "NOT_SPECIFIED");
    }

    #[test]
    fn resolve_date_of_birth_update_prefers_clear() {
        assert_eq!(
            resolve_date_of_birth_update(Some("2000-01-01"), true),
            Some(String::new())
        );
    }

    #[test]
    fn resolve_date_of_birth_update_uses_value_when_not_clearing() {
        assert_eq!(
            resolve_date_of_birth_update(Some("2000-01-01"), false),
            Some("2000-01-01".to_string())
        );
    }

    #[test]
    fn resolve_date_of_birth_update_none_means_leave_untouched() {
        assert_eq!(resolve_date_of_birth_update(None, false), None);
    }

    #[test]
    fn ensure_user_update_has_fields_rejects_all_none() {
        let err = ensure_user_update_has_fields(None, None, None, None, false).unwrap_err();
        assert!(err.to_string().contains("at least one field"));
    }

    #[test]
    fn ensure_user_update_has_fields_accepts_clear_date_of_birth_alone() {
        assert!(ensure_user_update_has_fields(None, None, None, None, true).is_ok());
    }

    #[test]
    fn ensure_user_update_has_fields_accepts_single_field() {
        assert!(ensure_user_update_has_fields(Some("name"), None, None, None, false).is_ok());
    }

    #[test]
    fn ensure_settings_update_has_fields_rejects_all_none() {
        let err = ensure_settings_update_has_fields(None, None, None).unwrap_err();
        assert!(err.to_string().contains("at least one field"));
    }

    #[test]
    fn ensure_settings_update_has_fields_accepts_single_field() {
        assert!(ensure_settings_update_has_fields(Some(true), None, None).is_ok());
    }
}
