use anyhow::Result;
use chrono::Utc;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
};
use std::{collections::HashMap, path::PathBuf};
use tokio::runtime::Handle;

use crate::api::{
    ApiClient, CreateGoalRequest, DeliverableType, GoalStatus, Member, MemberId, Organization,
    UpdateGoalRequest,
};
use crate::dbg_log;

use super::goal_tree::{GoalTree, TreeRow};
use super::ui;

#[derive(PartialEq, Eq)]
pub enum ActivePane {
    OrgSelector,
    Navigation,
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormField {
    Title,
    Description,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliverableKind {
    File,
    Document,
    Link,
    Folder,
}

impl DeliverableKind {
    fn next(&self) -> Self {
        match self {
            DeliverableKind::File => DeliverableKind::Document,
            DeliverableKind::Document => DeliverableKind::Link,
            DeliverableKind::Link => DeliverableKind::Folder,
            DeliverableKind::Folder => DeliverableKind::File,
        }
    }

    fn prev(&self) -> Self {
        match self {
            DeliverableKind::File => DeliverableKind::Folder,
            DeliverableKind::Document => DeliverableKind::File,
            DeliverableKind::Link => DeliverableKind::Document,
            DeliverableKind::Folder => DeliverableKind::Link,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            DeliverableKind::File => "file",
            DeliverableKind::Document => "document",
            DeliverableKind::Link => "link",
            DeliverableKind::Folder => "folder",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliverableFormField {
    Kind,
    Name,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionMenuItem {
    AddDeliverable,
    AddComment,
    EditGoal,
    DeleteGoal,
    UpdateDeliverable,
    RenameDeliverable,
    MoveDeliverable,
    DeleteDeliverable,
    ReplyComment,
    ResolveComment,
    UnresolveComment,
    EditComment,
    DeleteComment,
    ReactComment,
}

impl ActionMenuItem {
    pub fn label(&self) -> &'static str {
        match self {
            ActionMenuItem::AddDeliverable => "add deliverable",
            ActionMenuItem::AddComment => "add comment",
            ActionMenuItem::EditGoal => "edit goal",
            ActionMenuItem::DeleteGoal => "delete goal",
            ActionMenuItem::UpdateDeliverable => "update document",
            ActionMenuItem::RenameDeliverable => "rename deliverable",
            ActionMenuItem::MoveDeliverable => "move deliverable",
            ActionMenuItem::DeleteDeliverable => "delete deliverable",
            ActionMenuItem::ReplyComment => "reply to comment",
            ActionMenuItem::ResolveComment => "resolve comment",
            ActionMenuItem::UnresolveComment => "unresolve comment",
            ActionMenuItem::EditComment => "edit comment",
            ActionMenuItem::DeleteComment => "delete comment",
            ActionMenuItem::ReactComment => "react (👍)",
        }
    }
}

/// Display status combining GoalStatus and completed_at
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalDisplayStatus {
    NotStarted, // GoalStatus::None + not completed
    InProgress, // GoalStatus::InProgress + not completed
    Cancelled,  // GoalStatus::Cancelled + not completed
    Completed,  // completed_at.is_some()
}

impl GoalDisplayStatus {
    /// Get allowed transitions from current status
    pub fn allowed_transitions(&self) -> Vec<GoalDisplayStatus> {
        match self {
            GoalDisplayStatus::NotStarted => vec![
                GoalDisplayStatus::InProgress,
                GoalDisplayStatus::Cancelled,
                GoalDisplayStatus::Completed,
            ],
            GoalDisplayStatus::InProgress => vec![
                GoalDisplayStatus::NotStarted,
                GoalDisplayStatus::Cancelled,
                GoalDisplayStatus::Completed,
            ],
            GoalDisplayStatus::Cancelled => vec![
                GoalDisplayStatus::NotStarted,
                GoalDisplayStatus::InProgress,
                GoalDisplayStatus::Completed,
            ],
            GoalDisplayStatus::Completed => vec![
                // Completed is final state - no transitions allowed
            ],
        }
    }

    pub fn to_emoji_string(&self) -> String {
        match self {
            GoalDisplayStatus::NotStarted => "🔵 未着手".to_string(),
            GoalDisplayStatus::InProgress => "⏩ 進行中".to_string(),
            GoalDisplayStatus::Cancelled => "⏸ 停止中".to_string(),
            GoalDisplayStatus::Completed => "✅ 完了".to_string(),
        }
    }

    /// Convert to GoalStatus for API (Completed is handled separately via completed_at)
    pub fn to_goal_status(&self) -> GoalStatus {
        match self {
            GoalDisplayStatus::NotStarted => GoalStatus::None,
            GoalDisplayStatus::InProgress => GoalStatus::InProgress,
            GoalDisplayStatus::Cancelled => GoalStatus::Cancelled,
            GoalDisplayStatus::Completed => GoalStatus::None, // Will set completed_at instead
        }
    }

    /// Create from GoalStatus and is_completed flag
    pub fn from_goal_state(status: Option<&GoalStatus>, is_completed: bool) -> Self {
        if is_completed {
            return GoalDisplayStatus::Completed;
        }
        match status {
            Some(GoalStatus::InProgress) => GoalDisplayStatus::InProgress,
            Some(GoalStatus::Cancelled) => GoalDisplayStatus::Cancelled,
            _ => GoalDisplayStatus::NotStarted,
        }
    }
}

/// コメント本文を一覧表示・タイトル用に短く切り詰める（改行は空白化）。
fn truncate_comment(content: &str) -> String {
    let oneline = content.replace(['\n', '\r'], " ");
    let trimmed = oneline.trim();
    let max = 40;
    if trimmed.chars().count() > max {
        let head: String = trimmed.chars().take(max).collect();
        format!("{head}…")
    } else {
        trimmed.to_string()
    }
}

// NOTE: 将来goal以外についてのモーダルが出得るためsuffixにGoalと付けている
// それまでclippy errorを抑制
#[allow(clippy::enum_variant_names)]
pub enum ModalState {
    ActionMenu {
        title: String,
        items: Vec<ActionMenuItem>,
        selected_index: usize,
    },
    CreateGoal {
        title: String,
        description: String,
        parent_goal_id: Option<String>,
        parent_goal_title: Option<String>,
        current_field: FormField,
    },
    EditGoal {
        goal_id: String,
        title: String,
        description: String,
        current_status: GoalDisplayStatus,
        selected_status_index: usize,
        allowed_statuses: Vec<GoalDisplayStatus>,
        current_field: FormField,
    },
    DeleteGoal {
        goal_id: String,
        goal_title: String,
        confirm_index: usize, // 0 = Cancel (default), 1 = Delete
    },
    AddDeliverable {
        goal_id: String,
        goal_title: String,
        kind: DeliverableKind,
        name: String,
        value: String,
        current_field: DeliverableFormField,
    },
    UpdateDeliverable {
        goal_id: String,
        deliverable_id: String,
        deliverable_name: String,
        content_file: String,
    },
    RenameDeliverable {
        goal_id: String,
        deliverable_id: String,
        current_name: String,
        name: String,
    },
    MoveDeliverable {
        goal_id: String,
        deliverable_id: String,
        deliverable_name: String,
        targets: Vec<(Option<String>, String)>,
        selected_index: usize,
    },
    DeleteDeliverable {
        goal_id: String,
        deliverable_id: String,
        deliverable_name: String,
        confirm_index: usize,
    },
    AddComment {
        goal_id: String,
        goal_title: String,
        body: String,
    },
    ReplyComment {
        goal_id: String,
        parent_comment_id: String,
        parent_excerpt: String,
        body: String,
    },
    EditComment {
        goal_id: String,
        comment_id: String,
        body: String,
    },
    DeleteComment {
        goal_id: String,
        comment_id: String,
        excerpt: String,
        confirm_index: usize,
    },
    ReactComment {
        goal_id: String,
        comment_id: String,
        emojis: Vec<&'static str>,
        selected_index: usize,
    },
}

pub struct App {
    pub(super) client: ApiClient,
    rt: Handle,
    pub running: bool,
    pub active_pane: ActivePane,
    pub sidebar_index: usize,
    pub sidebar_items: Vec<&'static str>,

    // Organization state
    pub orgs: Vec<Organization>,
    pub current_org_index: usize,
    pub show_org_popup: bool,
    pub org_popup_index: usize,

    // Goal trees
    pub goal_tree: GoalTree,
    pub todays_goals_tree: GoalTree,
    /// Execution(todays) ツリーは初回表示時に遅延ロードする。起動を軽くするため。
    todays_loaded: bool,

    // Members state
    pub members: HashMap<MemberId, Member>,
    pub members_list: Vec<Member>, // For display in Members tab
    pub members_cursor: usize,
    pub members_scroll_offset: usize,

    /// Last known content viewport height (for scroll calculations)
    pub content_height: usize,

    /// Error message to display in status bar (cleared on next key press)
    pub error_message: Option<String>,

    /// Modal state for create/edit goal dialogs
    pub modal_state: Option<ModalState>,

    /// Success message to display in status bar (cleared on next key press)
    pub success_message: Option<String>,
}

impl App {
    pub fn new(client: ApiClient, rt: Handle) -> Self {
        Self {
            client,
            rt,
            running: true,
            active_pane: ActivePane::Navigation,
            sidebar_index: 0,
            sidebar_items: vec!["Goals", "Execution", "Members"],
            orgs: vec![],
            current_org_index: 0,
            show_org_popup: false,
            org_popup_index: 0,
            goal_tree: GoalTree::empty(),
            todays_goals_tree: GoalTree::empty(),
            todays_loaded: false,
            members: HashMap::new(),
            members_list: vec![],
            members_cursor: 0,
            members_scroll_offset: 0,
            content_height: 0,
            error_message: None,
            modal_state: None,
            success_message: None,
        }
    }

    pub fn current_org_name(&self) -> &str {
        self.orgs
            .get(self.current_org_index)
            .map(|o| o.name.as_str())
            .unwrap_or("(no org)")
    }

    pub fn current_org_id(&self) -> Option<&str> {
        self.orgs.get(self.current_org_index).map(|o| o.id.as_str())
    }

    /// Get reference to the currently active goal tree based on sidebar selection
    pub fn active_goal_tree(&self) -> &GoalTree {
        match self.sidebar_index {
            0 => &self.goal_tree,
            1 => &self.todays_goals_tree,
            _ => &self.goal_tree,
        }
    }

    /// Get mutable reference to the currently active goal tree based on sidebar selection
    pub fn active_goal_tree_mut(&mut self) -> &mut GoalTree {
        match self.sidebar_index {
            0 => &mut self.goal_tree,
            1 => &mut self.todays_goals_tree,
            _ => &mut self.goal_tree,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        self.load_initial_data();

        while self.running {
            // 表示しようとしているタブのデータを必要になった時点で取得する。
            self.ensure_active_tab_loaded();
            terminal.draw(|frame| ui::draw(frame, self))?;
            self.handle_events()?;
        }
        Ok(())
    }

    /// アクティブなタブが未ロードなら取得する（遅延ロード）。
    /// Execution(todays) は初回表示までロードしないことで起動を軽くする。
    fn ensure_active_tab_loaded(&mut self) {
        if self.sidebar_index == 1 && !self.todays_loaded {
            self.load_todays_goals();
        }
    }

    // -----------------------------------------------------------------------
    // API helpers
    // -----------------------------------------------------------------------

    fn api_call<F, T>(&self, future: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        self.rt.block_on(future)
    }

    fn load_initial_data(&mut self) {
        // Fetch organizations
        match self.api_call(self.client.list_organizations()) {
            Ok(resp) => {
                self.orgs = resp.data.organizations;
                if !self.orgs.is_empty() {
                    self.current_org_index = 0;
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load organizations: {e}"));
                return;
            }
        }

        // Goals タブのみ起動時にロードし、Execution(todays) は初回表示まで遅延する。
        self.load_goal_tree();
        self.load_members();
    }

    fn load_goal_tree(&mut self) {
        let Some(org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.goal_tree = GoalTree::empty();
            return;
        };

        self.client.set_org_id(Some(org_id.clone()));

        match self.api_call(self.client.get_goal_tree(&org_id, 2)) {
            Ok(resp) => {
                self.goal_tree = GoalTree::from_tree_items(resp.data.items);

                // Load comments and deliverables for auto-expanded root goals
                self.load_root_goal_details();
            }
            Err(e) => {
                self.goal_tree = GoalTree::empty();
                self.error_message = Some(format!("Failed to load goals: {e}"));
            }
        }
    }

    fn load_root_goal_details(&mut self) {
        // Collect root goal IDs from the flattened tree
        let root_goal_ids: Vec<String> = {
            let rows = self.goal_tree.flatten();
            rows.iter()
                .filter_map(|row| {
                    if let TreeRow::Goal { goal_id, depth, .. } = row {
                        if *depth == 0 {
                            Some(goal_id.to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        };

        if root_goal_ids.is_empty() {
            return;
        }

        // 全ルートゴールのコメント・成果物を並列取得する（逐次 N 往復 → 概ね 1 往復）。
        let client = &self.client;
        let result = self.api_call(async {
            let id_refs: Vec<&str> = root_goal_ids.iter().map(String::as_str).collect();
            let maps = tokio::join!(
                client.get_comments_map(&id_refs),
                client.get_deliverables_map(&id_refs),
            );
            Ok::<_, anyhow::Error>(maps)
        });

        if let Ok((comments_map, deliverables_map)) = result {
            for (id, comments) in comments_map {
                self.goal_tree.set_comments_for_goal_id(&id, comments);
            }
            for (id, deliverables) in deliverables_map {
                self.goal_tree
                    .set_deliverables_for_goal_id(&id, deliverables);
            }
        }
    }

    fn load_todays_goals(&mut self) {
        // 一度試みたら（成否に関わらず）再取得しないようフラグを立てる。
        self.todays_loaded = true;
        let Some(org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.todays_goals_tree = GoalTree::empty();
            return;
        };

        dbg_log!("=== load_todays_goals ===");
        self.client.set_org_id(Some(org_id.clone()));

        match self.api_call(self.client.get_todays_goals(&org_id, None, None)) {
            Ok(resp) => {
                dbg_log!("Total nodes loaded: {}", resp.data.nodes.len());
                dbg_log!("Auto-generated count: {}", resp.data.auto_generated_count);

                for node in &resp.data.nodes {
                    dbg_log!(
                        "  Goal: {} | id: {} | depth: {} | is_direct: {} | owner: {}",
                        node.title,
                        node.id,
                        node.depth,
                        node.is_direct_assignment,
                        node.owner
                            .as_ref()
                            .map(|o| o.name.as_str())
                            .unwrap_or("None")
                    );
                }

                self.todays_goals_tree = GoalTree::from_todays_goal_nodes(resp.data.nodes);

                dbg_log!(
                    "Today's goals tree has {} roots",
                    self.todays_goals_tree.roots.len()
                );
            }
            Err(e) => {
                self.todays_goals_tree = GoalTree::empty();
                self.error_message = Some(format!("Failed to load today's goals: {e}"));
            }
        }
    }

    fn load_members(&mut self) {
        let Some(org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.members = HashMap::new();
            self.members_list = vec![];
            return;
        };

        match self.api_call(self.client.get_members(&org_id)) {
            Ok(resp) => {
                let member_list = resp.data.members;

                // Build HashMap for fast lookup
                self.members = member_list
                    .iter()
                    .map(|m| (MemberId::new(m.id.clone()), m.clone()))
                    .collect();

                // Keep list for display
                self.members_list = member_list;
                self.members_cursor = 0;
                self.members_scroll_offset = 0;
            }
            Err(e) => {
                self.members = HashMap::new();
                self.members_list = vec![];
                self.error_message = Some(format!("Failed to load members: {e}"));
            }
        }
    }

    fn members_cursor_up(&mut self) {
        if self.members_cursor > 0 {
            self.members_cursor -= 1;
        }
    }

    fn members_cursor_down(&mut self) {
        if self.members_cursor + 1 < self.members_list.len() {
            self.members_cursor += 1;
        }
    }

    pub(super) fn adjust_members_scroll(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.members_cursor < self.members_scroll_offset {
            self.members_scroll_offset = self.members_cursor;
        } else if self.members_cursor >= self.members_scroll_offset + viewport_height {
            self.members_scroll_offset = self.members_cursor - viewport_height + 1;
        }
    }

    fn selected_goal_context(&self) -> Option<(String, String)> {
        let tree = self.active_goal_tree();
        let rows = tree.flatten();

        // 成果物行を選択していても、その成果物が属するゴール行まで遡って
        // 実際のタイトルを使う（IDではなくタイトルで提示する方針）。
        for row in rows.iter().take(tree.cursor + 1).rev() {
            if let TreeRow::Goal { goal_id, title, .. } = row {
                return Some((goal_id.to_string(), title.to_string()));
            }
        }
        None
    }

    fn selected_deliverable_context(
        &self,
    ) -> Option<(String, String, String, String, DeliverableType)> {
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        match rows.get(tree.cursor) {
            Some(TreeRow::DeliverableItem { deliverable, .. }) => Some((
                deliverable.objective_id.clone(),
                deliverable.id.clone(),
                deliverable.display_name.clone(),
                deliverable
                    .parent_deliverable_id
                    .clone()
                    .unwrap_or_default(),
                deliverable.node_type.clone(),
            )),
            _ => None,
        }
    }

    fn selected_goal_row_context(&self) -> Option<(String, String)> {
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        match rows.get(tree.cursor) {
            Some(TreeRow::Goal { goal_id, title, .. }) => {
                Some((goal_id.to_string(), title.to_string()))
            }
            _ => None,
        }
    }

    /// カーソル上のコメント行の情報。返り値は
    /// (goal_id, comment_id, 本文, 解決済みか)。
    /// goal_id はコメントの commentable_id（= ゴールID）から得る。
    fn selected_comment_context(&self) -> Option<(String, String, String, bool)> {
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        match rows.get(tree.cursor) {
            Some(TreeRow::CommentItem { comment, .. }) => Some((
                comment.commentable_id.clone(),
                comment.id.clone(),
                comment.content.clone(),
                comment.resolved_at.is_some(),
            )),
            _ => None,
        }
    }

    fn deliverable_folder_targets_for_goal(
        &self,
        goal_id: &str,
        current_deliverable_id: &str,
    ) -> Vec<(Option<String>, String)> {
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        let mut targets = vec![(None, "(root)".to_string())];

        for row in rows {
            if let TreeRow::DeliverableItem { deliverable, .. } = row
                && deliverable.objective_id == goal_id
                && deliverable.id != current_deliverable_id
                && deliverable.node_type == DeliverableType::Folder
            {
                targets.push((
                    Some(deliverable.id.clone()),
                    format!("{} ({})", deliverable.display_name, deliverable.id),
                ));
            }
        }

        targets
    }

    fn start_action_menu(&mut self) {
        // コメント行が最も具体的なので先に判定する。
        if let Some((_, _, content, resolved)) = self.selected_comment_context() {
            let mut items = vec![ActionMenuItem::ReplyComment];
            if resolved {
                items.push(ActionMenuItem::UnresolveComment);
            } else {
                items.push(ActionMenuItem::ResolveComment);
            }
            items.push(ActionMenuItem::ReactComment);
            items.push(ActionMenuItem::EditComment);
            items.push(ActionMenuItem::DeleteComment);
            self.modal_state = Some(ModalState::ActionMenu {
                title: format!("comment: {}", truncate_comment(&content)),
                items,
                selected_index: 0,
            });
            return;
        }

        if let Some((_, deliverable_id, deliverable_name, _, node_type)) =
            self.selected_deliverable_context()
        {
            let mut items = Vec::new();
            if node_type == DeliverableType::Document {
                items.push(ActionMenuItem::UpdateDeliverable);
            }
            items.push(ActionMenuItem::RenameDeliverable);
            items.push(ActionMenuItem::MoveDeliverable);
            items.push(ActionMenuItem::DeleteDeliverable);
            self.modal_state = Some(ModalState::ActionMenu {
                title: format!("{deliverable_name} ({deliverable_id})"),
                items,
                selected_index: 0,
            });
            return;
        }

        if let Some((_, goal_title)) = self.selected_goal_row_context() {
            self.modal_state = Some(ModalState::ActionMenu {
                title: goal_title,
                items: vec![
                    ActionMenuItem::AddComment,
                    ActionMenuItem::AddDeliverable,
                    ActionMenuItem::EditGoal,
                    ActionMenuItem::DeleteGoal,
                ],
                selected_index: 0,
            });
            return;
        }

        self.error_message =
            Some("Select a goal, deliverable or comment to open actions".to_string());
    }

    fn reload_deliverables_for_goal(&mut self, goal_id: &str) {
        match self.api_call(self.client.get_goal_deliverables(goal_id)) {
            Ok(resp) => {
                let deliverables = resp.data.deliverables;
                self.goal_tree
                    .set_deliverables_for_goal_id(goal_id, deliverables.clone());
                self.todays_goals_tree
                    .set_deliverables_for_goal_id(goal_id, deliverables);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to reload deliverables: {e}"));
            }
        }
    }

    fn reload_comments_for_goal(&mut self, goal_id: &str) {
        match self.api_call(self.client.list_comments(goal_id)) {
            Ok(resp) => {
                let comments = resp.comments;
                self.goal_tree
                    .set_comments_for_goal_id(goal_id, comments.clone());
                self.todays_goals_tree
                    .set_comments_for_goal_id(goal_id, comments);
                // 削除・解決で行数が減るとカーソルが範囲外/別ノードを指すため再クランプ。
                self.goal_tree.clamp_cursor();
                self.todays_goals_tree.clamp_cursor();
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to reload comments: {e}"));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Comment actions
    // -----------------------------------------------------------------------

    fn start_add_comment_modal(&mut self) {
        let Some((goal_id, goal_title)) = self.selected_goal_context() else {
            self.error_message = Some("Please select a goal to add a comment".to_string());
            return;
        };
        self.modal_state = Some(ModalState::AddComment {
            goal_id,
            goal_title,
            body: String::new(),
        });
    }

    fn start_reply_comment_modal(&mut self) {
        let Some((goal_id, comment_id, content, _)) = self.selected_comment_context() else {
            self.error_message = Some("Please select a comment to reply to".to_string());
            return;
        };
        self.modal_state = Some(ModalState::ReplyComment {
            goal_id,
            parent_comment_id: comment_id,
            parent_excerpt: truncate_comment(&content),
            body: String::new(),
        });
    }

    fn start_edit_comment_modal(&mut self) {
        let Some((goal_id, comment_id, content, _)) = self.selected_comment_context() else {
            self.error_message = Some("Please select a comment to edit".to_string());
            return;
        };
        self.modal_state = Some(ModalState::EditComment {
            goal_id,
            comment_id,
            body: content,
        });
    }

    fn start_delete_comment_modal(&mut self) {
        let Some((goal_id, comment_id, content, _)) = self.selected_comment_context() else {
            self.error_message = Some("Please select a comment to delete".to_string());
            return;
        };
        self.modal_state = Some(ModalState::DeleteComment {
            goal_id,
            comment_id,
            excerpt: truncate_comment(&content),
            confirm_index: 0,
        });
    }

    fn start_react_comment_modal(&mut self) {
        let Some((goal_id, comment_id, _, _)) = self.selected_comment_context() else {
            self.error_message = Some("Please select a comment to react to".to_string());
            return;
        };
        self.modal_state = Some(ModalState::ReactComment {
            goal_id,
            comment_id,
            emojis: vec!["👍", "❤️", "🎉", "👀", "🙏", "🚀"],
            selected_index: 0,
        });
    }

    fn do_set_comment_resolved(&mut self, resolved: bool) {
        let Some((goal_id, comment_id, _, _)) = self.selected_comment_context() else {
            self.error_message = Some("Please select a comment".to_string());
            return;
        };
        let result = if resolved {
            self.api_call(self.client.resolve_comment(&comment_id))
                .map(|_| ())
        } else {
            self.api_call(self.client.unresolve_comment(&comment_id))
                .map(|_| ())
        };
        match result {
            Ok(()) => {
                self.success_message = Some(
                    if resolved {
                        "Comment resolved"
                    } else {
                        "Comment unresolved"
                    }
                    .to_string(),
                );
                self.reload_comments_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to update comment: {e}"));
            }
        }
    }

    fn modal_submit_add_comment(
        &mut self,
        goal_id: String,
        parent_comment_id: Option<String>,
        body: String,
    ) {
        let body = body.trim();
        if body.is_empty() {
            self.error_message = Some("Comment body is required".to_string());
            return;
        }
        // AIではなく人間の操作だが、CLI経由と区別できるよう本文はそのまま送る。
        let result = self.api_call(self.client.create_comment_with_options(
            &goal_id,
            body,
            parent_comment_id,
            Vec::new(),
        ));
        match result {
            Ok(_) => {
                self.success_message = Some("Comment posted".to_string());
                self.reload_comments_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to post comment: {e}"));
            }
        }
    }

    fn modal_submit_edit_comment(&mut self, goal_id: String, comment_id: String, body: String) {
        let body = body.trim();
        if body.is_empty() {
            self.error_message = Some("Comment body is required".to_string());
            return;
        }
        match self.api_call(self.client.update_comment(&comment_id, body, Vec::new())) {
            Ok(_) => {
                self.success_message = Some("Comment updated".to_string());
                self.reload_comments_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to update comment: {e}"));
            }
        }
    }

    fn modal_submit_delete_comment(&mut self, goal_id: String, comment_id: String) {
        match self.api_call(self.client.delete_comment(&comment_id)) {
            Ok(()) => {
                self.success_message = Some("Comment deleted".to_string());
                self.reload_comments_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to delete comment: {e}"));
            }
        }
    }

    fn modal_submit_react_comment(&mut self, goal_id: String, comment_id: String, emoji: &str) {
        match self.api_call(self.client.add_reaction(&comment_id, emoji)) {
            Ok(()) => {
                self.success_message = Some(format!("Reacted {emoji}"));
                self.reload_comments_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to react: {e}"));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Event handling
    // -----------------------------------------------------------------------

    fn handle_events(&mut self) -> Result<()> {
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                return Ok(());
            }

            // Clear error and success messages on any key press
            self.error_message = None;
            self.success_message = None;

            if self.modal_state.is_some() {
                self.handle_modal_input(key.code);
            } else if self.show_org_popup {
                self.handle_org_popup(key.code);
            } else {
                self.handle_normal(key.code);
            }
        }
        Ok(())
    }

    fn handle_org_popup(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.show_org_popup = false;
            }
            KeyCode::Up | KeyCode::Char('k') if self.org_popup_index > 0 => {
                self.org_popup_index -= 1;
            }
            KeyCode::Down | KeyCode::Char('j')
                if !self.orgs.is_empty() && self.org_popup_index < self.orgs.len() - 1 =>
            {
                self.org_popup_index += 1;
            }
            KeyCode::Enter => {
                let new_index = self.org_popup_index;
                self.show_org_popup = false;
                if new_index != self.current_org_index {
                    self.current_org_index = new_index;
                    self.load_goal_tree();
                    self.load_members();
                    // Execution は次に表示された時に再ロードする。
                    self.todays_loaded = false;
                    self.todays_goals_tree = GoalTree::empty();
                }
            }
            _ => {}
        }
    }

    fn handle_normal(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.running = false,
            KeyCode::Tab => {
                self.active_pane = match self.active_pane {
                    ActivePane::OrgSelector => ActivePane::Navigation,
                    ActivePane::Navigation => ActivePane::Content,
                    ActivePane::Content => ActivePane::OrgSelector,
                };
            }
            KeyCode::BackTab => {
                self.active_pane = match self.active_pane {
                    ActivePane::OrgSelector => ActivePane::Content,
                    ActivePane::Navigation => ActivePane::OrgSelector,
                    ActivePane::Content => ActivePane::Navigation,
                };
            }
            KeyCode::Enter => match self.active_pane {
                ActivePane::OrgSelector => {
                    self.org_popup_index = self.current_org_index;
                    self.show_org_popup = true;
                }
                ActivePane::Navigation => {
                    self.active_pane = ActivePane::Content;
                }
                ActivePane::Content if self.sidebar_index == 0 || self.sidebar_index == 1 => {
                    self.handle_goal_expand();
                }
                _ => {}
            },
            KeyCode::Char('o') | KeyCode::Char(' ')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_action_menu();
            }
            KeyCode::Up | KeyCode::Char('k') => match self.active_pane {
                ActivePane::Navigation if self.sidebar_index > 0 => {
                    self.sidebar_index -= 1;
                }
                ActivePane::Content if self.sidebar_index == 0 || self.sidebar_index == 1 => {
                    self.active_goal_tree_mut().cursor_up();
                }
                ActivePane::Content if self.sidebar_index == 2 => {
                    self.members_cursor_up();
                }
                _ => {}
            },
            KeyCode::Down | KeyCode::Char('j') => match self.active_pane {
                ActivePane::Navigation if self.sidebar_index < self.sidebar_items.len() - 1 => {
                    self.sidebar_index += 1;
                }
                ActivePane::Content if self.sidebar_index == 0 || self.sidebar_index == 1 => {
                    self.active_goal_tree_mut().cursor_down();
                }
                ActivePane::Content if self.sidebar_index == 2 => {
                    self.members_cursor_down();
                }
                _ => {}
            },
            KeyCode::Right | KeyCode::Char('l') => match self.active_pane {
                ActivePane::Navigation => {
                    self.active_pane = ActivePane::Content;
                }
                ActivePane::Content if self.sidebar_index == 0 || self.sidebar_index == 1 => {
                    self.handle_goal_expand();
                }
                _ => {}
            },
            KeyCode::Left | KeyCode::Char('h')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.active_goal_tree_mut().collapse_or_parent();
            }
            KeyCode::Char('c')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_create_goal_modal();
            }
            KeyCode::Char('C')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.active_goal_tree_mut().cycle_comment_view();
                let mode = self.active_goal_tree().comment_view.label();
                self.success_message = Some(format!("Comments: {mode}"));
            }
            KeyCode::Char('e')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_edit_goal_modal();
            }
            KeyCode::Char('d')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_delete_goal_modal();
            }
            KeyCode::Char('a')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_add_deliverable_modal();
            }
            KeyCode::Char('u')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_update_deliverable_modal();
            }
            KeyCode::Char('r')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_rename_deliverable_modal();
            }
            KeyCode::Char('m')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_move_deliverable_modal();
            }
            KeyCode::Char('x')
                if self.active_pane == ActivePane::Content
                    && (self.sidebar_index == 0 || self.sidebar_index == 1) =>
            {
                self.start_delete_deliverable_modal();
            }
            _ => {}
        }
    }

    fn handle_goal_expand(&mut self) {
        // Check if this is a complete tree (from todays-goals) - if so, only toggle expand
        let is_completed_tree = !self.active_goal_tree().is_required_to_fetch;

        // Check if cursor is on CommentOmitted row - if so, increase display limit
        {
            let tree = self.active_goal_tree();
            let rows = tree.flatten();
            if matches!(rows.get(tree.cursor), Some(TreeRow::CommentOmitted { .. })) {
                self.active_goal_tree_mut().increase_comment_limit();
                return;
            }
        }

        // If this is a complete tree, just toggle expand without fetching
        if is_completed_tree {
            self.active_goal_tree_mut().toggle_expand();
            return;
        }

        // Extract info from the cursor row before mutating
        let info = {
            let tree = self.active_goal_tree();
            let rows = tree.flatten();
            match rows.get(tree.cursor) {
                Some(TreeRow::Goal {
                    goal_id,
                    has_children,
                    children_loaded,
                    comments_loaded,
                    deliverables_loaded,
                    ..
                }) => Some((
                    goal_id.to_string(),
                    *has_children,
                    *children_loaded,
                    *comments_loaded,
                    *deliverables_loaded,
                )),
                _ => None,
            }
        };

        let Some((goal_id, has_children, children_loaded, comments_loaded, deliverables_loaded)) =
            info
        else {
            return;
        };

        // Determine what needs to be loaded
        let need_children = has_children && !children_loaded;
        let need_comments = !comments_loaded;
        let need_deliverables = !deliverables_loaded;

        // Lazy-load data if needed
        if need_comments || need_deliverables || need_children {
            let goal_id_ref = &goal_id;
            let client = &self.client;
            let result = self.api_call(async {
                tokio::try_join!(
                    async {
                        if need_comments {
                            client.list_comments(goal_id_ref).await
                        } else {
                            Ok(crate::api::CommentsResponse {
                                comments: vec![],
                                total_count: 0,
                            })
                        }
                    },
                    async {
                        if need_deliverables {
                            client.get_goal_deliverables(goal_id_ref).await
                        } else {
                            Ok(crate::api::ApiResponse {
                                data: crate::api::DeliverableListData {
                                    deliverables: vec![],
                                    total: 0,
                                },
                            })
                        }
                    },
                    async {
                        if need_children {
                            client.get_goal_children(goal_id_ref, 100, 0).await
                        } else {
                            Ok(crate::api::ApiResponse {
                                data: crate::api::GoalChildrenData {
                                    children: vec![],
                                    pagination: None,
                                },
                            })
                        }
                    }
                )
            });

            match result {
                Ok((comments_resp, deliverables_resp, children_resp)) => {
                    let tree = self.active_goal_tree_mut();
                    if need_comments {
                        tree.set_comments_at_cursor(comments_resp.comments);
                    }
                    if need_deliverables {
                        tree.set_deliverables_at_cursor(deliverables_resp.data.deliverables);
                    }
                    if need_children {
                        tree.set_children_at_cursor(children_resp.data.children);
                    }
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load data: {e}"));
                    return;
                }
            }
        }

        self.active_goal_tree_mut().toggle_expand();
    }

    // -----------------------------------------------------------------------
    // Modal handling - Create/Edit Goal
    // -----------------------------------------------------------------------

    fn start_create_goal_modal(&mut self) {
        // Get parent goal info from cursor position
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        let (parent_goal_id, parent_goal_title) = match rows.get(tree.cursor) {
            Some(TreeRow::Goal { goal_id, title, .. }) => {
                (Some(goal_id.to_string()), Some(title.to_string()))
            }
            _ => (None, None),
        };

        self.modal_state = Some(ModalState::CreateGoal {
            title: String::new(),
            description: String::new(),
            parent_goal_id,
            parent_goal_title,
            current_field: FormField::Title,
        });
    }

    fn start_edit_goal_modal(&mut self) {
        // Get goal info from cursor position
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        let goal_info = match rows.get(tree.cursor) {
            Some(TreeRow::Goal {
                goal_id,
                status,
                is_completed,
                ..
            }) => {
                let display_status = GoalDisplayStatus::from_goal_state(*status, *is_completed);
                Some((goal_id.to_string(), display_status))
            }
            _ => None,
        };

        if let Some((goal_id, current_status)) = goal_info {
            // Fetch full goal details to get description
            match self.api_call(self.client.get_goal(&goal_id)) {
                Ok(resp) => {
                    let goal = resp.data;
                    let allowed_statuses = current_status.allowed_transitions();

                    self.modal_state = Some(ModalState::EditGoal {
                        goal_id: goal.id,
                        title: goal.title,
                        description: goal.description.unwrap_or_default(),
                        current_status,
                        selected_status_index: 0,
                        allowed_statuses,
                        current_field: FormField::Title,
                    });
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load goal: {e}"));
                }
            }
        } else {
            self.error_message = Some("Please select a goal to edit".to_string());
        }
    }

    fn start_delete_goal_modal(&mut self) {
        // Get goal info from cursor position
        let tree = self.active_goal_tree();
        let rows = tree.flatten();
        let goal_info = match rows.get(tree.cursor) {
            Some(TreeRow::Goal { goal_id, title, .. }) => {
                Some((goal_id.to_string(), title.to_string()))
            }
            _ => None,
        };

        if let Some((goal_id, goal_title)) = goal_info {
            self.modal_state = Some(ModalState::DeleteGoal {
                goal_id,
                goal_title,
                confirm_index: 0, // Default to Cancel
            });
        } else {
            self.error_message = Some("Please select a goal to delete".to_string());
        }
    }

    fn start_add_deliverable_modal(&mut self) {
        let Some((goal_id, goal_title)) = self.selected_goal_context() else {
            self.error_message = Some("Please select a goal to add a deliverable".to_string());
            return;
        };

        self.modal_state = Some(ModalState::AddDeliverable {
            goal_id,
            goal_title,
            kind: DeliverableKind::File,
            name: String::new(),
            value: String::new(),
            current_field: DeliverableFormField::Kind,
        });
    }

    fn start_update_deliverable_modal(&mut self) {
        let Some((goal_id, deliverable_id, deliverable_name, _, node_type)) =
            self.selected_deliverable_context()
        else {
            self.error_message = Some("Please select a document deliverable to update".to_string());
            return;
        };
        if node_type != DeliverableType::Document {
            self.error_message = Some("Only document deliverables can be updated".to_string());
            return;
        }

        self.modal_state = Some(ModalState::UpdateDeliverable {
            goal_id,
            deliverable_id,
            deliverable_name,
            content_file: String::new(),
        });
    }

    fn start_rename_deliverable_modal(&mut self) {
        let Some((goal_id, deliverable_id, deliverable_name, _, _)) =
            self.selected_deliverable_context()
        else {
            self.error_message = Some("Please select a deliverable to rename".to_string());
            return;
        };

        self.modal_state = Some(ModalState::RenameDeliverable {
            goal_id,
            deliverable_id,
            current_name: deliverable_name.clone(),
            name: deliverable_name,
        });
    }

    fn start_move_deliverable_modal(&mut self) {
        let Some((goal_id, deliverable_id, deliverable_name, _, _)) =
            self.selected_deliverable_context()
        else {
            self.error_message = Some("Please select a deliverable to move".to_string());
            return;
        };
        let targets = self.deliverable_folder_targets_for_goal(&goal_id, &deliverable_id);

        self.modal_state = Some(ModalState::MoveDeliverable {
            goal_id,
            deliverable_id,
            deliverable_name,
            targets,
            selected_index: 0,
        });
    }

    fn start_delete_deliverable_modal(&mut self) {
        let Some((goal_id, deliverable_id, deliverable_name, _, _)) =
            self.selected_deliverable_context()
        else {
            self.error_message = Some("Please select a deliverable to delete".to_string());
            return;
        };

        self.modal_state = Some(ModalState::DeleteDeliverable {
            goal_id,
            deliverable_id,
            deliverable_name,
            confirm_index: 0,
        });
    }

    fn handle_modal_input(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.modal_state = None;
            }
            KeyCode::Tab => {
                self.modal_next_field();
            }
            KeyCode::BackTab => {
                self.modal_prev_field();
            }
            KeyCode::Enter => {
                self.modal_submit();
            }
            KeyCode::Up => {
                if matches!(self.modal_state, Some(ModalState::ActionMenu { .. })) {
                    self.modal_menu_prev();
                } else if matches!(self.modal_state, Some(ModalState::MoveDeliverable { .. })) {
                    self.modal_move_target_prev();
                } else if matches!(self.modal_state, Some(ModalState::ReactComment { .. })) {
                    self.modal_react_prev();
                } else if let Some(ModalState::EditGoal {
                    current_field: FormField::Status,
                    ..
                }) = &self.modal_state
                {
                    self.modal_prev_status();
                } else if let Some(ModalState::AddDeliverable {
                    current_field: DeliverableFormField::Kind,
                    ..
                }) = &self.modal_state
                {
                    self.modal_prev_deliverable_kind();
                }
            }
            KeyCode::Down => {
                if matches!(self.modal_state, Some(ModalState::ActionMenu { .. })) {
                    self.modal_menu_next();
                } else if matches!(self.modal_state, Some(ModalState::MoveDeliverable { .. })) {
                    self.modal_move_target_next();
                } else if matches!(self.modal_state, Some(ModalState::ReactComment { .. })) {
                    self.modal_react_next();
                } else if let Some(ModalState::EditGoal {
                    current_field: FormField::Status,
                    ..
                }) = &self.modal_state
                {
                    self.modal_next_status();
                } else if let Some(ModalState::AddDeliverable {
                    current_field: DeliverableFormField::Kind,
                    ..
                }) = &self.modal_state
                {
                    self.modal_next_deliverable_kind();
                }
            }
            KeyCode::Left => {
                // For delete confirmation modal
                if let Some(ModalState::DeleteGoal { confirm_index, .. }) = &mut self.modal_state {
                    *confirm_index = 0; // Cancel
                } else if let Some(ModalState::DeleteDeliverable { confirm_index, .. }) =
                    &mut self.modal_state
                {
                    *confirm_index = 0;
                } else if let Some(ModalState::DeleteComment { confirm_index, .. }) =
                    &mut self.modal_state
                {
                    *confirm_index = 0;
                } else if matches!(self.modal_state, Some(ModalState::ReactComment { .. })) {
                    self.modal_react_prev();
                }
            }
            KeyCode::Right => {
                // For delete confirmation modal
                if let Some(ModalState::DeleteGoal { confirm_index, .. }) = &mut self.modal_state {
                    *confirm_index = 1; // Delete
                } else if let Some(ModalState::DeleteDeliverable { confirm_index, .. }) =
                    &mut self.modal_state
                {
                    *confirm_index = 1;
                } else if let Some(ModalState::DeleteComment { confirm_index, .. }) =
                    &mut self.modal_state
                {
                    *confirm_index = 1;
                } else if matches!(self.modal_state, Some(ModalState::ReactComment { .. })) {
                    self.modal_react_next();
                }
            }
            KeyCode::Char(c) => {
                if matches!(self.modal_state, Some(ModalState::ActionMenu { .. })) {
                    match c {
                        'j' => self.modal_menu_next(),
                        'k' => self.modal_menu_prev(),
                        _ => {}
                    }
                } else if matches!(self.modal_state, Some(ModalState::MoveDeliverable { .. })) {
                    match c {
                        'j' => self.modal_move_target_next(),
                        'k' => self.modal_move_target_prev(),
                        _ => {}
                    }
                } else if let Some(ModalState::DeleteGoal { confirm_index, .. }) =
                    &mut self.modal_state
                {
                    match c {
                        'h' => *confirm_index = 0, // Cancel
                        'l' => *confirm_index = 1, // Delete
                        _ => {}
                    }
                } else if let Some(ModalState::DeleteDeliverable { confirm_index, .. }) =
                    &mut self.modal_state
                {
                    match c {
                        'h' => *confirm_index = 0,
                        'l' => *confirm_index = 1,
                        _ => {}
                    }
                } else if let Some(ModalState::DeleteComment { confirm_index, .. }) =
                    &mut self.modal_state
                {
                    match c {
                        'h' => *confirm_index = 0,
                        'l' => *confirm_index = 1,
                        _ => {}
                    }
                } else if matches!(self.modal_state, Some(ModalState::ReactComment { .. })) {
                    match c {
                        'j' | 'l' => self.modal_react_next(),
                        'k' | 'h' => self.modal_react_prev(),
                        _ => {}
                    }
                } else {
                    self.modal_input_char(c);
                }
            }
            KeyCode::Backspace => {
                self.modal_backspace();
            }
            _ => {}
        }
    }

    fn modal_next_field(&mut self) {
        if let Some(ref mut modal) = self.modal_state {
            match modal {
                ModalState::CreateGoal { current_field, .. } => {
                    *current_field = match current_field {
                        FormField::Title => FormField::Description,
                        FormField::Description => FormField::Title,
                        _ => FormField::Title,
                    };
                }
                ModalState::EditGoal { current_field, .. } => {
                    *current_field = match current_field {
                        FormField::Title => FormField::Description,
                        FormField::Description => FormField::Status,
                        FormField::Status => FormField::Title,
                    };
                }
                ModalState::DeleteGoal { .. } => {
                    // No fields to navigate in delete modal
                }
                ModalState::AddDeliverable { current_field, .. } => {
                    *current_field = match current_field {
                        DeliverableFormField::Kind => DeliverableFormField::Name,
                        DeliverableFormField::Name => DeliverableFormField::Value,
                        DeliverableFormField::Value => DeliverableFormField::Kind,
                    };
                }
                ModalState::ActionMenu { .. } | ModalState::MoveDeliverable { .. } => {}
                ModalState::UpdateDeliverable { .. }
                | ModalState::RenameDeliverable { .. }
                | ModalState::DeleteDeliverable { .. } => {}
                ModalState::AddComment { .. }
                | ModalState::ReplyComment { .. }
                | ModalState::EditComment { .. }
                | ModalState::DeleteComment { .. }
                | ModalState::ReactComment { .. } => {}
            }
        }
    }

    fn modal_prev_field(&mut self) {
        if let Some(ref mut modal) = self.modal_state {
            match modal {
                ModalState::CreateGoal { current_field, .. } => {
                    *current_field = match current_field {
                        FormField::Title => FormField::Description,
                        FormField::Description => FormField::Title,
                        _ => FormField::Title,
                    };
                }
                ModalState::EditGoal { current_field, .. } => {
                    *current_field = match current_field {
                        FormField::Title => FormField::Status,
                        FormField::Description => FormField::Title,
                        FormField::Status => FormField::Description,
                    };
                }
                ModalState::DeleteGoal { .. } => {
                    // No fields to navigate in delete modal
                }
                ModalState::AddDeliverable { current_field, .. } => {
                    *current_field = match current_field {
                        DeliverableFormField::Kind => DeliverableFormField::Value,
                        DeliverableFormField::Name => DeliverableFormField::Kind,
                        DeliverableFormField::Value => DeliverableFormField::Name,
                    };
                }
                ModalState::ActionMenu { .. } | ModalState::MoveDeliverable { .. } => {}
                ModalState::UpdateDeliverable { .. }
                | ModalState::RenameDeliverable { .. }
                | ModalState::DeleteDeliverable { .. } => {}
                ModalState::AddComment { .. }
                | ModalState::ReplyComment { .. }
                | ModalState::EditComment { .. }
                | ModalState::DeleteComment { .. }
                | ModalState::ReactComment { .. } => {}
            }
        }
    }

    fn modal_input_char(&mut self, c: char) {
        if let Some(ref mut modal) = self.modal_state {
            match modal {
                ModalState::CreateGoal {
                    title,
                    description,
                    current_field,
                    ..
                } => match current_field {
                    FormField::Title => title.push(c),
                    FormField::Description => description.push(c),
                    _ => {}
                },
                ModalState::EditGoal {
                    title,
                    description,
                    current_field,
                    ..
                } => match current_field {
                    FormField::Title => title.push(c),
                    FormField::Description => description.push(c),
                    _ => {}
                },
                ModalState::DeleteGoal { .. } => {
                    // No text input in delete modal
                }
                ModalState::AddDeliverable {
                    name,
                    value,
                    current_field,
                    ..
                } => match current_field {
                    DeliverableFormField::Name => name.push(c),
                    DeliverableFormField::Value => value.push(c),
                    _ => {}
                },
                ModalState::UpdateDeliverable { content_file, .. } => {
                    content_file.push(c);
                }
                ModalState::RenameDeliverable { name, .. } => {
                    name.push(c);
                }
                ModalState::AddComment { body, .. }
                | ModalState::ReplyComment { body, .. }
                | ModalState::EditComment { body, .. } => {
                    body.push(c);
                }
                ModalState::ActionMenu { .. } | ModalState::MoveDeliverable { .. } => {}
                ModalState::DeleteDeliverable { .. }
                | ModalState::DeleteComment { .. }
                | ModalState::ReactComment { .. } => {}
            }
        }
    }

    fn modal_backspace(&mut self) {
        if let Some(ref mut modal) = self.modal_state {
            match modal {
                ModalState::CreateGoal {
                    title,
                    description,
                    current_field,
                    ..
                } => match current_field {
                    FormField::Title => {
                        title.pop();
                    }
                    FormField::Description => {
                        description.pop();
                    }
                    _ => {}
                },
                ModalState::EditGoal {
                    title,
                    description,
                    current_field,
                    ..
                } => match current_field {
                    FormField::Title => {
                        title.pop();
                    }
                    FormField::Description => {
                        description.pop();
                    }
                    _ => {}
                },
                ModalState::DeleteGoal { .. } => {
                    // No text input in delete modal
                }
                ModalState::AddDeliverable {
                    name,
                    value,
                    current_field,
                    ..
                } => match current_field {
                    DeliverableFormField::Name => {
                        name.pop();
                    }
                    DeliverableFormField::Value => {
                        value.pop();
                    }
                    _ => {}
                },
                ModalState::UpdateDeliverable { content_file, .. } => {
                    content_file.pop();
                }
                ModalState::RenameDeliverable { name, .. } => {
                    name.pop();
                }
                ModalState::AddComment { body, .. }
                | ModalState::ReplyComment { body, .. }
                | ModalState::EditComment { body, .. } => {
                    body.pop();
                }
                ModalState::ActionMenu { .. } | ModalState::MoveDeliverable { .. } => {}
                ModalState::DeleteDeliverable { .. }
                | ModalState::DeleteComment { .. }
                | ModalState::ReactComment { .. } => {}
            }
        }
    }

    fn modal_submit(&mut self) {
        match self.modal_state.take() {
            Some(ModalState::ActionMenu {
                items,
                selected_index,
                ..
            }) => {
                if let Some(item) = items.get(selected_index).cloned() {
                    self.run_action_menu_item(item);
                }
            }
            Some(ModalState::CreateGoal {
                title,
                description,
                parent_goal_id,
                ..
            }) => {
                self.modal_submit_create(title, description, parent_goal_id);
            }
            Some(ModalState::EditGoal {
                goal_id,
                title,
                description,
                current_status,
                selected_status_index,
                allowed_statuses,
                ..
            }) => {
                // Get selected status from allowed transitions
                let new_status = allowed_statuses
                    .get(selected_status_index)
                    .cloned()
                    .unwrap_or(current_status);
                self.modal_submit_edit(goal_id, title, description, new_status);
            }
            Some(ModalState::DeleteGoal {
                goal_id,
                confirm_index: 1,
                ..
            }) => {
                // User confirmed deletion
                self.modal_submit_delete(goal_id);
            }
            Some(ModalState::DeleteGoal { .. }) => {
                // confirm_index == 0 means cancel, do nothing
            }
            Some(ModalState::AddDeliverable {
                goal_id,
                kind,
                name,
                value,
                ..
            }) => {
                self.modal_submit_add_deliverable(goal_id, kind, name, value);
            }
            Some(ModalState::UpdateDeliverable {
                goal_id,
                deliverable_id,
                content_file,
                ..
            }) => {
                self.modal_submit_update_deliverable(goal_id, deliverable_id, content_file);
            }
            Some(ModalState::RenameDeliverable {
                goal_id,
                deliverable_id,
                name,
                ..
            }) => {
                self.modal_submit_rename_deliverable(goal_id, deliverable_id, name);
            }
            Some(ModalState::MoveDeliverable {
                goal_id,
                deliverable_id,
                targets,
                selected_index,
                ..
            }) => {
                let parent = targets
                    .get(selected_index)
                    .and_then(|(parent_id, _)| parent_id.clone());
                self.modal_submit_move_deliverable(goal_id, deliverable_id, parent);
            }
            Some(ModalState::DeleteDeliverable {
                goal_id,
                deliverable_id,
                confirm_index: 1,
                ..
            }) => {
                self.modal_submit_delete_deliverable(goal_id, deliverable_id);
            }
            Some(ModalState::DeleteDeliverable { .. }) => {}
            Some(ModalState::AddComment { goal_id, body, .. }) => {
                self.modal_submit_add_comment(goal_id, None, body);
            }
            Some(ModalState::ReplyComment {
                goal_id,
                parent_comment_id,
                body,
                ..
            }) => {
                self.modal_submit_add_comment(goal_id, Some(parent_comment_id), body);
            }
            Some(ModalState::EditComment {
                goal_id,
                comment_id,
                body,
                ..
            }) => {
                self.modal_submit_edit_comment(goal_id, comment_id, body);
            }
            Some(ModalState::DeleteComment {
                goal_id,
                comment_id,
                confirm_index: 1,
                ..
            }) => {
                self.modal_submit_delete_comment(goal_id, comment_id);
            }
            Some(ModalState::DeleteComment { .. }) => {}
            Some(ModalState::ReactComment {
                goal_id,
                comment_id,
                emojis,
                selected_index,
            }) => {
                if let Some(emoji) = emojis.get(selected_index) {
                    self.modal_submit_react_comment(goal_id, comment_id, emoji);
                }
            }
            None => {}
        }
    }

    fn modal_submit_create(
        &mut self,
        title: String,
        description: String,
        parent_goal_id: Option<String>,
    ) {
        // Validate
        if title.trim().is_empty() {
            self.error_message = Some("Title is required".to_string());
            return;
        }

        let Some(org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.error_message = Some("No organization selected".to_string());
            return;
        };

        let req = CreateGoalRequest {
            organization_id: org_id,
            title,
            parent_objective_id: parent_goal_id,
            description: if description.is_empty() {
                None
            } else {
                Some(description)
            },
        };

        match self.api_call(self.client.create_goal(&req)) {
            Ok(_) => {
                self.success_message = Some("Goal created successfully".to_string());
                self.load_goal_tree();
                self.load_todays_goals();
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to create goal: {e}"));
            }
        }
    }

    fn modal_submit_edit(
        &mut self,
        goal_id: String,
        title: String,
        description: String,
        new_display_status: GoalDisplayStatus,
    ) {
        // Validate
        if title.trim().is_empty() {
            self.error_message = Some("Title is required".to_string());
            return;
        }

        let Some(_org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.error_message = Some("No organization selected".to_string());
            return;
        };

        // Determine completed_at and status based on new display status
        let (api_status, completed_at) = match new_display_status {
            GoalDisplayStatus::Completed => {
                // Mark as completed: set completed_at to current time
                (GoalStatus::None, Some(Some(Utc::now().to_rfc3339())))
            }
            _ => {
                // Not completed: uncomplete if needed, set appropriate status
                (new_display_status.to_goal_status(), Some(None))
            }
        };

        let req = UpdateGoalRequest {
            title: Some(title),
            description: Some(description),
            status: Some(api_status),
            completed_at,
        };

        match self.api_call(self.client.update_goal(&goal_id, &req)) {
            Ok(_) => {
                self.success_message = Some("Goal updated successfully".to_string());
                self.load_goal_tree();
                self.load_todays_goals();
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to update goal: {e}"));
            }
        }
    }

    fn modal_submit_delete(&mut self, goal_id: String) {
        match self.api_call(self.client.delete_goal(&goal_id)) {
            Ok(_) => {
                self.success_message = Some("Goal deleted successfully".to_string());
                self.load_goal_tree();
                self.load_todays_goals();
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to delete goal: {e}"));
            }
        }
    }

    fn run_action_menu_item(&mut self, item: ActionMenuItem) {
        match item {
            ActionMenuItem::AddDeliverable => self.start_add_deliverable_modal(),
            ActionMenuItem::AddComment => self.start_add_comment_modal(),
            ActionMenuItem::EditGoal => self.start_edit_goal_modal(),
            ActionMenuItem::DeleteGoal => self.start_delete_goal_modal(),
            ActionMenuItem::UpdateDeliverable => self.start_update_deliverable_modal(),
            ActionMenuItem::RenameDeliverable => self.start_rename_deliverable_modal(),
            ActionMenuItem::MoveDeliverable => self.start_move_deliverable_modal(),
            ActionMenuItem::DeleteDeliverable => self.start_delete_deliverable_modal(),
            ActionMenuItem::ReplyComment => self.start_reply_comment_modal(),
            ActionMenuItem::ResolveComment => self.do_set_comment_resolved(true),
            ActionMenuItem::UnresolveComment => self.do_set_comment_resolved(false),
            ActionMenuItem::EditComment => self.start_edit_comment_modal(),
            ActionMenuItem::DeleteComment => self.start_delete_comment_modal(),
            ActionMenuItem::ReactComment => self.start_react_comment_modal(),
        }
    }

    fn modal_submit_add_deliverable(
        &mut self,
        goal_id: String,
        kind: DeliverableKind,
        name: String,
        value: String,
    ) {
        let display_name = name.trim();
        let value = value.trim();

        let result = match kind {
            DeliverableKind::File => {
                if value.is_empty() {
                    self.error_message = Some("File path is required".to_string());
                    return;
                }
                let path = PathBuf::from(value);
                let display = if display_name.is_empty() {
                    None
                } else {
                    Some(display_name)
                };
                self.api_call(
                    self.client
                        .create_file_deliverable_from_path(&goal_id, &path, display),
                )
                .map(|resp| resp.data.id)
            }
            DeliverableKind::Document => {
                if value.is_empty() {
                    self.error_message = Some("Content file path is required".to_string());
                    return;
                }
                let path = PathBuf::from(value);
                let content = match std::fs::read_to_string(&path) {
                    Ok(content) => content,
                    Err(e) => {
                        self.error_message = Some(format!(
                            "Failed to read content file {}: {e}",
                            path.display()
                        ));
                        return;
                    }
                };
                let display = if display_name.is_empty() {
                    match path.file_name().and_then(|s| s.to_str()) {
                        Some(file_name) => file_name.to_string(),
                        None => {
                            self.error_message = Some("Name is required".to_string());
                            return;
                        }
                    }
                } else {
                    display_name.to_string()
                };
                self.api_call(
                    self.client
                        .create_document_deliverable(&goal_id, &display, &content),
                )
                .map(|resp| resp.data.id)
            }
            DeliverableKind::Link => {
                if display_name.is_empty() || value.is_empty() {
                    self.error_message = Some("Name and URL are required".to_string());
                    return;
                }
                self.api_call(
                    self.client
                        .create_link_deliverable(&goal_id, value, display_name),
                )
                .map(|resp| resp.data.id)
            }
            DeliverableKind::Folder => {
                if display_name.is_empty() {
                    self.error_message = Some("Name is required".to_string());
                    return;
                }
                self.api_call(
                    self.client
                        .create_folder_deliverable(&goal_id, display_name),
                )
                .map(|resp| resp.data.id)
            }
        };

        match result {
            Ok(id) => {
                self.success_message = Some(format!("Deliverable added: {id}"));
                self.reload_deliverables_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to add deliverable: {e}"));
            }
        }
    }

    fn modal_submit_update_deliverable(
        &mut self,
        goal_id: String,
        deliverable_id: String,
        content_file: String,
    ) {
        let path = PathBuf::from(content_file.trim());
        if path.as_os_str().is_empty() {
            self.error_message = Some("Content file path is required".to_string());
            return;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                self.error_message = Some(format!(
                    "Failed to read content file {}: {e}",
                    path.display()
                ));
                return;
            }
        };

        match self.api_call(self.client.update_deliverable(
            &goal_id,
            &deliverable_id,
            &content,
            vec![],
        )) {
            Ok(_) => {
                self.success_message = Some("Deliverable updated".to_string());
                self.reload_deliverables_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to update deliverable: {e}"));
            }
        }
    }

    fn modal_submit_rename_deliverable(
        &mut self,
        goal_id: String,
        deliverable_id: String,
        name: String,
    ) {
        let name = name.trim();
        if name.is_empty() {
            self.error_message = Some("Name is required".to_string());
            return;
        }

        match self.api_call(
            self.client
                .rename_deliverable(&goal_id, &deliverable_id, name),
        ) {
            Ok(_) => {
                self.success_message = Some("Deliverable renamed".to_string());
                self.reload_deliverables_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to rename deliverable: {e}"));
            }
        }
    }

    fn modal_submit_move_deliverable(
        &mut self,
        goal_id: String,
        deliverable_id: String,
        parent: Option<String>,
    ) {
        match self.api_call(
            self.client
                .move_deliverable(&goal_id, &deliverable_id, parent, 0.0),
        ) {
            Ok(_) => {
                self.success_message = Some("Deliverable moved".to_string());
                self.reload_deliverables_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to move deliverable: {e}"));
            }
        }
    }

    fn modal_submit_delete_deliverable(&mut self, goal_id: String, deliverable_id: String) {
        match self.api_call(self.client.delete_deliverable(&goal_id, &deliverable_id)) {
            Ok(_) => {
                self.success_message = Some("Deliverable deleted".to_string());
                self.reload_deliverables_for_goal(&goal_id);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to delete deliverable: {e}"));
            }
        }
    }

    fn modal_next_status(&mut self) {
        if let Some(ModalState::EditGoal {
            selected_status_index,
            allowed_statuses,
            ..
        }) = &mut self.modal_state
            && !allowed_statuses.is_empty()
        {
            *selected_status_index = (*selected_status_index + 1) % allowed_statuses.len();
        }
    }

    fn modal_prev_status(&mut self) {
        if let Some(ModalState::EditGoal {
            selected_status_index,
            allowed_statuses,
            ..
        }) = &mut self.modal_state
            && !allowed_statuses.is_empty()
        {
            *selected_status_index = if *selected_status_index == 0 {
                allowed_statuses.len() - 1
            } else {
                *selected_status_index - 1
            };
        }
    }

    fn modal_next_deliverable_kind(&mut self) {
        if let Some(ModalState::AddDeliverable { kind, .. }) = &mut self.modal_state {
            *kind = kind.next();
        }
    }

    fn modal_prev_deliverable_kind(&mut self) {
        if let Some(ModalState::AddDeliverable { kind, .. }) = &mut self.modal_state {
            *kind = kind.prev();
        }
    }

    fn modal_menu_next(&mut self) {
        if let Some(ModalState::ActionMenu {
            selected_index,
            items,
            ..
        }) = &mut self.modal_state
            && !items.is_empty()
        {
            *selected_index = (*selected_index + 1) % items.len();
        }
    }

    fn modal_menu_prev(&mut self) {
        if let Some(ModalState::ActionMenu {
            selected_index,
            items,
            ..
        }) = &mut self.modal_state
            && !items.is_empty()
        {
            *selected_index = if *selected_index == 0 {
                items.len() - 1
            } else {
                *selected_index - 1
            };
        }
    }

    fn modal_move_target_next(&mut self) {
        if let Some(ModalState::MoveDeliverable {
            selected_index,
            targets,
            ..
        }) = &mut self.modal_state
            && !targets.is_empty()
        {
            *selected_index = (*selected_index + 1) % targets.len();
        }
    }

    fn modal_move_target_prev(&mut self) {
        if let Some(ModalState::MoveDeliverable {
            selected_index,
            targets,
            ..
        }) = &mut self.modal_state
            && !targets.is_empty()
        {
            *selected_index = if *selected_index == 0 {
                targets.len() - 1
            } else {
                *selected_index - 1
            };
        }
    }

    fn modal_react_next(&mut self) {
        if let Some(ModalState::ReactComment {
            selected_index,
            emojis,
            ..
        }) = &mut self.modal_state
            && !emojis.is_empty()
        {
            *selected_index = (*selected_index + 1) % emojis.len();
        }
    }

    fn modal_react_prev(&mut self) {
        if let Some(ModalState::ReactComment {
            selected_index,
            emojis,
            ..
        }) = &mut self.modal_state
            && !emojis.is_empty()
        {
            *selected_index = if *selected_index == 0 {
                emojis.len() - 1
            } else {
                *selected_index - 1
            };
        }
    }
}
