use anyhow::{Context, Result, bail};
use clap::{Subcommand, ValueEnum};
use colored::Colorize;

use crate::api::{
    ApiClient, ApiResponse, CalendarEventCompletionRequest, CreateGoalRequest,
    CreatePlannedTodoRequest, CreateTodayTodoRequest, Goal, GoalStatus,
    RecordTodayTodoActivityRequest, TodaysGoalsData, UpdateChatTodayTodoRequest, UpdateGoalRequest,
    UpdatePlannedTodoRequest,
};
use crate::cli::commands::goal::parse_status;
use crate::cli::commands::org::resolve_org_id;
use crate::cli::output::resolve_status;

/// `status` accepted by the today-todos / planned-todos rows
/// (`internal/goalexecution/usecase/today_todos_create_chat.go` validates
/// exactly these four values; distinct from `GoalStatus`, which has no
/// `COMPLETED` variant because objectives track completion via
/// `completedAt`).
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum TodoStatus {
    None,
    InProgress,
    Completed,
    Cancelled,
}

impl TodoStatus {
    fn as_str(self) -> &'static str {
        match self {
            TodoStatus::None => "NONE",
            TodoStatus::InProgress => "IN_PROGRESS",
            TodoStatus::Completed => "COMPLETED",
            TodoStatus::Cancelled => "CANCELLED",
        }
    }
}

/// `action` accepted by `today todo activity`
/// (`domain/goalexecution` `TodayTodoAction*` constants).
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum TodoActivityAction {
    Engage,
    Progress,
    Complete,
}

impl TodoActivityAction {
    fn as_str(self) -> &'static str {
        match self {
            TodoActivityAction::Engage => "ENGAGE",
            TodoActivityAction::Progress => "PROGRESS",
            TodoActivityAction::Complete => "COMPLETE",
        }
    }
}

fn parse_chat_metadata(raw: &str) -> Result<serde_json::Value> {
    serde_json::from_str(raw).context("--chat-metadata must be valid JSON")
}

/// `today todo add` requires exactly one of `--objective-id` (pin an
/// existing goal) or `--title` (create an addness.chat-origin ad-hoc item);
/// the backend dispatches on `objectiveId` presence
/// (`today_todos_add.go` `Add`).
fn validate_todo_add_args(objective_id: Option<&str>, title: Option<&str>) -> Result<()> {
    if objective_id.is_none() && title.is_none() {
        bail!(
            "Specify either --objective-id (pin an existing goal) or --title (create an ad-hoc item)"
        );
    }
    Ok(())
}

/// `today planned update --recurrence` (set) and `--clear-recurrence`
/// (clear, sent as JSON `null`) are mutually exclusive ways to touch the
/// same field.
fn validate_planned_update_args(recurrence: Option<&str>, clear_recurrence: bool) -> Result<()> {
    if recurrence.is_some() && clear_recurrence {
        bail!("--recurrence and --clear-recurrence are mutually exclusive");
    }
    Ok(())
}

#[derive(Subcommand)]
pub enum TodayCommands {
    /// List today's goals (default when no subcommand is given)
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Date in YYYY-MM-DD (defaults to today)
        #[arg(long)]
        date: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Add a new goal as today's todo
    Add {
        /// Goal title
        #[arg(long)]
        title: String,
        /// Parent goal ID (omit to create as root goal)
        #[arg(long)]
        parent: Option<String>,
        /// Description (definition of done)
        #[arg(long)]
        description: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark a today's goal as completed
    Done {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Reopen a completed goal (mark as not completed)
    Reopen {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Change a goal's status (NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED)
    Status {
        /// Goal ID
        id: String,
        /// Status: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED
        status: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage "today's todo" rows (goal-linked or addness.chat-origin ad-hoc items)
    Todo {
        #[command(subcommand)]
        command: TodoCommands,
    },
    /// Manage the "material pool" of scheduled/recurring/backlog todos you can adopt into today
    Planned {
        #[command(subcommand)]
        command: PlannedCommands,
    },
    /// Read external calendar events and the goal-calendar/goal-history heatmaps
    Calendar {
        #[command(subcommand)]
        command: CalendarCommands,
    },
}

#[derive(Subcommand)]
pub enum TodoCommands {
    /// List today's todo rows (goal-linked + addness.chat-origin)
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Date in YYYY-MM-DD (defaults to today)
        #[arg(long)]
        date: Option<String>,
        /// Filter to a specific organization member (UUID, defaults to yourself)
        #[arg(long)]
        member_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Add a today's todo row: pass --objective-id to add an existing goal (idempotent),
    /// or --title to create an addness.chat-origin ad-hoc item
    Add {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Activity date in YYYY-MM-DD (defaults to today)
        #[arg(long)]
        date: Option<String>,
        /// Existing goal ID to pin as today's todo (mutually exclusive with --title)
        #[arg(long)]
        objective_id: Option<String>,
        /// Title for an addness.chat-origin ad-hoc item (required without --objective-id)
        #[arg(long)]
        title: Option<String>,
        /// Detail text (addness.chat-origin only)
        #[arg(long)]
        detail: Option<String>,
        /// Execution date in YYYY-MM-DD (addness.chat-origin only)
        #[arg(long)]
        execution_date: Option<String>,
        /// Definition of done (addness.chat-origin only)
        #[arg(long)]
        definition_of_done: Option<String>,
        /// Free-text current status note (addness.chat-origin only)
        #[arg(long)]
        current_status: Option<String>,
        /// Status (addness.chat-origin only)
        #[arg(long, value_enum)]
        status: Option<TodoStatus>,
        /// Manual sort order (addness.chat-origin only)
        #[arg(long)]
        sort_order: Option<i32>,
        /// Opaque JSON metadata (addness.chat-origin only)
        #[arg(long)]
        chat_metadata: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update an addness.chat-origin today's todo row (goal-linked rows have no editable fields here)
    Update {
        /// Today's todo ID (addness.chat-origin row)
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New detail text
        #[arg(long)]
        detail: Option<String>,
        /// New execution date (YYYY-MM-DD); pass an empty string to clear it
        #[arg(long)]
        execution_date: Option<String>,
        /// New definition of done
        #[arg(long)]
        definition_of_done: Option<String>,
        /// New free-text current status note
        #[arg(long)]
        current_status: Option<String>,
        /// New status
        #[arg(long, value_enum)]
        status: Option<TodoStatus>,
        /// New manual sort order
        #[arg(long)]
        sort_order: Option<i32>,
        /// New opaque JSON metadata (replaces the whole object)
        #[arg(long)]
        chat_metadata: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a today's todo row (accepts either an addness.chat-origin id or a goal ID)
    Rm {
        /// Today's todo ID or goal ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Activity date in YYYY-MM-DD, used for the goal-ID fallback (defaults to today)
        #[arg(long)]
        date: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Record an engage/progress/complete activity event on an addness.chat-origin row
    Activity {
        /// Today's todo ID (addness.chat-origin row)
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Activity action
        #[arg(long, value_enum)]
        action: TodoActivityAction,
        /// Idempotency key (defaults to a freshly generated UUID)
        #[arg(long)]
        idempotency_key: Option<String>,
        /// Free-text current status note to record alongside the activity
        #[arg(long)]
        current_status: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum PlannedCommands {
    /// List every planned todo in the material pool (flat, all statuses)
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the material pool split into due/overdue, recurring-today, and backlog
    Material {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Date in YYYY-MM-DD (defaults to today)
        #[arg(long)]
        date: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Add a planned todo: no dates → backlog; --scheduled-date → a fixed plan;
    /// --recurrence → a recurring template
    Add {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Title
        #[arg(long)]
        title: String,
        /// Detail text
        #[arg(long)]
        detail: Option<String>,
        /// Definition of done
        #[arg(long)]
        definition_of_done: Option<String>,
        /// Free-text current status note
        #[arg(long)]
        current_status: Option<String>,
        /// Status
        #[arg(long, value_enum)]
        status: Option<TodoStatus>,
        /// Fixed scheduled date in YYYY-MM-DD
        #[arg(long)]
        scheduled_date: Option<String>,
        /// Recurrence rule as raw JSON (same shape as `recurring_goals`)
        #[arg(long)]
        recurrence: Option<String>,
        /// Manual sort order
        #[arg(long)]
        sort_order: Option<i32>,
        /// Opaque JSON metadata
        #[arg(long)]
        chat_metadata: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a planned todo
    Update {
        /// Planned todo ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New detail text
        #[arg(long)]
        detail: Option<String>,
        /// New definition of done
        #[arg(long)]
        definition_of_done: Option<String>,
        /// New free-text current status note
        #[arg(long)]
        current_status: Option<String>,
        /// New status
        #[arg(long, value_enum)]
        status: Option<TodoStatus>,
        /// New scheduled date (YYYY-MM-DD); pass an empty string to un-schedule it
        #[arg(long)]
        scheduled_date: Option<String>,
        /// New recurrence rule as raw JSON
        #[arg(long)]
        recurrence: Option<String>,
        /// Clear the recurrence rule (mutually exclusive with --recurrence)
        #[arg(long)]
        clear_recurrence: bool,
        /// New manual sort order
        #[arg(long)]
        sort_order: Option<i32>,
        /// New opaque JSON metadata (replaces the whole object)
        #[arg(long)]
        chat_metadata: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a planned todo from the material pool
    Rm {
        /// Planned todo ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Adopt a planned todo into today's todos (non-recurring items are consumed)
    Adopt {
        /// Planned todo ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum CalendarCommands {
    /// List today's connected external calendar events
    Events {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Date in YYYY-MM-DD (defaults to today)
        #[arg(long)]
        date: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark an external calendar event completed/incomplete for today's todo tracking
    Complete {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Activity date in YYYY-MM-DD
        #[arg(long)]
        date: String,
        /// Calendar event ID
        #[arg(long)]
        event_id: String,
        /// Event title (for activity-log context)
        #[arg(long)]
        event_title: Option<String>,
        /// Calendar name (for activity-log context)
        #[arg(long)]
        calendar_name: Option<String>,
        /// Event start time (RFC3339)
        #[arg(long)]
        event_start: String,
        /// Mark as completed (pass --completed=false to un-complete)
        #[arg(long, default_value_t = true)]
        completed: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the goal-completion heatmap for a date range
    GoalCalendar {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Range start in YYYY-MM-DD
        #[arg(long)]
        from: String,
        /// Range end in YYYY-MM-DD (max 366 days after --from)
        #[arg(long)]
        to: String,
        /// Filter to a specific organization member (UUID)
        #[arg(long)]
        member_id: Option<String>,
        /// Include per-day completed-goal counts
        #[arg(long)]
        include_counts: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the goals that were on a past date's "today" list
    GoalHistory {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Date in YYYY-MM-DD
        #[arg(long)]
        date: String,
        /// Filter to a specific organization member (UUID)
        #[arg(long)]
        member_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Handle `today` command. `None` means no subcommand → list today's goals.
pub async fn handle_today(cmd: Option<&TodayCommands>, client: &ApiClient) -> Result<()> {
    match cmd {
        None => list_todays_goals(None, None, false, client).await,
        Some(TodayCommands::List { org, date, json }) => {
            list_todays_goals(org.as_deref(), date.as_deref(), *json, client).await
        }
        Some(TodayCommands::Add {
            title,
            parent,
            description,
            org,
            json,
        }) => {
            let org_id = resolve_org_id(org.as_deref())?;
            let req = CreateGoalRequest {
                organization_id: org_id,
                title: title.clone(),
                parent_objective_id: parent.clone(),
                description: description.clone(),
            };
            let resp: ApiResponse<Goal> = client.create_goal(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                println!("Added today's todo: {} ({})", resp.data.title, resp.data.id);
            }
            Ok(())
        }
        Some(TodayCommands::Done { id, json }) => {
            let req = UpdateGoalRequest {
                status: None,
                completed_at: Some(Some(chrono::Utc::now().to_rfc3339())),
                title: None,
                description: None,
                body: None,
                due_date: None,
            };
            update_and_report(id, &req, "Completed", *json, client).await
        }
        Some(TodayCommands::Reopen { id, json }) => {
            let req = UpdateGoalRequest {
                status: Some(GoalStatus::None),
                completed_at: Some(None),
                title: None,
                description: None,
                body: None,
                due_date: None,
            };
            update_and_report(id, &req, "Reopened", *json, client).await
        }
        Some(TodayCommands::Status { id, status, json }) => {
            let (completed_at, goal_status) = parse_status(status)?;
            let req = UpdateGoalRequest {
                status: goal_status,
                completed_at,
                title: None,
                description: None,
                body: None,
                due_date: None,
            };
            update_and_report(id, &req, "Updated", *json, client).await
        }
        Some(TodayCommands::Todo { command }) => handle_todo(command, client).await,
        Some(TodayCommands::Planned { command }) => handle_planned(command, client).await,
        Some(TodayCommands::Calendar { command }) => handle_calendar(command, client).await,
    }
}

async fn handle_todo(cmd: &TodoCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        TodoCommands::List {
            org,
            date,
            member_id,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let todos = client
                .get_today_todos(&org_id, date.as_deref(), member_id.as_deref())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&todos)?);
            } else if todos.is_empty() {
                println!("No today's todos.");
            } else {
                for todo in &todos {
                    print_today_todo_line(todo);
                }
            }
            Ok(())
        }
        TodoCommands::Add {
            org,
            date,
            objective_id,
            title,
            detail,
            execution_date,
            definition_of_done,
            current_status,
            status,
            sort_order,
            chat_metadata,
            json,
        } => {
            validate_todo_add_args(objective_id.as_deref(), title.as_deref())?;
            let org_id = resolve_org_id(org.as_deref())?;
            let req = CreateTodayTodoRequest {
                date: date.clone(),
                objective_id: objective_id.clone(),
                title: title.clone(),
                detail: detail.clone(),
                execution_date: execution_date.clone(),
                definition_of_done: definition_of_done.clone(),
                current_status: current_status.clone(),
                status: status.map(|s| s.as_str().to_string()),
                sort_order: *sort_order,
                chat_metadata: chat_metadata
                    .as_deref()
                    .map(parse_chat_metadata)
                    .transpose()?,
            };
            let todo = client.add_today_todo(&org_id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&todo)?);
            } else {
                println!("Added today's todo.");
                print_today_todo_line(&todo);
            }
            Ok(())
        }
        TodoCommands::Update {
            id,
            org,
            title,
            detail,
            execution_date,
            definition_of_done,
            current_status,
            status,
            sort_order,
            chat_metadata,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let req = UpdateChatTodayTodoRequest {
                title: title.clone(),
                detail: detail.clone(),
                execution_date: execution_date.clone(),
                definition_of_done: definition_of_done.clone(),
                current_status: current_status.clone(),
                status: status.map(|s| s.as_str().to_string()),
                sort_order: *sort_order,
                chat_metadata: chat_metadata
                    .as_deref()
                    .map(parse_chat_metadata)
                    .transpose()?,
            };
            let todo = client.update_chat_today_todo(&org_id, id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&todo)?);
            } else {
                println!("Updated today's todo.");
                print_today_todo_line(&todo);
            }
            Ok(())
        }
        TodoCommands::Rm {
            id,
            org,
            date,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            client
                .delete_today_todo(&org_id, id, date.as_deref())
                .await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"deleted": true}))?
                );
            } else {
                println!("Removed today's todo {id}");
            }
            Ok(())
        }
        TodoCommands::Activity {
            id,
            org,
            action,
            idempotency_key,
            current_status,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let idempotency_key = idempotency_key
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let req = RecordTodayTodoActivityRequest {
                action: action.as_str().to_string(),
                idempotency_key,
                current_status: current_status.clone(),
            };
            let activity = client.record_today_todo_activity(&org_id, id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&activity)?);
            } else {
                println!(
                    "Recorded {} activity on {} ({})",
                    activity.kind, id, activity.current_state
                );
            }
            Ok(())
        }
    }
}

fn print_today_todo_line(todo: &crate::api::TodayTodoView) {
    match &todo.title {
        Some(title) => {
            let id = todo.id.as_deref().unwrap_or_default();
            let status = todo.status.as_deref().unwrap_or("NONE");
            println!(
                "{} [{}] {} {}",
                title,
                status,
                id.dimmed(),
                "(chat)".dimmed()
            );
        }
        None => {
            let objective_id = todo.objective_id.as_deref().unwrap_or_default();
            println!("{} {}", objective_id, "(goal)".dimmed());
        }
    }
}

async fn handle_planned(cmd: &PlannedCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        PlannedCommands::List { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let planned = client.list_planned_todos(&org_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&planned)?);
            } else if planned.is_empty() {
                println!("No planned todos.");
            } else {
                for item in &planned {
                    print_planned_todo_line(item);
                }
            }
            Ok(())
        }
        PlannedCommands::Material { org, date, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let material = client
                .get_planned_todo_material(&org_id, date.as_deref())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&material)?);
            } else {
                println!("Due/overdue ({}):", material.due_or_overdue.len());
                for item in &material.due_or_overdue {
                    print_planned_todo_line(item);
                }
                println!("Recurring today ({}):", material.recurring_today.len());
                for item in &material.recurring_today {
                    print_planned_todo_line(item);
                }
                println!("Backlog ({}):", material.backlog.len());
                for item in &material.backlog {
                    print_planned_todo_line(item);
                }
            }
            Ok(())
        }
        PlannedCommands::Add {
            org,
            title,
            detail,
            definition_of_done,
            current_status,
            status,
            scheduled_date,
            recurrence,
            sort_order,
            chat_metadata,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let req = CreatePlannedTodoRequest {
                title: title.clone(),
                detail: detail.clone().unwrap_or_default(),
                definition_of_done: definition_of_done.clone().unwrap_or_default(),
                current_status: current_status.clone().unwrap_or_default(),
                status: status.map(|s| s.as_str().to_string()).unwrap_or_default(),
                scheduled_date: scheduled_date.clone(),
                recurrence: recurrence.as_deref().map(parse_chat_metadata).transpose()?,
                sort_order: sort_order.unwrap_or_default(),
                chat_metadata: chat_metadata
                    .as_deref()
                    .map(parse_chat_metadata)
                    .transpose()?,
            };
            let planned = client.create_planned_todo(&org_id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&planned)?);
            } else {
                println!("Added planned todo.");
                print_planned_todo_line(&planned);
            }
            Ok(())
        }
        PlannedCommands::Update {
            id,
            org,
            title,
            detail,
            definition_of_done,
            current_status,
            status,
            scheduled_date,
            recurrence,
            clear_recurrence,
            sort_order,
            chat_metadata,
            json,
        } => {
            validate_planned_update_args(recurrence.as_deref(), *clear_recurrence)?;
            let org_id = resolve_org_id(org.as_deref())?;
            let recurrence_value = if *clear_recurrence {
                Some(serde_json::Value::Null)
            } else {
                recurrence.as_deref().map(parse_chat_metadata).transpose()?
            };
            let req = UpdatePlannedTodoRequest {
                title: title.clone(),
                detail: detail.clone(),
                definition_of_done: definition_of_done.clone(),
                current_status: current_status.clone(),
                status: status.map(|s| s.as_str().to_string()),
                scheduled_date: scheduled_date.clone(),
                recurrence: recurrence_value,
                sort_order: *sort_order,
                chat_metadata: chat_metadata
                    .as_deref()
                    .map(parse_chat_metadata)
                    .transpose()?,
            };
            let planned = client.update_planned_todo(&org_id, id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&planned)?);
            } else {
                println!("Updated planned todo.");
                print_planned_todo_line(&planned);
            }
            Ok(())
        }
        PlannedCommands::Rm { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client.delete_planned_todo(&org_id, id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.deleted {
                println!("Removed planned todo {id}");
            } else {
                println!("No planned todo removed (already gone): {id}");
            }
            Ok(())
        }
        PlannedCommands::Adopt { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let todo = client.adopt_planned_todo(&org_id, id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&todo)?);
            } else {
                println!("Adopted planned todo {id} into today's todos.");
                print_today_todo_line(&todo);
            }
            Ok(())
        }
    }
}

fn print_planned_todo_line(item: &crate::api::PlannedTodoView) {
    let schedule = if !item.scheduled_date.is_empty() {
        item.scheduled_date.clone()
    } else if !item.recurrence_text.is_empty() {
        item.recurrence_text.clone()
    } else {
        "backlog".to_string()
    };
    println!(
        "{} [{}] {} {}",
        item.title,
        schedule,
        item.id.dimmed(),
        if item.status.is_empty() {
            String::new()
        } else {
            format!("({})", item.status)
        }
    );
}

fn print_goal_calendar(calendar: &crate::api::GoalCalendarResponse) {
    let mut years: Vec<&String> = calendar.keys().collect();
    years.sort();
    for year in years {
        let months = &calendar[year];
        let mut month_keys: Vec<&String> = months.keys().collect();
        month_keys.sort();
        for month in month_keys {
            let days = &months[month];
            let mut day_keys: Vec<&String> = days.keys().collect();
            day_keys.sort();
            for day in day_keys {
                let data = &days[day];
                let mark = if data.completed_goal_exists {
                    "[x]"
                } else if data.goal_exists {
                    "[ ]"
                } else if data.frozen {
                    "[*]"
                } else {
                    "[.]"
                };
                let count = if data.completed_count > 0 {
                    format!(" ({})", data.completed_count)
                } else {
                    String::new()
                };
                println!("{year}-{month}-{day} {mark}{count}");
            }
        }
    }
}

async fn handle_calendar(cmd: &CalendarCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        CalendarCommands::Events { org, date, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let events = client.get_calendar_events(&org_id, date.as_deref()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&events)?);
            } else if events.is_empty() {
                println!("No calendar events for today.");
            } else {
                for event in &events {
                    let done = if event.completed_at.is_some() {
                        "[x]"
                    } else {
                        "[ ]"
                    };
                    println!("{done} {} {}", event.title, event.start.dimmed());
                }
            }
            Ok(())
        }
        CalendarCommands::Complete {
            org,
            date,
            event_id,
            event_title,
            calendar_name,
            event_start,
            completed,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let req = CalendarEventCompletionRequest {
                date: date.clone(),
                event_id: event_id.clone(),
                event_title: event_title.clone().unwrap_or_default(),
                calendar_name: calendar_name.clone().unwrap_or_default(),
                event_start: event_start.clone(),
                completed: *completed,
            };
            let resp = client.complete_calendar_event(&org_id, &req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if *completed {
                println!("Marked calendar event {event_id} completed.");
            } else {
                println!("Marked calendar event {event_id} incomplete.");
            }
            Ok(())
        }
        CalendarCommands::GoalCalendar {
            org,
            from,
            to,
            member_id,
            include_counts,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let calendar = client
                .get_goal_calendar(&org_id, from, to, member_id.as_deref(), *include_counts)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&calendar)?);
            } else {
                print_goal_calendar(&calendar);
            }
            Ok(())
        }
        CalendarCommands::GoalHistory {
            org,
            date,
            member_id,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let history = client
                .get_goal_history(&org_id, date, member_id.as_deref())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&history)?);
            } else if history.nodes.is_empty() {
                println!("No goals in history for {date}.");
            } else {
                for node in &history.nodes {
                    let indent = "  ".repeat(node.depth.max(0) as usize);
                    let check = if node.is_completed() { "[x]" } else { "[ ]" };
                    println!("{indent}{check} {} {}", node.title, node.id.dimmed());
                }
            }
            Ok(())
        }
    }
}

async fn list_todays_goals(
    org: Option<&str>,
    date: Option<&str>,
    json: bool,
    client: &ApiClient,
) -> Result<()> {
    let org_id = resolve_org_id(org)?;
    let resp: ApiResponse<TodaysGoalsData> = client.get_todays_goals(&org_id, date, None).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp.data)?);
        return Ok(());
    }

    if resp.data.nodes.is_empty() {
        println!("No goals for today.");
        return Ok(());
    }

    for node in &resp.data.nodes {
        let indent = "  ".repeat(node.depth.max(0) as usize);
        let check = if node.is_completed() { "[x]" } else { "[ ]" };
        let parsed = node.parsed_status();
        let (_, colored_status) = resolve_status(node.is_completed(), parsed.as_ref());
        println!(
            "{indent}{check} {} {}  {}",
            colored_status,
            node.title,
            node.id.dimmed()
        );
    }

    Ok(())
}

async fn update_and_report(
    id: &str,
    req: &UpdateGoalRequest,
    verb: &str,
    json: bool,
    client: &ApiClient,
) -> Result<()> {
    let resp: ApiResponse<Goal> = client.update_goal(id, req).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp.data)?);
    } else {
        let (label, _) = resolve_status(resp.data.is_completed, resp.data.status.as_ref());
        println!("{verb} today's todo: {} [{label}]", resp.data.title);
    }
    Ok(())
}

#[cfg(test)]
mod execution_calendar_tests {
    use super::{
        TodoActivityAction, TodoStatus, parse_chat_metadata, validate_planned_update_args,
        validate_todo_add_args,
    };

    #[test]
    fn todo_status_as_str_maps_to_backend_values() {
        assert_eq!(TodoStatus::None.as_str(), "NONE");
        assert_eq!(TodoStatus::InProgress.as_str(), "IN_PROGRESS");
        assert_eq!(TodoStatus::Completed.as_str(), "COMPLETED");
        assert_eq!(TodoStatus::Cancelled.as_str(), "CANCELLED");
    }

    #[test]
    fn todo_activity_action_as_str_maps_to_backend_values() {
        assert_eq!(TodoActivityAction::Engage.as_str(), "ENGAGE");
        assert_eq!(TodoActivityAction::Progress.as_str(), "PROGRESS");
        assert_eq!(TodoActivityAction::Complete.as_str(), "COMPLETE");
    }

    #[test]
    fn parse_chat_metadata_accepts_valid_json_object() {
        let value = parse_chat_metadata(r#"{"key":"value"}"#).unwrap();
        assert_eq!(value["key"], "value");
    }

    #[test]
    fn parse_chat_metadata_rejects_invalid_json() {
        let err = parse_chat_metadata("not json").unwrap_err();
        assert!(err.to_string().contains("--chat-metadata"));
    }

    #[test]
    fn validate_todo_add_args_requires_objective_id_or_title() {
        let err = validate_todo_add_args(None, None).unwrap_err();
        assert!(err.to_string().contains("--objective-id"));
    }

    #[test]
    fn validate_todo_add_args_accepts_objective_id_alone() {
        assert!(validate_todo_add_args(Some("obj-1"), None).is_ok());
    }

    #[test]
    fn validate_todo_add_args_accepts_title_alone() {
        assert!(validate_todo_add_args(None, Some("Buy milk")).is_ok());
    }

    #[test]
    fn validate_planned_update_args_rejects_recurrence_and_clear_together() {
        let err = validate_planned_update_args(Some("{}"), true).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn validate_planned_update_args_accepts_recurrence_alone() {
        assert!(validate_planned_update_args(Some("{}"), false).is_ok());
    }

    #[test]
    fn validate_planned_update_args_accepts_clear_alone() {
        assert!(validate_planned_update_args(None, true).is_ok());
    }
}
