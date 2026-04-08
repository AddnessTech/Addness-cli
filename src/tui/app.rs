use anyhow::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
};

use super::ui;

#[derive(PartialEq, Eq)]
pub enum ActivePane {
    Sidebar,
    Content,
}

pub struct App {
    pub running: bool,
    pub active_pane: ActivePane,
    pub sidebar_index: usize,
    pub sidebar_items: Vec<&'static str>,
}

impl App {
    pub fn new() -> Self {
        Self {
            running: true,
            active_pane: ActivePane::Sidebar,
            sidebar_index: 0,
            sidebar_items: vec!["Organizations", "Goals", "Comments"],
        }
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
            match key.code {
                KeyCode::Char('q') => self.running = false,
                KeyCode::Esc => self.running = false,
                KeyCode::Up | KeyCode::Char('k') => {
                    if self.sidebar_index > 0 {
                        self.sidebar_index -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.sidebar_index < self.sidebar_items.len() - 1 {
                        self.sidebar_index += 1;
                    }
                }
                KeyCode::Tab => {
                    self.active_pane = match self.active_pane {
                        ActivePane::Sidebar => ActivePane::Content,
                        ActivePane::Content => ActivePane::Sidebar,
                    };
                }
                _ => {}
            }
        }
        Ok(())
    }
}
