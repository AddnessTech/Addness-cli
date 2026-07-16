mod agent;
mod app;
mod codex_memory;
mod file_picker;
mod goal_tree;
mod markdown;
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
        // フォーカス復帰イベントを受け取れるようにする。画面切り替え後に戻ってきた際、
        // ratatui は前回描画したバッファとの差分しか送らないため、離れている間に端末側の
        // 表示が乱れても検知できず黒画面のまま固まることがある。FocusGained を
        // needs_full_clear のトリガに使い、復帰時に必ず全消去→再描画させる。
        let _ = ratatui::crossterm::execute!(
            std::io::stdout(),
            ratatui::crossterm::event::EnableFocusChange
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
            ratatui::crossterm::event::DisableFocusChange,
            ratatui::crossterm::event::DisableBracketedPaste,
            ratatui::crossterm::event::DisableMouseCapture
        );
        ratatui::restore();
        result
    })
}
