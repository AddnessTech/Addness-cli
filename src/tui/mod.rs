mod app;
mod goal_tree;
mod ui;

use anyhow::Result;

pub fn run() -> Result<()> {
    let mut terminal = ratatui::init();
    let result = app::App::new().run(&mut terminal);
    ratatui::restore();
    result
}
