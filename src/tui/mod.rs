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
        // codex 使用中に終了した場合に備え、マウスキャプチャを念のため解除する。
        let _ = ratatui::crossterm::execute!(
            std::io::stdout(),
            ratatui::crossterm::event::DisableMouseCapture
        );
        ratatui::restore();
        result
    })
}
