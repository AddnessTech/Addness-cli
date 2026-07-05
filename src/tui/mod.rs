mod agent;
mod app;
mod codex_memory;
mod codex_pane;
mod file_picker;
mod goal_tree;
mod ui;

use anyhow::Result;

use crate::api::ApiClient;

pub fn run(client: ApiClient) -> Result<()> {
    let rt = tokio::runtime::Handle::current();
    tokio::task::block_in_place(|| {
        let mut terminal = ratatui::init();
        let _ = ratatui::crossterm::execute!(
            std::io::stdout(),
            ratatui::crossterm::event::EnableBracketedPaste
        );
        let _ = ratatui::crossterm::execute!(
            std::io::stdout(),
            ratatui::crossterm::event::PushKeyboardEnhancementFlags(
                ratatui::crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
            )
        );
        let result = app::App::new(client, rt).run(&mut terminal);
        // codex 使用中に終了した場合に備え、マウスキャプチャを念のため解除する。
        {
            use std::io::Write as _;
            let _ = std::io::stdout().write_all(b"\x1b[?1007l");
        }
        let _ = ratatui::crossterm::execute!(
            std::io::stdout(),
            ratatui::crossterm::event::PopKeyboardEnhancementFlags
        );
        let _ = ratatui::crossterm::execute!(
            std::io::stdout(),
            ratatui::crossterm::event::DisableBracketedPaste,
            ratatui::crossterm::event::DisableMouseCapture
        );
        ratatui::restore();
        result
    })
}
