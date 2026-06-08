use anyhow::Result;
use chrono::Utc;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
};
use std::collections::HashMap;
use tokio::runtime::Handle;

use crate::api::{
    ApiClient, CreateGoalRequest, GoalStatus, Member, MemberId, Organization, UpdateGoalRequest,
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

// NOTE: 将来goal以外についてのモーダルが出得るためsuffixにGoalと付けている
// それまでclippy errorを抑制
#[allow(clippy::enum_variant_names)]
pub enum ModalState {
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
            terminal.draw(|frame| ui::draw(frame, self))?;
            self.handle_events()?;
        }
        Ok(())
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

        self.load_goal_tree();
        self.load_todays_goals();
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

        // Load comments and deliverables for each root goal by id (not by flat index)
        for goal_id in root_goal_ids {
            let goal_id_ref = &goal_id;
            let client = &self.client;

            let result = self.api_call(async {
                tokio::try_join!(
                    client.list_comments(goal_id_ref),
                    client.get_goal_deliverables(goal_id_ref)
                )
            });

            match result {
                Ok((comments_resp, deliverables_resp)) => {
                    self.goal_tree
                        .set_comments_for_goal_id(&goal_id, comments_resp.comments);
                    self.goal_tree.set_deliverables_for_goal_id(
                        &goal_id,
                        deliverables_resp.data.deliverables,
                    );
                }
                Err(e) => {
                    self.error_message = Some(e.to_string());
                }
            }
        }
    }

    fn load_todays_goals(&mut self) {
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
                    self.load_todays_goals();
                    self.load_members();
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
                // For status field in edit modal
                if let Some(ModalState::EditGoal {
                    current_field: FormField::Status,
                    ..
                }) = &self.modal_state
                {
                    self.modal_prev_status();
                }
            }
            KeyCode::Down => {
                // For status field in edit modal
                if let Some(ModalState::EditGoal {
                    current_field: FormField::Status,
                    ..
                }) = &self.modal_state
                {
                    self.modal_next_status();
                }
            }
            KeyCode::Left => {
                // For delete confirmation modal
                if let Some(ModalState::DeleteGoal { confirm_index, .. }) = &mut self.modal_state {
                    *confirm_index = 0; // Cancel
                }
            }
            KeyCode::Right => {
                // For delete confirmation modal
                if let Some(ModalState::DeleteGoal { confirm_index, .. }) = &mut self.modal_state {
                    *confirm_index = 1; // Delete
                }
            }
            KeyCode::Char(c) => {
                // For delete confirmation modal, handle h/l as left/right
                if let Some(ModalState::DeleteGoal { confirm_index, .. }) = &mut self.modal_state {
                    match c {
                        'h' => *confirm_index = 0, // Cancel
                        'l' => *confirm_index = 1, // Delete
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
            }
        }
    }

    fn modal_submit(&mut self) {
        match self.modal_state.take() {
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
}
