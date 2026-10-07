use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::api::GoalChildItem;
use serde_json::{Value, json};

struct FakeApi {
    goals: RefCell<BTreeMap<String, Value>>,
    calls: RefCell<Vec<String>>,
    writes: RefCell<Vec<String>>,
    fail_on: RefCell<Option<String>>,
    move_on_read: RefCell<Option<String>>,
    add_child_after_write: RefCell<bool>,
    hidden_first_page: bool,
    changed_total: bool,
}

impl FakeApi {
    fn new(rows: &[(&str, Option<&str>)]) -> Self {
        Self {
            goals: RefCell::new(
                rows.iter()
                    .map(|(id, parent)| {
                        (
                            id.to_string(),
                            json!({"id": id, "title": id, "parentId": parent,
                    "isCompleted": false, "organizationId": "org", "orderNo": 1,
                    "hasChildren": false, "owner": null}),
                        )
                    })
                    .collect(),
            ),
            calls: RefCell::new(vec![]),
            writes: RefCell::new(vec![]),
            fail_on: RefCell::new(None),
            move_on_read: RefCell::new(None),
            add_child_after_write: RefCell::new(false),
            hidden_first_page: false,
            changed_total: false,
        }
    }

    fn unfinished_children(&self, parent: &str) -> Vec<Value> {
        self.goals
            .borrow()
            .values()
            .filter(|row| row["parentId"] == parent && row["isCompleted"] == false)
            .cloned()
            .collect()
    }

    fn done(&self, id: &str) -> bool {
        self.goals.borrow()[id]["isCompleted"] == true
    }
}

impl CompletionApi for FakeApi {
    async fn goal(&self, id: &str) -> Result<Goal> {
        self.calls.borrow_mut().push(format!("get:{id}"));
        if self.move_on_read.borrow().as_deref() == Some(id) {
            self.goals.borrow_mut().get_mut(id).unwrap()["parentId"] = json!("outside");
        }
        Ok(serde_json::from_value(self.goals.borrow()[id].clone())?)
    }

    async fn children(&self, id: &str, limit: usize, offset: usize) -> Result<GoalChildrenData> {
        self.calls
            .borrow_mut()
            .push(format!("children:{id}:{limit}:{offset}"));
        let all = self.unfinished_children(id);
        let total = all.len() + usize::from(self.changed_total && offset > 0);
        let rows: Vec<Value> = if self.hidden_first_page && offset == 0 {
            vec![]
        } else {
            all.into_iter().skip(offset).take(limit).collect()
        };
        Ok(serde_json::from_value(json!({"children": rows,
            "pagination": {"limit": limit, "offset": offset, "total": total}}))?)
    }

    async fn complete(&self, id: &str) -> Result<Goal> {
        self.calls.borrow_mut().push(format!("patch:{id}"));
        if self.fail_on.borrow().as_deref() == Some(id) {
            bail!("write rejected for {id}");
        }
        ensure!(
            self.unfinished_children(id).is_empty(),
            "unfinished children"
        );
        self.goals.borrow_mut().get_mut(id).unwrap()["isCompleted"] = json!(true);
        self.writes.borrow_mut().push(id.to_string());
        if *self.add_child_after_write.borrow() {
            *self.add_child_after_write.borrow_mut() = false;
            self.goals.borrow_mut().insert(
                "new-child".into(),
                json!({
                    "id": "new-child", "title": "new child", "parentId": "root",
                    "isCompleted": false, "organizationId": "org", "orderNo": 1,
                    "hasChildren": false, "owner": null
                }),
            );
        }
        self.goal(id).await
    }
}

fn args() -> CompleteArgs {
    CompleteArgs {
        id: "root".into(),
        recursive: true,
        exclude: vec![],
        dry_run: false,
        json: true,
    }
}

#[tokio::test]
async fn completes_deepest_children_first_without_trusting_has_children_hint() {
    let api = FakeApi::new(&[
        ("root", None),
        ("a", Some("root")),
        ("b", Some("a")),
        ("c", Some("b")),
        ("d", Some("c")),
        ("outside", None),
    ]);
    let args = args();
    let mut report = CompletionReport::new(&args);
    execute(&api, &args, &mut report).await.unwrap();
    assert_eq!(*api.writes.borrow(), ["d", "c", "b", "a", "root"]);
    assert!(report.root_completed);
    assert!(!api.done("outside"));
    let before = api.writes.borrow().clone();
    let mut again = CompletionReport::new(&args);
    execute(&api, &args, &mut again).await.unwrap();
    assert_eq!(*api.writes.borrow(), before);
    assert_eq!(again.already_completed_ids, ["root"]);
}

#[tokio::test]
async fn collects_every_page_before_mutations_shift_active_offsets() {
    let ids: Vec<_> = (0..205).map(|i| format!("child-{i:03}")).collect();
    let mut rows = vec![("root", None)];
    rows.extend(ids.iter().map(|id| (id.as_str(), Some("root"))));
    let api = FakeApi::new(&rows);
    let args = args();
    let mut report = CompletionReport::new(&args);
    execute(&api, &args, &mut report).await.unwrap();
    assert_eq!(report.completed_ids.len(), 206);
    assert_eq!(report.completed_ids.last().unwrap(), "root");
    let calls = api.calls.borrow();
    let last_page = calls
        .iter()
        .position(|call| call == "children:root:100:200")
        .unwrap();
    let first_write = calls
        .iter()
        .position(|call| call.starts_with("patch:"))
        .unwrap();
    assert!(last_page < first_write);
}

#[tokio::test]
async fn empty_filtered_page_does_not_hide_later_pages() {
    let ids: Vec<_> = (0..101).map(|i| format!("child-{i:03}")).collect();
    let mut rows = vec![("root", None)];
    rows.extend(ids.iter().map(|id| (id.as_str(), Some("root"))));
    let mut api = FakeApi::new(&rows);
    api.hidden_first_page = true;
    let root = api.goal("root").await.unwrap();
    let plan = collect_plan(&api, &root, true).await.unwrap();
    assert_eq!(
        plan.iter()
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>(),
        ["child-100", "root"]
    );
    assert!(api.writes.borrow().is_empty());
}

#[tokio::test]
async fn pagination_changes_stop_before_any_write() {
    let ids: Vec<_> = (0..101).map(|i| format!("child-{i:03}")).collect();
    let mut rows = vec![("root", None)];
    rows.extend(ids.iter().map(|id| (id.as_str(), Some("root"))));
    let mut api = FakeApi::new(&rows);
    api.changed_total = true;
    let args = args();
    let mut report = CompletionReport::new(&args);
    let error = execute(&api, &args, &mut report).await.unwrap_err();
    assert!(error.to_string().contains("changed during pagination"));
    assert!(api.writes.borrow().is_empty());
}

#[tokio::test]
async fn dry_run_and_exclusions_leave_feedback_and_ancestors_open() {
    let api = FakeApi::new(&[
        ("root", None),
        ("ready", Some("root")),
        ("mixed", Some("root")),
        ("feedback", Some("mixed")),
        ("feedback-child", Some("feedback")),
        ("ready-child", Some("mixed")),
    ]);
    let mut args = args();
    args.exclude.push("feedback".into());
    args.dry_run = true;
    let mut report = CompletionReport::new(&args);
    execute(&api, &args, &mut report).await.unwrap();
    assert!(api.writes.borrow().is_empty());
    assert_eq!(
        report
            .planned
            .iter()
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>(),
        ["ready-child", "ready"]
    );
    assert_eq!(report.status, "planned");
    args.dry_run = false;
    execute(&api, &args, &mut report).await.unwrap();
    assert_eq!(*api.writes.borrow(), ["ready-child", "ready"]);
    for id in ["root", "mixed", "feedback", "feedback-child"] {
        assert!(!api.done(id), "{id}");
    }
    assert!(!report.root_completed);
}

#[tokio::test]
async fn unrelated_exclusion_fails_before_any_write() {
    let api = FakeApi::new(&[("root", None), ("child", Some("root")), ("outside", None)]);
    let mut args = args();
    args.exclude.push("outside".into());
    let mut report = CompletionReport::new(&args);
    assert!(execute(&api, &args, &mut report).await.is_err());
    assert!(api.writes.borrow().is_empty());
}

#[tokio::test]
async fn failure_reports_partial_progress_and_same_command_resumes() {
    let api = FakeApi::new(&[("root", None), ("a", Some("root")), ("b", Some("root"))]);
    *api.fail_on.borrow_mut() = Some("b".into());
    let args = args();
    let mut report = CompletionReport::new(&args);
    assert!(execute(&api, &args, &mut report).await.is_err());
    assert_eq!(report.completed_ids, ["a"]);
    assert_eq!(report.failed_goal_id.as_deref(), Some("b"));
    assert!(!api.done("root"));
    *api.fail_on.borrow_mut() = None;
    let mut resumed = CompletionReport::new(&args);
    execute(&api, &args, &mut resumed).await.unwrap();
    assert_eq!(resumed.completed_ids, ["b", "root"]);
    assert_eq!(*api.writes.borrow(), ["a", "b", "root"]);
}

#[tokio::test]
async fn moved_goal_is_not_completed() {
    let api = FakeApi::new(&[("root", None), ("child", Some("root"))]);
    *api.move_on_read.borrow_mut() = Some("child".into());
    let args = args();
    let mut report = CompletionReport::new(&args);
    let error = execute(&api, &args, &mut report).await.unwrap_err();
    assert!(error.to_string().contains("changed parent"));
    assert!(api.writes.borrow().is_empty());
}

#[tokio::test]
async fn new_child_stops_parent_completion() {
    let api = FakeApi::new(&[("root", None), ("child", Some("root"))]);
    *api.add_child_after_write.borrow_mut() = true;
    let args = args();
    let mut report = CompletionReport::new(&args);
    let error = execute(&api, &args, &mut report).await.unwrap_err();
    assert!(error.to_string().contains("unfinished children"));
    assert_eq!(report.completed_ids, ["child"]);
    assert!(!api.done("root"));
}

#[tokio::test]
async fn single_goal_completion_does_not_implicitly_complete_children() {
    let api = FakeApi::new(&[("root", None), ("child", Some("root"))]);
    let mut args = args();
    args.recursive = false;
    let mut report = CompletionReport::new(&args);
    assert!(execute(&api, &args, &mut report).await.is_err());
    assert!(api.writes.borrow().is_empty());
}

#[test]
fn pagination_without_metadata_and_invalid_metadata() {
    let child: GoalChildItem = serde_json::from_value(json!({"id": "a", "title": "a",
        "parentId": "root", "isCompleted": false, "orderNo": 1, "owner": null}))
    .unwrap();
    let mut page = GoalChildrenData {
        children: vec![child],
        pagination: None,
    };
    assert_eq!(next_page(&page, 0, &mut None).unwrap(), None);
    page.pagination =
        Some(serde_json::from_value(json!({"offset": 0, "limit": 0, "total": 1})).unwrap());
    assert!(next_page(&page, 0, &mut None).is_err());
}

#[test]
fn clap_requires_recursive_for_exclusions_and_accepts_preview() {
    use clap::Parser;
    let root = "11111111-1111-4111-8111-111111111111";
    let branch = "22222222-2222-4222-8222-222222222222";
    assert!(
        crate::Cli::try_parse_from([
            "addness",
            "goal",
            "complete",
            root,
            "--recursive",
            "--exclude",
            branch,
            "--dry-run",
            "--json"
        ])
        .is_ok()
    );
    assert!(
        crate::Cli::try_parse_from(["addness", "goal", "complete", root, "--exclude", branch])
            .is_err()
    );
    assert!(crate::Cli::try_parse_from(["addness", "goal", "complete", "invalid"]).is_err());
}
