mod app;
mod codex_pane;
mod goal_tree;
mod ui;

use anyhow::Result;

use crate::api::ApiClient;

pub fn run(client: ApiClient) -> Result<()> {
    let rt = tokio::runtime::Handle::current();
    tokio::task::block_in_place(|| {
        let mut terminal = ratatui::init();
        let result = app::App::new(client, rt).run(&mut terminal);
        ratatui::restore();
        result
    })
}
