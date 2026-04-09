use anyhow::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
};

use super::goal_tree::{GoalTree, TreeRow};
use super::ui;

#[derive(PartialEq, Eq)]
pub enum ActivePane {
    OrgSelector,
    Navigation,
    Content,
}

pub struct MockOrg {
    pub id: &'static str,
    pub name: &'static str,
}

pub struct App {
    pub running: bool,
    pub active_pane: ActivePane,
    pub sidebar_index: usize,
    pub sidebar_items: Vec<&'static str>,

    // Organization state
    pub orgs: Vec<MockOrg>,
    pub current_org_index: usize,
    pub show_org_popup: bool,
    pub org_popup_index: usize,

    // Goal tree
    pub goal_tree: GoalTree,

    /// Last known content viewport height (for scroll calculations)
    pub content_height: usize,
}

impl App {
    pub fn new() -> Self {
        let orgs = vec![
            MockOrg {
                id: "org-001",
                name: "Addness Inc.",
            },
            MockOrg {
                id: "org-002",
                name: "Side Project",
            },
            MockOrg {
                id: "org-003",
                name: "Personal",
            },
        ];
        Self {
            running: true,
            active_pane: ActivePane::Navigation,
            sidebar_index: 0,
            sidebar_items: vec!["Goals", "Comments"],
            orgs,
            current_org_index: 0,
            show_org_popup: false,
            org_popup_index: 0,
            goal_tree: GoalTree::mock(),
            content_height: 0,
        }
    }

    pub fn current_org_name(&self) -> &str {
        self.orgs[self.current_org_index].name
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while self.running {
            terminal.draw(|frame| ui::draw(frame, self))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn handle_events(&mut self) -> Result<()> {
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                return Ok(());
            }

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
                if self.org_popup_index < self.orgs.len() - 1 {
                    self.org_popup_index += 1;
                }
            }
            KeyCode::Enter => {
                self.current_org_index = self.org_popup_index;
                self.show_org_popup = false;
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
        let rows = self.goal_tree.flatten();
        if let Some(TreeRow::Goal { .. }) = rows.get(self.goal_tree.cursor) {
            self.goal_tree.toggle_expand();
        }
    }
}
