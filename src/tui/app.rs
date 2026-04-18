use anyhow::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
};
use tokio::runtime::Handle;

use crate::api::{ApiClient, Organization};

use super::goal_tree::{GoalTree, TreeRow};
use super::ui;

#[derive(PartialEq, Eq)]
pub enum ActivePane {
    OrgSelector,
    Navigation,
    Content,
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

    // Goal tree
    pub goal_tree: GoalTree,

    /// Last known content viewport height (for scroll calculations)
    pub content_height: usize,

    /// Error message to display in status bar (cleared on next key press)
    pub error_message: Option<String>,
}

impl App {
    pub fn new(client: ApiClient, rt: Handle) -> Self {
        Self {
            client,
            rt,
            running: true,
            active_pane: ActivePane::Navigation,
            sidebar_index: 0,
            sidebar_items: vec!["Goals", "Comments"],
            orgs: vec![],
            current_org_index: 0,
            show_org_popup: false,
            org_popup_index: 0,
            goal_tree: GoalTree::empty(),
            content_height: 0,
            error_message: None,
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
                self.orgs = resp.data;
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
    }

    fn load_goal_tree(&mut self) {
        let Some(org_id) = self.current_org_id().map(|s| s.to_string()) else {
            self.goal_tree = GoalTree::empty();
            return;
        };

        self.client.set_org_id(Some(org_id.clone()));

        match self.api_call(self.client.get_goal_tree(&org_id, 1)) {
            Ok(resp) => {
                self.goal_tree = GoalTree::from_tree_items(resp.data.items);
            }
            Err(e) => {
                self.goal_tree = GoalTree::empty();
                self.error_message = Some(format!("Failed to load goals: {e}"));
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

            // Clear error on any key press
            self.error_message = None;

            if self.show_org_popup {
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
            KeyCode::Up | KeyCode::Char('k') => {
                if self.org_popup_index > 0 {
                    self.org_popup_index -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.orgs.is_empty() && self.org_popup_index < self.orgs.len() - 1 {
                    self.org_popup_index += 1;
                }
            }
            KeyCode::Enter => {
                let new_index = self.org_popup_index;
                self.show_org_popup = false;
                if new_index != self.current_org_index {
                    self.current_org_index = new_index;
                    self.load_goal_tree();
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
                ActivePane::Content if self.sidebar_index == 0 => {
                    self.handle_goal_expand();
                }
                _ => {}
            },
            KeyCode::Up | KeyCode::Char('k') => match self.active_pane {
                ActivePane::Navigation => {
                    if self.sidebar_index > 0 {
                        self.sidebar_index -= 1;
                    }
                }
                ActivePane::Content if self.sidebar_index == 0 => {
                    self.goal_tree.cursor_up();
                }
                _ => {}
            },
            KeyCode::Down | KeyCode::Char('j') => match self.active_pane {
                ActivePane::Navigation => {
                    if self.sidebar_index < self.sidebar_items.len() - 1 {
                        self.sidebar_index += 1;
                    }
                }
                ActivePane::Content if self.sidebar_index == 0 => {
                    self.goal_tree.cursor_down();
                }
                _ => {}
            },
            KeyCode::Right | KeyCode::Char('l') => {
                if self.active_pane == ActivePane::Content && self.sidebar_index == 0 {
                    self.handle_goal_expand();
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.active_pane == ActivePane::Content && self.sidebar_index == 0 {
                    self.goal_tree.collapse_or_parent();
                }
            }
            _ => {}
        }
    }

    fn handle_goal_expand(&mut self) {
        // Extract info from the cursor row before mutating
        let info = {
            let rows = self.goal_tree.flatten();
            match rows.get(self.goal_tree.cursor) {
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
                    if need_comments {
                        self.goal_tree.set_comments_at_cursor(comments_resp.comments);
                    }
                    if need_deliverables {
                        self.goal_tree
                            .set_deliverables_at_cursor(deliverables_resp.data.deliverables);
                    }
                    if need_children {
                        self.goal_tree
                            .set_children_at_cursor(children_resp.data.children);
                    }
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load data: {e}"));
                    return;
                }
            }
        }

        self.goal_tree.toggle_expand();
    }
}
