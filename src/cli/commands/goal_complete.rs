use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use serde::Serialize;

use crate::api::{ApiClient, Goal, GoalChildrenData, UpdateGoalRequest};

const PAGE_SIZE: usize = 100;

#[derive(Args)]
pub struct CompleteArgs {
    /// Goal ID to complete (the root is included with --recursive)
    #[arg(value_parser = canonical_id)]
    id: String,
    /// Complete all unfinished descendants before completing their parents
    #[arg(long)]
    recursive: bool,
    /// Keep this unfinished branch and its ancestors open; repeat for multiple branches
    #[arg(long, requires = "recursive", value_name = "GOAL_ID", value_parser = canonical_id)]
    exclude: Vec<String>,
    /// List the completion plan without updating goals
    #[arg(long)]
    dry_run: bool,
    /// Output the plan, completed IDs and any partial failure as JSON
    #[arg(long)]
    json: bool,
}

impl CompleteArgs {
    pub(crate) fn outputs_json(&self) -> bool {
        self.json
    }
}

fn canonical_id(value: &str) -> std::result::Result<String, String> {
    uuid::Uuid::parse_str(value)
        .map(|id| id.to_string())
        .map_err(|_| "Goal ID must be a UUID".to_string())
}

#[derive(Clone, Debug, Serialize)]
struct Target {
    id: String,
    title: String,
    parent_id: Option<String>,
    depth: usize,
}

#[derive(Debug, Serialize)]
struct CompletionReport {
    root_id: String,
    recursive: bool,
    dry_run: bool,
    root_completed: bool,
    status: &'static str,
    planned: Vec<Target>,
    excluded: Vec<Target>,
    completed_ids: Vec<String>,
    already_completed_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failed_goal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl CompletionReport {
    fn new(args: &CompleteArgs) -> Self {
        Self {
            root_id: args.id.clone(),
            recursive: args.recursive,
            dry_run: args.dry_run,
            root_completed: false,
            status: "planning",
            planned: vec![],
            excluded: vec![],
            completed_ids: vec![],
            already_completed_ids: vec![],
            failed_goal_id: None,
            error: None,
        }
    }
}

trait CompletionApi {
    async fn goal(&self, id: &str) -> Result<Goal>;
    async fn children(&self, id: &str, limit: usize, offset: usize) -> Result<GoalChildrenData>;
    async fn complete(&self, id: &str) -> Result<Goal>;
}

impl CompletionApi for ApiClient {
    async fn goal(&self, id: &str) -> Result<Goal> {
        Ok(self.get_goal(id).await?.data)
    }

    async fn children(&self, id: &str, limit: usize, offset: usize) -> Result<GoalChildrenData> {
        Ok(self.get_goal_children(id, limit, offset).await?.data)
    }

    async fn complete(&self, id: &str) -> Result<Goal> {
        let request = UpdateGoalRequest {
            completed_at: Some(Some(chrono::Utc::now().to_rfc3339())),
            status: None,
            title: None,
            description: None,
            body: None,
            due_date: None,
        };
        Ok(self.update_goal(id, &request).await?.data)
    }
}

pub async fn handle(args: &CompleteArgs, client: &ApiClient) -> Result<()> {
    if !args.json {
        eprintln!("Collecting unfinished goals for {}...", args.id);
    }
    let mut report = CompletionReport::new(args);
    let result = execute(client, args, &mut report).await;
    if let Err(error) = &result {
        report.status = "failed";
        report.error = Some(format!("{error:#}"));
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    result
}

fn print_report(report: &CompletionReport) {
    if report.dry_run {
        println!("Completion plan for {} (children first):", report.root_id);
        for target in &report.planned {
            println!("  {} ({})", target.title, target.id);
        }
    }
    println!(
        "Planned: {}, completed: {}, already completed: {}, excluded (including ancestors): {}",
        report.planned.len(),
        report.completed_ids.len(),
        report.already_completed_ids.len(),
        report.excluded.len()
    );
    println!("Root completed: {}", report.root_completed);
    if report.error.is_some() {
        println!(
            "Stopped. Confirmed completed IDs: {}",
            report.completed_ids.join(", ")
        );
        println!("Run the same command again to continue; completed goals will be skipped.");
    }
}

async fn collect_plan(
    api: &impl CompletionApi,
    root: &Goal,
    recursive: bool,
) -> Result<Vec<Target>> {
    if root.is_completed {
        return Ok(vec![]);
    }
    let root_target = Target {
        id: root.id.clone(),
        title: root.title.clone(),
        parent_id: root.parent_id.clone(),
        depth: 0,
    };
    let mut targets = vec![root_target.clone()];
    let mut queue = if recursive { vec![root_target] } else { vec![] };
    let mut seen = HashSet::from([root.id.clone()]);
    while let Some(parent) = queue.pop() {
        let mut offset = 0;
        let mut expected_total = None;
        loop {
            let page = api
                .children(&parent.id, PAGE_SIZE, offset)
                .await
                .with_context(|| format!("Read children of {}", parent.id))?;
            let next = next_page(&page, offset, &mut expected_total)?;
            for child in page.children {
                ensure!(
                    child.parent_id.as_deref() == Some(&parent.id),
                    "Goal {} moved outside its listed parent; run again to refresh the plan",
                    child.id
                );
                ensure!(
                    seen.insert(child.id.clone()),
                    "Duplicate goal or cycle in hierarchy: {}",
                    child.id
                );
                if child.is_completed {
                    continue;
                }
                let target = Target {
                    id: child.id,
                    title: child.title,
                    parent_id: child.parent_id,
                    depth: parent.depth + 1,
                };
                // No depth limit, and no dependence on a hasChildren hint from an old snapshot.
                queue.push(target.clone());
                targets.push(target);
            }
            match next {
                Some(value) => offset = value,
                None => break,
            }
        }
    }
    targets.sort_by(|a, b| b.depth.cmp(&a.depth).then_with(|| a.id.cmp(&b.id)));
    Ok(targets)
}

fn next_page(
    page: &GoalChildrenData,
    offset: usize,
    total: &mut Option<i64>,
) -> Result<Option<usize>> {
    if let Some(pagination) = &page.pagination {
        ensure!(
            pagination.offset == offset as i64 && pagination.limit > 0 && pagination.total >= 0,
            "Invalid child pagination metadata"
        );
        ensure!(
            total.is_none_or(|value| value == pagination.total),
            "Child list changed during pagination; run again to refresh the plan"
        );
        *total = Some(pagination.total);
        let next = offset
            .checked_add(usize::try_from(pagination.limit)?)
            .context("Child pagination overflow")?;
        return Ok((next < usize::try_from(pagination.total)?).then_some(next));
    }
    // Older servers may omit pagination. An empty extra page is harmless.
    Ok((page.children.len() == PAGE_SIZE).then_some(offset + PAGE_SIZE))
}

fn apply_exclusions(
    targets: Vec<Target>,
    excludes: &[String],
) -> Result<(Vec<Target>, Vec<Target>)> {
    let by_id: HashMap<_, _> = targets
        .iter()
        .map(|target| (target.id.as_str(), target))
        .collect();
    for excluded in excludes {
        ensure!(
            by_id.contains_key(excluded.as_str()),
            "Excluded goal {excluded} is not an unfinished goal in this subtree"
        );
    }
    let mut protected = HashSet::new();
    for target in &targets {
        let mut path = vec![];
        let mut current = Some(target);
        while let Some(node) = current {
            path.push(node.id.as_str());
            current = if node.depth == 0 {
                None
            } else {
                node.parent_id
                    .as_deref()
                    .and_then(|id| by_id.get(id).copied())
            };
        }
        if path
            .iter()
            .any(|id| excludes.iter().any(|excluded| excluded == id))
        {
            protected.insert(target.id.clone());
        }
        if excludes.contains(&target.id) {
            protected.extend(path.into_iter().map(str::to_string));
        }
    }
    Ok(targets
        .into_iter()
        .partition(|target| !protected.contains(&target.id)))
}

async fn execute(
    api: &impl CompletionApi,
    args: &CompleteArgs,
    report: &mut CompletionReport,
) -> Result<()> {
    ensure!(
        !args.exclude.contains(&args.id),
        "Exclude a descendant branch, not the root goal"
    );
    let root = api.goal(&args.id).await?;
    ensure!(
        root.id == args.id,
        "Root goal response did not match the requested ID"
    );
    report.root_completed = root.is_completed;
    let targets = collect_plan(api, &root, args.recursive).await?;
    if root.is_completed {
        report.already_completed_ids.push(root.id.clone());
        report.status = if args.dry_run { "planned" } else { "completed" };
        return Ok(());
    }
    (report.planned, report.excluded) = apply_exclusions(targets, &args.exclude)?;
    report.status = "planned";
    if args.dry_run {
        return Ok(());
    }
    for target in &report.planned {
        report.failed_goal_id = Some(target.id.clone());
        let current = api.goal(&target.id).await?;
        ensure!(
            current.id == target.id
                && current.parent_id == target.parent_id
                && current.organization_id == root.organization_id,
            "Goal {} changed parent or organization; run again to refresh the plan",
            target.id
        );
        if current.is_completed {
            report.already_completed_ids.push(target.id.clone());
            continue;
        }
        let remaining = api.children(&target.id, 1, 0).await?;
        if remaining
            .pagination
            .as_ref()
            .is_some_and(|page| page.total > 0)
            || remaining.children.iter().any(|child| !child.is_completed)
        {
            bail!(
                "Goal {} still has unfinished children; run with --recursive or refresh the plan",
                target.id
            );
        }
        let updated = api.complete(&target.id).await?;
        ensure!(
            updated.id == target.id && updated.is_completed,
            "Completion was not confirmed for {}",
            target.id
        );
        let verified = api.goal(&target.id).await?;
        ensure!(
            verified.id == target.id && verified.is_completed,
            "Completion was not confirmed for {}",
            target.id
        );
        report.completed_ids.push(target.id.clone());
        if !args.json {
            eprintln!(
                "Completed {}/{}: {} ({})",
                report.completed_ids.len(),
                report.planned.len(),
                target.title,
                target.id
            );
        }
    }
    report.failed_goal_id = Some(root.id.clone());
    let verified = api.goal(&root.id).await?;
    ensure!(
        verified.id == root.id,
        "Root goal verification returned a different ID"
    );
    report.root_completed = verified.is_completed;
    ensure!(
        report.root_completed || report.excluded.iter().any(|target| target.id == root.id),
        "Root goal is still unfinished; run again to refresh the plan"
    );
    report.failed_goal_id = None;
    report.status = "completed";
    Ok(())
}

#[cfg(test)]
mod tests;
