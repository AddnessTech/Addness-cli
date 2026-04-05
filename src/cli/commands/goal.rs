use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::{
    ApiClient, ApiResponse, Comment, CreateGoalRequest, Deliverable, DeliverableType, Goal,
    GoalStatus, GoalTreeData, GoalTreeItem, UpdateGoalRequest,
};
use crate::cli::commands::org::resolve_org_id;
use crate::cli::output::{
    print_children_table, print_goals_table, print_search_results, resolve_status,
};

#[derive(Subcommand)]
pub enum GoalCommands {
    /// List goals in the organization tree
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Tree depth (default: 3)
        #[arg(long, default_value = "3")]
        depth: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a goal's details
    Get {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// With deliverables information
        #[arg(long)]
        with_deliverable: bool,
        /// With comments information
        #[arg(long)]
        with_comment: bool,
    },
    /// List children of a goal
    Children {
        /// Goal ID
        id: String,
        /// Max number of children to return (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,
        /// Offset for pagination
        #[arg(long, default_value = "0")]
        offset: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show subtree of a goal
    Tree {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get sibling goals with their deliverables and descriptions
    Siblings {
        /// Goal ID
        id: String,
        /// Max number of siblings to return (default: 50)
        #[arg(long, default_value = "50")]
        limit: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Search goals by keyword
    Search {
        /// Search query
        query: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a new goal
    Create {
        /// Goal title
        #[arg(long)]
        title: String,
        /// Parent goal ID (omit to create as root goal)
        #[arg(long)]
        parent: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a goal's status or title
    Update {
        /// Goal ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Status: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED
        #[arg(long)]
        status: Option<String>,
        /// Title
        #[arg(long)]
        title: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// GoalInfo はゴールとその関連情報を保持するための
/// 汎用的な構造体
/// 木構造を表現できる
#[derive(Debug, serde::Serialize)]
struct GoalNode {
    goal: Goal,
    deliverables: Option<Vec<Deliverable>>,
    comments: Option<Vec<Comment>>,
    children: Option<Vec<GoalChildNode>>,
}

#[derive(Debug, serde::Serialize)]
struct GoalChildNode {
    goal: GoalTreeItem,
    deliverables: Option<Vec<Deliverable>>,
    comments: Option<Vec<Comment>>,
    children: Option<Vec<Self>>,
}

impl GoalChildNode {
    fn build_children(
        parent_id: &str,
        children_map: &mut HashMap<String, Vec<GoalTreeItem>>,
        deliverables_map: &mut HashMap<String, Vec<Deliverable>>,
        comments_map: &mut HashMap<String, Vec<Comment>>,
    ) -> Vec<Self> {
        match children_map.remove(parent_id) {
            Some(goals) => goals
                .into_iter()
                .map(|goal| {
                    let children = Self::build_children(
                        &goal.id,
                        children_map,
                        deliverables_map,
                        comments_map,
                    );

                    let deliverables = deliverables_map.remove(&goal.id);
                    let comments = comments_map.remove(&goal.id);

                    Self {
                        goal,
                        deliverables,
                        comments,
                        children: Some(children),
                    }
                })
                .collect(),
            None => vec![],
        }
    }
}

impl GoalNode {
    fn build_forest(
        root_id: &str,
        goals: Vec<GoalTreeItem>,
        mut deliverables_map: HashMap<String, Vec<Deliverable>>,
        mut comments_map: HashMap<String, Vec<Comment>>,
    ) -> Vec<GoalChildNode> {
        let mut children_map: HashMap<String, Vec<GoalTreeItem>> = HashMap::new();

        for goal in goals {
            if let Some(parent_id) = goal.parent_id.clone() {
                children_map.entry(parent_id).or_default().push(goal);
            }
        }

        GoalChildNode::build_children(
            root_id,
            &mut children_map,
            &mut deliverables_map,
            &mut comments_map,
        )
    }

    fn build_tree(
        goal: Goal,
        deliverables: Option<Vec<Deliverable>>,
        comments: Option<Vec<Comment>>,
        children: Option<Vec<GoalTreeItem>>,
        children_deliverables: HashMap<String, Vec<Deliverable>>,
        children_comments: HashMap<String, Vec<Comment>>,
    ) -> Self {
        let children = children.map(|children| {
            Self::build_forest(&goal.id, children, children_deliverables, children_comments)
        });

        Self {
            goal,
            comments,
            deliverables,
            children,
        }
    }
}

use crate::api::GoalChildItem;
use colored::Colorize;
use std::collections::HashMap;

/// ゴール一覧+成果物マップをJSON出力
fn print_goals_with_deliverables_json(
    goals: &[GoalChildItem],
    deliverables_map: &HashMap<String, Vec<Deliverable>>,
) -> Result<()> {
    let output: Vec<serde_json::Value> = goals
        .iter()
        .map(|g| {
            let deliverables = deliverables_map.get(&g.id).cloned().unwrap_or_default();
            serde_json::json!({
                "id": g.id,
                "title": g.title,
                "description": g.description,
                "status": g.status,
                "is_completed": g.is_completed,
                "deliverables": deliverables.iter().map(|d| {
                    serde_json::json!({
                        "id": d.id,
                        "display_name": d.display_name,
                        "node_type": d.node_type,
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_context_table(
    children: &[GoalChildItem],
    deliverables_map: &std::collections::HashMap<String, Vec<Deliverable>>,
) {
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            println!();
        }
        let (_, colored_status) = resolve_status(child.is_completed, child.status.as_ref());
        println!(
            "{} {} [{}]",
            format!("{}.", i + 1).bold(),
            child.title.bold(),
            colored_status
        );
        println!("   {}: {}", "ID".dimmed(), child.id.dimmed());

        if let Some(desc) = &child.description
            && !desc.is_empty()
        {
            println!("   {}: {desc}", "完了条件".dimmed());
        }

        if let Some(deliverables) = deliverables_map.get(&child.id) {
            if deliverables.is_empty() {
                println!("   {}: {}", "成果物".dimmed(), "(なし)".dimmed());
            } else {
                println!("   {}:", "成果物".dimmed());
                for d in deliverables {
                    println!("     {} {}", d.node_type.as_icon(), d.display_name);
                }
            }
        } else {
            println!("   {}: {}", "成果物".dimmed(), "(取得失敗)".dimmed());
        }
    }
}

/// Parse CLI status into (is_completed, api_status) pair.
/// CLI status      → is_completed  | API status
/// NOT_STARTED     → false         | NONE
/// IN_PROGRESS     → false         | IN_PROGRESS
/// COMPLETED       → true          | (unchanged)
/// CANCELLED       → false         | CANCELLED
fn parse_status(status: &str) -> Result<(Option<bool>, Option<GoalStatus>)> {
    match status.to_uppercase().as_str() {
        "NOT_STARTED" => Ok((Some(false), Some(GoalStatus::None))),
        "IN_PROGRESS" => Ok((Some(false), Some(GoalStatus::InProgress))),
        "COMPLETED" => Ok((Some(true), Some(GoalStatus::None))),
        "CANCELLED" => Ok((Some(false), Some(GoalStatus::Cancelled))),
        _ => bail!(
            "Invalid status: '{status}'. Use one of: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED"
        ),
    }
}

pub async fn handle_goals(cmd: &GoalCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        GoalCommands::List { org, depth, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp: ApiResponse<GoalTreeData> = client.get_goal_tree(&org_id, *depth).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.items)?);
            } else {
                print_goals_table(&resp.data.items);
            }
            Ok(())
        }
        GoalCommands::Get {
            id,
            json,
            with_deliverable,
            with_comment,
        } => {
            // id で指定されたゴール自身の情報を取得
            let resp: ApiResponse<Goal> = client.get_goal(id).await?;
            let deliverables = if *with_deliverable {
                Some(client.get_goal_deliverables(id).await?.data.deliverables)
            } else {
                None
            };
            let comments = if *with_comment {
                Some(client.list_comments(id).await?.comments)
            } else {
                None
            };

            // サブツリーの情報を取得（階層の終わりまで）
            let subtree_resp: ApiResponse<GoalTreeData> = client.get_goal_subtree(id).await?;
            let subtree_items: Vec<GoalTreeItem> = subtree_resp
                .data
                .items
                .into_iter()
                .filter(|item| item.id != *id)
                .collect();

            let children_deliverables = client
                .get_deliverables_map(
                    &subtree_items
                        .iter()
                        .map(|g| g.id.as_str())
                        .collect::<Vec<_>>(),
                )
                .await;
            let children_comments = client
                .get_comments_map(
                    &subtree_items
                        .iter()
                        .map(|g| g.id.as_str())
                        .collect::<Vec<_>>(),
                )
                .await;

            // 階層構造を構成
            let goal_tree = GoalNode::build_tree(
                resp.data,
                deliverables,
                comments,
                Some(subtree_items),
                children_deliverables,
                children_comments,
            );

            // 出力
            if *json {
                let output = serde_json::json!(goal_tree);
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                goal_tree.print_goal_detail_tree();
            }

            Ok(())
        }
        GoalCommands::Children {
            id,
            limit,
            offset,
            json,
        } => {
            let resp = client.get_goal_children(id, *limit, *offset).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.children)?);
            } else {
                print_children_table(&resp.data.children);
            }
            Ok(())
        }
        GoalCommands::Tree { id, json } => {
            let resp: ApiResponse<GoalTreeData> = client.get_goal_subtree(id).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.items)?);
            } else {
                print_goals_table(&resp.data.items);
            }
            Ok(())
        }
        GoalCommands::Siblings { id, limit, json } => {
            // 1. 対象ゴールの詳細を取得して親IDを得る
            let goal_resp: ApiResponse<Goal> = client.get_goal(id).await?;
            let parent_id = match &goal_resp.data.parent_id {
                Some(pid) => pid.clone(),
                None => {
                    if *json {
                        println!("[]");
                    } else {
                        println!("This goal has no parent (root goal). No siblings.");
                    }
                    return Ok(());
                }
            };

            // 2. 親の子ゴール一覧を取得（自分自身を除外）
            let children_resp = client.get_goal_children(&parent_id, *limit, 0).await?;
            let siblings: Vec<_> = children_resp
                .data
                .children
                .into_iter()
                .filter(|c| c.id != *id)
                .collect();

            if siblings.is_empty() {
                if *json {
                    println!("[]");
                } else {
                    println!("No sibling goals found.");
                }
                return Ok(());
            }

            let deliverables_map = client
                .get_deliverables_map(&siblings.iter().map(|g| g.id.as_str()).collect::<Vec<_>>())
                .await;

            if *json {
                print_goals_with_deliverables_json(&siblings, &deliverables_map)?;
            } else {
                print_context_table(&siblings, &deliverables_map);
            }
            Ok(())
        }
        GoalCommands::Search { query, json } => {
            let resp = client.search_goals(query).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.items)?);
            } else {
                print_search_results(&resp.data.items);
            }
            Ok(())
        }
        GoalCommands::Create {
            title,
            parent,
            org,
            description,
            json,
        } => {
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
                println!("Created goal: {} ({})", resp.data.title, resp.data.id);
            }
            Ok(())
        }
        GoalCommands::Update {
            id,
            org,
            status,
            title,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;

            let (is_completed, goal_status) = if let Some(s) = status {
                parse_status(s)?
            } else {
                (None, None)
            };

            let mut req = UpdateGoalRequest {
                status: goal_status,
                is_completed,
                title: None,
                description: None,
            };

            if let Some(t) = title {
                req.title = Some(t.clone());
            }

            if req.status.is_none() && req.is_completed.is_none() && req.title.is_none() {
                bail!("Nothing to update. Specify --status or --title.");
            }

            let resp: ApiResponse<Goal> = client.update_goal(&org_id, id, &req).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                let (label, _) = resolve_status(resp.data.is_completed, resp.data.status.as_ref());
                println!("Updated goal: {} [{label}]", resp.data.title);
            }
            Ok(())
        }
    }
}

impl GoalChildNode {
    fn print_goal_detail_subtree(
        &self,
        current_depth: usize,
        with_deliverable: bool,
        with_comment: bool,
    ) {
        let indent = " ".repeat(current_depth * 4);
        let (_, colored_status) = resolve_status(self.goal.is_completed, self.goal.status.as_ref());
        println!(
            "{}└─ {} [{colored_status}]",
            &indent[3.min(indent.len())..],
            self.goal.title.bold()
        );
        println!("{indent}   {}: {}", "ID".dimmed(), self.goal.id.dimmed());

        // 成果物も表示するオプションが立っているとき
        if with_deliverable {
            if let Some(deliverables) = &self.deliverables {
                if deliverables.is_empty() {
                    println!("{indent}   {}: {}", "成果物".dimmed(), "(なし)".dimmed());
                } else {
                    println!("{indent}   {}:", "成果物".dimmed());
                    for d in deliverables {
                        println!(
                            "{indent}     - {} {}",
                            d.node_type.as_icon(),
                            d.display_name
                        );
                        match &d.node_type {
                            DeliverableType::Link => {
                                if let Some(link) = &d.link_url {
                                    println!("       {link}");
                                }
                            }
                            DeliverableType::Document => {
                                if let Some(content) = &d.content {
                                    let truncated = if content.chars().count() > 30 {
                                        let end = content
                                            .char_indices()
                                            .nth(27)
                                            .map(|(i, _)| i)
                                            .unwrap_or(content.len());
                                        &format!("{}...", &content[..end])
                                    } else {
                                        content
                                    };
                                    println!("       {truncated}");
                                }
                            }
                            DeliverableType::File | DeliverableType::Folder => {
                                // nothing to do
                            }
                        }
                    }
                }
            } else {
                println!(
                    "{indent}   {}: {}",
                    "成果物".dimmed(),
                    "(取得失敗)".dimmed()
                );
            }
        }

        if with_comment {
            if let Some(comments) = &self.comments {
                if comments.is_empty() {
                    println!("{indent}   {}: {}", "コメント".dimmed(), "(なし)".dimmed());
                } else {
                    println!("{indent}   {}:", "コメント".dimmed());
                    for comment in comments {
                        let content = comment.content.replace('\n', " ");
                        let truncated = if content.chars().count() > 30 {
                            let end = content
                                .char_indices()
                                .nth(27)
                                .map(|(i, _)| i)
                                .unwrap_or(content.len());
                            format!("{}...", &content[..end])
                        } else {
                            content
                        };

                        let author_name = if comment.author.is_ai_agent {
                            format!("{} (AI)", comment.author.name)
                        } else {
                            comment.author.name.clone()
                        };

                        let date = &comment.created_at[..10.min(comment.created_at.len())];

                        println!(
                            "{indent}     - \"{}\" {} {}",
                            truncated,
                            author_name.dimmed(),
                            date.dimmed(),
                        );
                    }
                }
            } else {
                println!(
                    "{indent}   {}: {}",
                    "コメント".dimmed(),
                    "(取得失敗)".dimmed()
                );
            }
        }

        if let Some(children) = &self.children {
            for c in children {
                c.print_goal_detail_subtree(current_depth + 1, with_deliverable, with_comment);
            }
        }
    }
}

impl GoalNode {
    fn print_goal_detail_tree(&self) {
        let (_, colored_status) = resolve_status(self.goal.is_completed, self.goal.status.as_ref());
        println!("{} [{colored_status}]", self.goal.title.bold());
        println!("   {}: {}", "ID".dimmed(), self.goal.id.dimmed());

        if let Some(desc) = &self.goal.description
            && !desc.is_empty()
        {
            println!("   {}: {desc}", "完了条件".dimmed());
        }
        if let Some(parent_id) = &self.goal.parent_id {
            println!("   {}: {}", "Parent".dimmed(), parent_id.dimmed());
        }
        if let Some(owner) = &self.goal.owner {
            println!("   {}: {}", "Owner".dimmed(), owner.name);
        }
        if let Some(due) = &self.goal.due_date {
            println!("   {}: {}", "Due".dimmed(), &due[..10.min(due.len())]);
        }
        if let Some(body) = &self.goal.body
            && !body.is_empty()
        {
            println!("\n   {}", "Body".dimmed());
            println!("{body}");
        }

        // 成果物も表示するオプションが立っているとき
        if let Some(deliverables) = &self.deliverables {
            if deliverables.is_empty() {
                println!("   {}: {}", "成果物".dimmed(), "(なし)".dimmed());
            } else {
                println!("   {}:", "成果物".dimmed());
                for d in deliverables {
                    println!("     - {} {}", d.node_type.as_icon(), d.display_name);
                    match &d.node_type {
                        DeliverableType::Link => {
                            if let Some(link) = &d.link_url {
                                println!("       {link}");
                            }
                        }
                        DeliverableType::Document => {
                            if let Some(content) = &d.content {
                                let truncated = if content.chars().count() > 30 {
                                    let end = content
                                        .char_indices()
                                        .nth(27)
                                        .map(|(i, _)| i)
                                        .unwrap_or(content.len());
                                    &format!("{}...", &content[..end])
                                } else {
                                    content
                                };
                                println!("       {truncated}");
                            }
                        }
                        DeliverableType::File | DeliverableType::Folder => {
                            // nothing to do
                        }
                    }
                }
            }
        }
        let with_deliverable = self.deliverables.is_some();

        // コメントも表示するオプションが立っているとき
        if let Some(comments) = &self.comments {
            if comments.is_empty() {
                println!("   {}: {}", "コメント".dimmed(), "(なし)".dimmed());
            } else {
                println!("   {}:", "コメント".dimmed());
                for comment in comments {
                    let content = comment.content.replace('\n', " ");
                    let truncated = if content.chars().count() > 30 {
                        let end = content
                            .char_indices()
                            .nth(27)
                            .map(|(i, _)| i)
                            .unwrap_or(content.len());
                        format!("{}...", &content[..end])
                    } else {
                        content
                    };

                    let author_name = if comment.author.is_ai_agent {
                        format!("{} (AI)", comment.author.name)
                    } else {
                        comment.author.name.clone()
                    };

                    let date = &comment.created_at[..10.min(comment.created_at.len())];

                    println!(
                        "     - \"{}\" {} {}",
                        truncated,
                        author_name.dimmed(),
                        date.dimmed(),
                    );
                }
            }
        }
        let with_comment = self.comments.is_some();

        // 子の階層を表示するオプションが立っているとき
        if let Some(children) = &self.children {
            for c in children {
                c.print_goal_detail_subtree(1, with_deliverable, with_comment);
            }
        }
    }
}
