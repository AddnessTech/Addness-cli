use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use std::collections::HashMap;
use std::time::Instant;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use tui_term::widget::PseudoTerminal;

use super::app::{ActivePane, App, DeliverableFormField, FormField, ModalState};
use super::codex_pane::CodexPane;
use super::goal_tree::{CommentView, TreeRow};
use crate::api::{DeliverableType, GoalStatus, Member, MemberId};

const COLOR_ADDNESS: Color = Color::Rgb(91, 171, 255);
const COLOR_CODEX: Color = Color::Rgb(207, 140, 255);
const COLOR_MEMORY: Color = Color::Rgb(84, 214, 190);
const COLOR_SUCCESS: Color = Color::Rgb(101, 218, 123);
const COLOR_WARN: Color = Color::Rgb(236, 188, 80);
const COLOR_MUTED: Color = Color::Rgb(112, 122, 138);
const COLOR_PANEL: Color = Color::Rgb(65, 81, 105);

/// Replace @uuid mentions in text with @member_name
fn replace_member_mentions(text: &str, members: &HashMap<MemberId, Member>) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '@' {
            // Collect characters after @
            let mut potential_uuid = String::new();
            while let Some(&next_ch) = chars.peek() {
                if next_ch.is_alphanumeric() || next_ch == '-' {
                    potential_uuid.push(chars.next().unwrap());
                } else {
                    break;
                }
            }

            // Check if it looks like a UUID (36 chars with hyphens at positions 8, 13, 18, 23)
            let is_uuid_like = potential_uuid.len() == 36
                && potential_uuid.chars().nth(8) == Some('-')
                && potential_uuid.chars().nth(13) == Some('-')
                && potential_uuid.chars().nth(18) == Some('-')
                && potential_uuid.chars().nth(23) == Some('-');

            if is_uuid_like {
                // Try to find member
                let member_id = MemberId::new(&potential_uuid);
                if let Some(member) = members.get(&member_id) {
                    result.push('@');
                    result.push_str(&member.name);
                } else {
                    result.push('@');
                    result.push_str(&potential_uuid);
                    result.push_str("(不明なメンバ)");
                }
            } else {
                // Not a UUID, keep original
                result.push('@');
                result.push_str(&potential_uuid);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(frame.area());

    draw_title_bar(frame, main_layout[0], app);

    if app.codex.is_some() {
        // codex 使用中は org/navigation を出さず（切り替えないため）、
        // 全幅を「Addnessの進行が見えるペイン + codex本体」に使う。
        draw_codex(frame, main_layout[1], app);
    } else {
        let content_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(24), Constraint::Min(0)])
            .split(main_layout[1]);

        draw_left_panel(frame, content_layout[0], app);
        draw_content(frame, content_layout[1], app);
    }
    draw_status_bar(frame, main_layout[2], app);

    if app.show_org_popup {
        draw_org_popup(frame, app);
    }

    // Draw modals last (on top of everything)
    if let Some(ref modal) = app.modal_state {
        match modal {
            ModalState::ActionMenu { .. } => draw_action_menu(frame, app),
            ModalState::CreateGoal { .. } => draw_create_goal_modal(frame, app),
            ModalState::EditGoal { .. } => draw_edit_goal_modal(frame, app),
            ModalState::DeleteGoal { .. } => draw_delete_goal_modal(frame, app),
            ModalState::AddDeliverable { .. } => draw_add_deliverable_modal(frame, app),
            ModalState::UpdateDeliverable { .. } => draw_update_deliverable_modal(frame, app),
            ModalState::RenameDeliverable { .. } => draw_rename_deliverable_modal(frame, app),
            ModalState::MoveDeliverable { .. } => draw_move_deliverable_modal(frame, app),
            ModalState::DeleteDeliverable { .. } => draw_delete_deliverable_modal(frame, app),
            ModalState::AddComment { .. } => draw_add_comment_modal(frame, app),
            ModalState::ReplyComment { .. } => draw_reply_comment_modal(frame, app),
            ModalState::EditComment { .. } => draw_edit_comment_modal(frame, app),
            ModalState::DeleteComment { .. } => draw_delete_comment_modal(frame, app),
            ModalState::ReactComment { .. } => draw_react_comment_modal(frame, app),
            ModalState::FilePicker { .. } => draw_file_picker_modal(frame, app),
        }
    }

    // Help overlay sits above everything else.
    if app.show_help {
        draw_help_overlay(frame);
    }
}

/// ロード画面に表示する Addness のシンボルロゴ。
/// インストーラ（install.sh）の起動バナーと同じアートを流用している。
/// 斜めの形状を内部の空白で表現しているため、各行は等幅にパディングして
/// ブロックごと中央寄せする（行ごとに中央寄せすると形が崩れる）。
const LOGO: [&str; 14] = [
    "                                        .",
    "                   .:=+*###***+=:.    =:",
    "               .=*%@@%*=:.    .:=**+#=",
    "            .:*@@@@*:.            :#%*:",
    "          .+@@@@@*.            :+%%=. .+=",
    "         =@@@@@@:          .=*%%+.     ::",
    "       .*@@@@@@.      .:+*%%%#=.        :",
    "      .@@@@@@@:  =+*#%%%%%%+:",
    "     .@@@@@@@+ .*%%%%%%#+:",
    "    .@@@@@@@@. *%%%%*=.",
    "    *@@@@@@@+ .%%*=.",
    "   :@@@@@@@@.",
    "   #@@@@@@@*",
    "   ++==::..",
];

/// ロゴを斜めに流れる波として、セル位置と時刻(tick)から青系の色を決める。
/// 位相を列・行・時刻の線形結合で作り、sin で明度を揺らす。
fn wave_color(x: usize, y: usize, tick: u64) -> Color {
    let phase = x as f32 * 0.35 + y as f32 * 0.7 - tick as f32 * 0.6;
    let t = (phase.sin() + 1.0) / 2.0; // 0.0..=1.0
    let lerp = |lo: f32, hi: f32| (lo + (hi - lo) * t) as u8;
    Color::Rgb(lerp(30.0, 150.0), lerp(110.0, 205.0), lerp(210.0, 255.0))
}

/// 起動直後、初期データ取得が終わるまで表示するロード画面。
/// `tick` は描画ごとに増えるカウンタで、ロゴを波打たせるのに使う。
pub fn draw_loading(frame: &mut Frame, tick: u64) {
    let area = frame.area();

    let logo_width = LOGO.iter().map(|l| l.width()).max().unwrap_or(0);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    // ロゴ＋枠＋上下左右の余白が収まる端末でだけロゴを出し、
    // 狭ければシンプルな文字表示に切り替える。
    let show_logo = area.width as usize >= logo_width + 4 && area.height >= 22;
    if show_logo {
        for (y, row) in LOGO.iter().enumerate() {
            // 1文字ずつ波の色を付ける。等幅になるよう末尾を空白で埋め、
            // 中央寄せでも形が崩れないようにする。
            let mut spans: Vec<Span> = Vec::with_capacity(logo_width + 1);
            for (x, ch) in row.chars().enumerate() {
                if ch == ' ' {
                    spans.push(Span::raw(" "));
                } else {
                    spans.push(Span::styled(
                        ch.to_string(),
                        Style::default()
                            .fg(wave_color(x, y, tick))
                            .add_modifier(Modifier::BOLD),
                    ));
                }
            }
            let pad = logo_width.saturating_sub(row.width());
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            lines.push(Line::from(spans));
        }
        lines.push(Line::from(""));
    } else {
        lines.push(Line::from(Span::styled(
            "Addness",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(Span::styled(
        "Goal Management TUI",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "読み込み中…",
        Style::default().fg(Color::White),
    )));

    // border(2) + 中身の行数。
    let box_height = lines.len() as u16 + 2;
    let inner_width = if show_logo { logo_width } else { 19 };
    let box_width = (inner_width as u16 + 4).min(area.width);
    let x = area.x + area.width.saturating_sub(box_width) / 2;
    let y = area.y + area.height.saturating_sub(box_height) / 2;
    let box_area = Rect::new(x, y, box_width, box_height.min(area.height));

    let paragraph = Paragraph::new(lines).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(format!(" addness v{} ", env!("CARGO_PKG_VERSION"))),
    );
    frame.render_widget(paragraph, box_area);
}

fn draw_title_bar(frame: &mut Frame, area: Rect, app: &App) {
    // codex 使用中は、参照しているローカルフォルダ（cwd）を出して文脈を明示する。
    let line = if let Some(pane) = app.codex.as_ref() {
        Line::from(vec![
            Span::styled(
                " codex ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("作業フォルダ: ", Style::default().fg(Color::DarkGray)),
            Span::styled(pane.cwd.as_str(), Style::default().fg(Color::White)),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                " Addness ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "- Goal Management TUI",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    };
    let border = if app.codex.is_some() {
        Color::Magenta
    } else {
        Color::Cyan
    };
    let title = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .title(format!(" addness v{} ", env!("CARGO_PKG_VERSION"))),
    );
    frame.render_widget(title, area);
}

/// Left panel: org selector (top) + navigation (bottom)
fn draw_left_panel(frame: &mut Frame, area: Rect, app: &App) {
    let panel_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    draw_org_pane(frame, panel_layout[0], app);
    draw_navigation(frame, panel_layout[1], app);
}

fn draw_org_pane(frame: &mut Frame, area: Rect, app: &App) {
    let is_active = app.active_pane == ActivePane::OrgSelector;
    let border_color = if is_active {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let org_name = app.current_org_name();
    let content = if is_active {
        Line::from(vec![
            Span::styled(
                format!(" {org_name} "),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" <Enter>", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(Span::styled(
            format!(" {org_name}"),
            Style::default().fg(Color::White),
        ))
    };

    let pane = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" Org "),
    );
    frame.render_widget(pane, area);
}

fn draw_navigation(frame: &mut Frame, area: Rect, app: &App) {
    let is_active = app.active_pane == ActivePane::Navigation;
    let highlight_color = if is_active {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let items: Vec<ListItem> = app
        .sidebar_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let selected = i == app.sidebar_index;
            let style = if selected {
                Style::default()
                    .fg(highlight_color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { " > " } else { "   " };
            ListItem::new(Line::from(Span::styled(format!("{prefix}{item}"), style)))
        })
        .collect();

    let nav = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if is_active {
                Color::Cyan
            } else {
                Color::DarkGray
            }))
            .title(" Navigation "),
    );
    frame.render_widget(nav, area);
}

fn draw_content(frame: &mut Frame, area: Rect, app: &mut App) {
    let border_color = if app.active_pane == ActivePane::Content {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    match app.sidebar_index {
        0 => draw_goals(frame, area, app, border_color, "Goals"),
        1 => draw_goals(frame, area, app, border_color, "Execution"),
        2 => draw_members(frame, area, app, border_color),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Goal tree rendering
// ---------------------------------------------------------------------------

fn draw_goals(frame: &mut Frame, area: Rect, app: &mut App, border_color: Color, title: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(format!(" {} ", title));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let viewport_h = inner.height as usize;
    app.content_height = viewport_h;

    app.active_goal_tree_mut().adjust_scroll(viewport_h);
    let tree = app.active_goal_tree();

    let rows = tree.flatten();
    let scroll = tree.scroll_offset;
    let cursor = tree.cursor;
    let comment_view = tree.comment_view;

    let visible = rows.iter().enumerate().skip(scroll).take(viewport_h);

    for (i, row) in visible {
        let y = inner.y + (i - scroll) as u16;
        if y >= inner.y + inner.height {
            break;
        }
        let line_area = Rect::new(inner.x, y, inner.width, 1);
        let is_cursor = i == cursor;

        let line = render_tree_row(
            row,
            is_cursor,
            inner.width as usize,
            &app.members,
            comment_view,
        );
        frame.render_widget(Paragraph::new(line), line_area);
    }
}

fn render_tree_row(
    row: &TreeRow,
    is_cursor: bool,
    width: usize,
    members: &HashMap<MemberId, Member>,
    comment_view: CommentView,
) -> Line<'static> {
    let bg = if is_cursor {
        Color::DarkGray
    } else {
        Color::Reset
    };

    match row {
        TreeRow::Goal {
            title,
            status,
            owner_name,
            is_completed,
            expanded,
            unresolved_comments,
            guide,
            ..
        } => {
            let prefix = guide.prefix();
            let icon = if *expanded { "▾ " } else { "▸ " };

            let status_str = format_status(*is_completed, *status);
            let owner_str = owner_name.unwrap_or("");

            let completed = *is_completed;
            let title_style = if completed {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::CROSSED_OUT)
                    .bg(bg)
            } else {
                Style::default().fg(Color::White).bg(bg)
            };

            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray).bg(bg)),
                Span::styled(icon, Style::default().fg(Color::Cyan).bg(bg)),
                Span::styled(title.to_string(), title_style),
            ];

            // コメント未解決件数のバッジ。インラインにコメントが見えない場面
            // （Hidden モード or 折りたたみ中）でのみ出して存在を可視化する。
            let comments_inline_visible = *expanded && !matches!(comment_view, CommentView::Hidden);
            if let Some(n) = unresolved_comments
                && *n > 0
                && !comments_inline_visible
            {
                spans.push(Span::styled(
                    format!("  \u{1F4AC}{n}"),
                    Style::default().fg(Color::Yellow).bg(bg),
                ));
            }

            // Append status + owner inline if there's room
            let meta = format_goal_meta(status_str, owner_str);
            if !meta.is_empty() {
                spans.push(Span::styled(
                    format!("  {meta}"),
                    Style::default().fg(Color::DarkGray).bg(bg),
                ));
            }

            // Pad to full width for cursor highlight
            if is_cursor {
                let content_width = spans_display_width(&spans);
                pad_line(&mut spans, content_width, width, bg);
            }

            Line::from(spans)
        }
        TreeRow::Detail {
            status,
            owner_name,
            description,
            is_completed,
            guide,
            ..
        } => {
            let prefix = guide.prefix();
            let status_str = format_status(*is_completed, *status);
            let owner_str = owner_name.unwrap_or("-");
            let desc = description.unwrap_or("");

            let text = if desc.is_empty() {
                format!("{status_str} | {owner_str}")
            } else {
                format!("{status_str} | {owner_str} | {desc}")
            };

            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray).bg(bg)),
                Span::styled(text, Style::default().fg(Color::DarkGray).bg(bg)),
            ];
            if is_cursor {
                let content_width = spans_display_width(&spans);
                pad_line(&mut spans, content_width, width, bg);
            }
            Line::from(spans)
        }
        TreeRow::CommentHeader { count, guide, .. } => {
            let prefix = guide.prefix();
            let text = format!(
                "\u{1F4DD} {count} comment{}",
                if *count != 1 { "s" } else { "" }
            );

            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray).bg(bg)),
                Span::styled(text, Style::default().fg(Color::Yellow).bg(bg)),
            ];
            if is_cursor {
                let content_width = spans_display_width(&spans);
                pad_line(&mut spans, content_width, width, bg);
            }
            Line::from(spans)
        }
        TreeRow::CommentOmitted { count, guide, .. } => {
            let prefix = guide.prefix();
            let text = format!(
                "... {count} older comment{} hidden",
                if *count != 1 { "s" } else { "" }
            );

            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray).bg(bg)),
                Span::styled(text, Style::default().fg(Color::DarkGray).bg(bg)),
            ];
            if is_cursor {
                let content_width = spans_display_width(&spans);
                pad_line(&mut spans, content_width, width, bg);
            }
            Line::from(spans)
        }
        TreeRow::CommentItem { comment, guide, .. } => {
            let prefix = guide.prefix();
            let author = &comment.author.name;
            let resolved = comment.resolved_at.is_some();

            // 返信件数インジケータと解決済みマーカー。
            let mut suffix = String::new();
            if comment.reply_count > 0 {
                suffix.push_str(&format!("  \u{21B3}{}", comment.reply_count));
            }
            if resolved {
                suffix.push_str("  \u{2713}");
            }

            // Replace @uuid mentions with @member_name
            let content_with_mentions = replace_member_mentions(&comment.content, members);

            let content = truncate_str(
                &content_with_mentions,
                width.saturating_sub(
                    display_width(&prefix)
                        + display_width(author)
                        + display_width(": ")
                        + display_width(&suffix),
                ),
            );

            // 解決済みは淡色（All モードで主に出る）。
            let content_color = if resolved {
                Color::DarkGray
            } else {
                Color::White
            };
            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray).bg(bg)),
                Span::styled(
                    format!("{author}:"),
                    Style::default().fg(Color::Cyan).bg(bg),
                ),
                Span::styled(
                    format!(" {content}"),
                    Style::default().fg(content_color).bg(bg),
                ),
            ];
            if !suffix.is_empty() {
                spans.push(Span::styled(
                    suffix,
                    Style::default().fg(Color::DarkGray).bg(bg),
                ));
            }
            if is_cursor {
                let content_width = spans_display_width(&spans);
                pad_line(&mut spans, content_width, width, bg);
            }
            Line::from(spans)
        }
        TreeRow::DeliverableHeader { count, guide, .. } => {
            let prefix = guide.prefix();
            let text = format!(
                "\u{1F4CE} {count} deliverable{}",
                if *count != 1 { "s" } else { "" }
            );

            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray).bg(bg)),
                Span::styled(text, Style::default().fg(Color::Magenta).bg(bg)),
            ];
            if is_cursor {
                let content_width = spans_display_width(&spans);
                pad_line(&mut spans, content_width, width, bg);
            }
            Line::from(spans)
        }
        TreeRow::DeliverableItem {
            deliverable, guide, ..
        } => {
            let prefix = guide.prefix();
            let icon = match deliverable.node_type {
                DeliverableType::Document => "\u{1F4C4}",
                DeliverableType::Folder => "\u{1F4C1}",
                DeliverableType::File => "\u{1F4CE}",
                DeliverableType::Link => "\u{1F517}",
            };
            let text = format!("{icon} {}", deliverable.display_name);

            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray).bg(bg)),
                Span::styled(text, Style::default().fg(Color::White).bg(bg)),
            ];
            if is_cursor {
                let content_width = spans_display_width(&spans);
                pad_line(&mut spans, content_width, width, bg);
            }
            Line::from(spans)
        }
    }
}

fn format_status(is_completed: bool, status: Option<&GoalStatus>) -> &'static str {
    if is_completed {
        "完了"
    } else {
        match status {
            Some(GoalStatus::InProgress) => "進行中",
            Some(GoalStatus::Cancelled) => "停止中",
            _ => "未着手",
        }
    }
}

fn format_goal_meta(status: &str, owner: &str) -> String {
    match (status, owner) {
        ("-", "") => String::new(),
        (s, "") => s.to_string(),
        ("-", o) => o.to_string(),
        (s, o) => format!("{s} | {o}"),
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if display_width(s) <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else if max > 3 {
        let mut out = String::new();
        let mut width = 0;
        let limit = max - 3;

        for ch in s.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + ch_width > limit {
                break;
            }
            width += ch_width;
            out.push(ch);
        }

        out.push_str("...");
        out
    } else {
        let mut out = String::new();
        let mut width = 0;

        for ch in s.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + ch_width > max {
                break;
            }
            width += ch_width;
            out.push(ch);
        }

        out
    }
}

fn pad_line(spans: &mut Vec<Span<'static>>, content_len: usize, width: usize, bg: Color) {
    if content_len < width {
        spans.push(Span::styled(
            " ".repeat(width - content_len),
            Style::default().bg(bg),
        ));
    }
}

fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn spans_display_width(spans: &[Span<'static>]) -> usize {
    spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum()
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    // Show error message if present
    if let Some(ref err) = app.error_message {
        let status = Paragraph::new(Line::from(vec![
            Span::styled(
                " ERROR: ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .title(" Error "),
        );
        frame.render_widget(status, area);
        return;
    }

    // Show success message if present
    if let Some(ref msg) = app.success_message {
        let status = Paragraph::new(Line::from(vec![
            Span::styled(
                " SUCCESS: ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(msg.clone(), Style::default().fg(Color::Green)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .title(" Success "),
        );
        frame.render_widget(status, area);
        return;
    }

    let current_section = app.sidebar_items[app.sidebar_index];

    // codex フォーカス中は通常のキーが codex へ転送されるため、専用のヒントを出す。
    if app.active_pane == ActivePane::Codex {
        let finished = app.codex.as_ref().map(|c| c.finished).unwrap_or(true);
        let hint = if finished {
            " [c]コメント  [s]状態  [d]成果物(PR/Release)  [v]DoD判定  Esc/q: 閉じる "
        } else {
            " 入力はcodexへ転送  |  F9:Addnessから再開  |  Shift+↑↓/PgUp/PgDn: ログ  |  F12: 終了 "
        };
        let status = Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(COLOR_CODEX),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_CODEX))
                .title(" codex "),
        );
        frame.render_widget(status, area);
        return;
    }

    let pane_label = match app.active_pane {
        ActivePane::OrgSelector => "Org",
        ActivePane::Navigation => "Nav",
        ActivePane::Content => "Content",
        ActivePane::Codex => "codex",
    };

    let mut hints = vec![
        Span::styled(
            " q",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Quit  "),
        Span::styled(
            "Tab/S-Tab",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Switch Pane  "),
        Span::styled(
            "\u{2191}\u{2193}/jk",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Navigate  "),
        Span::styled(
            "?",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Help  "),
    ];

    if app.active_pane == ActivePane::Content && (app.sidebar_index == 0 || app.sidebar_index == 1)
    {
        hints.push(Span::styled(
            "Enter/l",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        hints.push(Span::raw(": Expand  "));
        hints.push(Span::styled(
            "h",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        hints.push(Span::raw(": Collapse  "));
        hints.push(Span::styled(
            "c",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        hints.push(Span::raw(": Create  "));
        hints.push(Span::styled(
            "e",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        hints.push(Span::raw(": Edit  "));
        hints.push(Span::styled(
            "d",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        hints.push(Span::raw(": Delete  "));
        hints.push(Span::styled(
            "o/Space",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        hints.push(Span::raw(": Actions  "));
        hints.push(Span::styled(
            "a/u/r/m/x",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        hints.push(Span::raw(": Direct  "));
        hints.push(Span::styled(
            "C",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        hints.push(Span::raw(format!(
            ": Comments({})  ",
            app.active_goal_tree().comment_view.label()
        )));
    }

    hints.push(Span::styled("|", Style::default().fg(Color::DarkGray)));
    hints.push(Span::styled(
        format!(" [{pane_label}] {current_section} "),
        Style::default().fg(Color::Yellow),
    ));

    let status = Paragraph::new(Line::from(hints)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Help "),
    );
    frame.render_widget(status, area);
}

// ---------------------------------------------------------------------------
// Help overlay
// ---------------------------------------------------------------------------

fn draw_help_overlay(frame: &mut Frame) {
    // (key, description) の行。section() は見出し。
    let section = |title: &str| {
        Line::from(Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let kv = |key: &str, desc: &str| {
        Line::from(vec![
            Span::styled(
                format!("  {key:<16}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(desc.to_string(), Style::default().fg(Color::White)),
        ])
    };
    let blank = || Line::from("");

    let lines: Vec<Line> = vec![
        section("全体 / ペイン"),
        kv("Tab / S-Tab", "ペイン移動"),
        kv("?", "ヘルプ表示 / 閉じる"),
        kv("q / Esc", "終了"),
        blank(),
        section("サイドバー (Navigation)"),
        kv("↑↓ / j k", "項目移動"),
        kv("Enter / → / l", "コンテンツへ移動"),
        blank(),
        section("ゴールツリー (Goals / Execution)"),
        kv("↑↓ / j k", "カーソル移動"),
        kv("Enter / → / l", "展開 / 子へ"),
        kv("h / ←", "折りたたみ / 親へ"),
        kv("c", "ゴール作成"),
        kv("e", "ゴール編集"),
        kv("d", "ゴール削除"),
        kv("C", "コメント表示切替 (非表示/未解決/全件)"),
        kv("o / Space", "アクションメニュー"),
        kv("a u r m x", "成果物: 追加/更新/リネーム/移動/削除"),
        blank(),
        section("アクションメニュー (o) の内容"),
        kv(
            "ゴール上",
            "codexで作業 / 完了・再開 / コメント追加 / 成果物追加 / 編集 / 削除",
        ),
        kv(
            "コメント上",
            "返信 / 解決・未解決 / 編集 / 削除 / リアクション",
        ),
        blank(),
        section("codex連携 (o →「codexで作業」)"),
        kv("起動", "選択ゴールの文脈付きでcodexをペイン起動"),
        kv("F9", "Addnessの作業メモ・決定ログから再開"),
        kv("Shift+↑↓", "実行中のcodexログをスクロール"),
        kv(
            "Shift+PgUp/PgDn",
            "実行中のcodexログをページ単位でスクロール",
        ),
        kv("F12", "実行中のcodexを終了して戻る"),
        kv(
            "終了後 c/s/d/v",
            "還流: コメント / 状態 / 成果物(PR・Release) / DoD判定",
        ),
        kv("Esc / q", "codexペインを閉じる"),
        blank(),
        section("モーダル共通"),
        kv("Enter", "確定"),
        kv("Esc", "キャンセル"),
        kv("Tab", "次フィールド"),
        kv("←→ / h l", "選択 (確認 / 移動先 / 絵文字)"),
        blank(),
        section("ファイルパス入力"),
        kv("Tab", "パス補完（共通接頭辞まで・~展開）"),
        kv("Ctrl+F", "ファイラーを開いて選択"),
    ];

    let height = (lines.len() as u16 + 2).min(frame.area().height);
    let area = centered_rect(64, height, frame.area());
    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Keybindings ")
        .title_bottom(
            Line::from(" ? / Esc / q: Close ").style(Style::default().fg(Color::DarkGray)),
        );
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

// ---------------------------------------------------------------------------
// Org selection popup
// ---------------------------------------------------------------------------

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_width = area.width * percent_x / 100;
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, popup_width, height.min(area.height))
}

/// モーダル/ポップアップ領域を消去する。
/// 背面に全角文字（絵文字・日本語・罫線）があると、モーダルの左右境界で
/// 全角セルの片側が残って透けて見えるため、左右を1列ずつ広げてクリアする。
fn clear_modal_area(frame: &mut Frame, area: Rect) {
    let full = frame.area();
    let left = area.x.saturating_sub(1);
    let right = (area.x + area.width)
        .saturating_add(1)
        .min(full.x + full.width);
    let expanded = Rect::new(left, area.y, right.saturating_sub(left), area.height);
    frame.render_widget(Clear, expanded);
}

fn draw_org_popup(frame: &mut Frame, app: &App) {
    let item_count = app.orgs.len() as u16;
    // border(2) + header(1) + items
    let popup_height = item_count + 3;
    let area = centered_rect(40, popup_height, frame.area());

    clear_modal_area(frame, area);

    let items: Vec<ListItem> = app
        .orgs
        .iter()
        .enumerate()
        .map(|(i, org)| {
            let selected = i == app.org_popup_index;
            let style = if selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { " > " } else { "   " };
            let marker = if i == app.current_org_index { " *" } else { "" };
            ListItem::new(Line::from(Span::styled(
                format!("{prefix}{}{marker}", org.name),
                style,
            )))
        })
        .collect();

    let popup = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Select Organization ")
            .title_bottom(
                Line::from(" Enter: select | Esc: cancel ")
                    .style(Style::default().fg(Color::DarkGray)),
            ),
    );
    frame.render_widget(popup, area);
}

// ---------------------------------------------------------------------------
// Modal dialogs - Create/Edit Goal
// ---------------------------------------------------------------------------

fn draw_create_goal_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::CreateGoal {
        title,
        description,
        parent_goal_title,
        current_field,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    let area = centered_rect(60, 15, frame.area());
    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Create Goal ")
        .title_bottom(
            Line::from(" Tab: Next Field | Enter: Create | Esc: Cancel ")
                .style(Style::default().fg(Color::DarkGray)),
        );

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into fields
    let field_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Length(3), // Description
            Constraint::Length(2), // Parent Goal (read-only)
            Constraint::Min(0),    // Spacer
        ])
        .split(inner);

    // Title / Description fields（draw_text_field でカーソル・全角幅対応を共通化）
    draw_text_field(
        frame,
        field_layout[0],
        " Title * ",
        title,
        *current_field == FormField::Title,
    );
    draw_text_field(
        frame,
        field_layout[1],
        " Description ",
        description,
        *current_field == FormField::Description,
    );

    // Parent Goal (read-only)
    let parent_text = parent_goal_title
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("(Root Goal)");
    let parent_widget = Paragraph::new(Line::from(vec![Span::styled(
        parent_text,
        Style::default().fg(Color::DarkGray),
    )]))
    .block(
        Block::default()
            .borders(Borders::NONE)
            .title(" Parent Goal: "),
    );
    frame.render_widget(parent_widget, field_layout[2]);
}

fn draw_edit_goal_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::EditGoal {
        title,
        description,
        current_status,
        selected_status_index,
        allowed_statuses,
        current_field,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    // Calculate status field height
    // "現在: ..." (1) + empty line (1) + transitions (n) + borders (2) = 4 + n
    let status_field_height = if !allowed_statuses.is_empty() {
        4 + allowed_statuses.len() as u16
    } else {
        4 // just "現在: ..." + borders
    };

    // Calculate total modal height
    // outer borders (2) + title field (3) + description field (3) + status field (status_field_height)
    let modal_height = 2 + 3 + 3 + status_field_height;

    let area = centered_rect(60, modal_height, frame.area());
    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Edit Goal ")
        .title_bottom(
            Line::from(
                " Tab: Next Field | \u{2191}\u{2193}: Change Status | Enter: Save | Esc: Cancel ",
            )
            .style(Style::default().fg(Color::DarkGray)),
        );

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let field_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),                   // Title
            Constraint::Length(3),                   // Description
            Constraint::Length(status_field_height), // Status
            Constraint::Min(0),                      // Spacer
        ])
        .split(inner);

    // Title / Description fields（draw_text_field でカーソル・全角幅対応を共通化）
    draw_text_field(
        frame,
        field_layout[0],
        " Title * ",
        title,
        *current_field == FormField::Title,
    );
    draw_text_field(
        frame,
        field_layout[1],
        " Description ",
        description,
        *current_field == FormField::Description,
    );

    // Status field - show current status and allowed transitions
    let status_focused = *current_field == FormField::Status;
    let status_border = if status_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    // Build status display
    let mut status_lines = vec![Line::from(vec![
        Span::styled("現在: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            current_status.to_emoji_string(),
            Style::default().fg(Color::White),
        ),
    ])];

    if !allowed_statuses.is_empty() {
        status_lines.push(Line::from(""));
        for (i, status_option) in allowed_statuses.iter().enumerate() {
            let is_selected = i == *selected_status_index;
            let prefix = if is_selected { " > " } else { "   " };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            status_lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, status_option.to_emoji_string()),
                style,
            )));
        }
    }

    let status_widget = Paragraph::new(status_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(status_border))
            .title(" Status (↑↓で選択) "),
    );
    frame.render_widget(status_widget, field_layout[2]);
}

fn draw_delete_goal_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::DeleteGoal {
        goal_title,
        confirm_index,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    let area = centered_rect(60, 10, frame.area());
    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(" 削除の確認 ")
        .title_bottom(
            Line::from(" ←→/hl: 選択 | Enter: 実行 | Esc: キャンセル ")
                .style(Style::default().fg(Color::DarkGray)),
        );

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Goal title
            Constraint::Length(2), // Warning message
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Buttons
            Constraint::Min(0),    // Spacer
        ])
        .split(inner);

    // Goal title
    let title_text = Paragraph::new(Line::from(vec![
        Span::styled("削除するゴール: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            goal_title.as_str(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    frame.render_widget(title_text, layout[0]);

    // Warning message
    let warning = Paragraph::new(Line::from(vec![
        Span::styled("! ", Style::default().fg(Color::Red)),
        Span::styled(
            "この操作は取り消せません。本当に削除しますか？",
            Style::default().fg(Color::Red),
        ),
    ]));
    frame.render_widget(warning, layout[1]);

    // Buttons
    let cancel_style = if *confirm_index == 0 {
        Style::default()
            .fg(Color::Black)
            .bg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let delete_style = if *confirm_index == 1 {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };

    let buttons = Line::from(vec![
        Span::raw("  "),
        Span::styled("[ キャンセル ]", cancel_style),
        Span::raw("    "),
        Span::styled("[ 削除 ]", delete_style),
    ]);

    frame.render_widget(Paragraph::new(buttons), layout[3]);
}

fn draw_action_menu(frame: &mut Frame, app: &App) {
    let Some(ModalState::ActionMenu {
        title,
        items,
        selected_index,
    }) = &app.modal_state
    else {
        return;
    };

    let height = (items.len() as u16 + 4).max(7);
    let area = centered_rect(48, height, frame.area());
    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Actions ")
        .title_bottom(
            Line::from(" j/k: Select | Enter: Open | Esc: Cancel ")
                .style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_str(title, layout[0].width as usize),
            Style::default().fg(Color::DarkGray),
        ))),
        layout[0],
    );

    let rows: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let selected = idx == *selected_index;
            let style = if selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { " > " } else { "   " };
            ListItem::new(Line::from(Span::styled(
                format!("{prefix}{}", item.label()),
                style,
            )))
        })
        .collect();
    frame.render_widget(List::new(rows), layout[1]);
}

fn draw_add_deliverable_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::AddDeliverable {
        goal_title,
        kind,
        name,
        value,
        current_field,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    let area = centered_rect(64, 17, frame.area());
    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Add Deliverable ")
        .title_bottom(
            Line::from(
                " Tab: Next/Complete | ↑↓: Kind | Ctrl+F: Browse | Enter: Add | Esc: Cancel ",
            )
            .style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Goal: ", Style::default().fg(Color::DarkGray)),
            Span::styled(goal_title.as_str(), Style::default().fg(Color::White)),
        ])),
        layout[0],
    );

    draw_readonly_field(
        frame,
        layout[1],
        " Kind ",
        kind.label(),
        *current_field == DeliverableFormField::Kind,
    );
    draw_text_field(
        frame,
        layout[2],
        " Name ",
        name,
        *current_field == DeliverableFormField::Name,
    );
    let value_title = match kind {
        super::app::DeliverableKind::File => " File Path * ",
        super::app::DeliverableKind::Document => " Content File Path * ",
        super::app::DeliverableKind::Link => " URL * ",
        super::app::DeliverableKind::Folder => " Value (unused) ",
    };
    draw_text_field(
        frame,
        layout[3],
        value_title,
        value,
        *current_field == DeliverableFormField::Value,
    );
}

fn draw_update_deliverable_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::UpdateDeliverable {
        deliverable_name,
        content_file,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    let area = centered_rect(60, 10, frame.area());
    clear_modal_area(frame, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Update Document Deliverable ")
        .title_bottom(
            Line::from(" Tab: Complete | Ctrl+F: Browse | Enter: Update | Esc: Cancel ")
                .style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Deliverable: ", Style::default().fg(Color::DarkGray)),
            Span::styled(deliverable_name.as_str(), Style::default().fg(Color::White)),
        ])),
        layout[0],
    );
    draw_text_field(
        frame,
        layout[1],
        " Content File Path * ",
        content_file,
        true,
    );
}

fn draw_rename_deliverable_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::RenameDeliverable {
        current_name, name, ..
    }) = &app.modal_state
    else {
        return;
    };

    let area = centered_rect(60, 10, frame.area());
    clear_modal_area(frame, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Rename Deliverable ")
        .title_bottom(
            Line::from(" Enter: Rename | Esc: Cancel ").style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Current: ", Style::default().fg(Color::DarkGray)),
            Span::styled(current_name.as_str(), Style::default().fg(Color::White)),
        ])),
        layout[0],
    );
    draw_text_field(frame, layout[1], " New Name * ", name, true);
}

fn draw_move_deliverable_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::MoveDeliverable {
        deliverable_name,
        targets,
        selected_index,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    let height = (targets.len() as u16 + 5).clamp(8, 18);
    let area = centered_rect(62, height, frame.area());
    clear_modal_area(frame, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Move Deliverable ")
        .title_bottom(
            Line::from(" j/k: Select Folder | Enter: Move | Esc: Cancel ")
                .style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Deliverable: ", Style::default().fg(Color::DarkGray)),
            Span::styled(deliverable_name.as_str(), Style::default().fg(Color::White)),
        ])),
        layout[0],
    );

    let rows: Vec<ListItem> = targets
        .iter()
        .enumerate()
        .map(|(idx, (_, label))| {
            let selected = idx == *selected_index;
            let style = if selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { " > " } else { "   " };
            ListItem::new(Line::from(Span::styled(format!("{prefix}{label}"), style)))
        })
        .collect();
    frame.render_widget(List::new(rows), layout[1]);
}

fn draw_delete_deliverable_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::DeleteDeliverable {
        deliverable_name,
        confirm_index,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    let area = centered_rect(60, 10, frame.area());
    clear_modal_area(frame, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(" Delete Deliverable ")
        .title_bottom(
            Line::from(" ←→/hl: Select | Enter: Apply | Esc: Cancel ")
                .style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Delete: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                deliverable_name.as_str(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "This cannot be undone.",
            Style::default().fg(Color::Red),
        ))),
        layout[1],
    );
    draw_confirm_buttons(frame, layout[3], *confirm_index, "Cancel", "Delete");
}

fn draw_add_comment_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::AddComment {
        goal_title, body, ..
    }) = &app.modal_state
    else {
        return;
    };

    let area = centered_rect(64, 10, frame.area());
    clear_modal_area(frame, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Add Comment ")
        .title_bottom(
            Line::from(" Enter: Post | Esc: Cancel ").style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Goal: ", Style::default().fg(Color::DarkGray)),
            Span::styled(goal_title.as_str(), Style::default().fg(Color::White)),
        ])),
        layout[0],
    );
    draw_text_field(frame, layout[1], " Comment * ", body, true);
}

fn draw_reply_comment_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::ReplyComment {
        parent_excerpt,
        body,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    let area = centered_rect(64, 10, frame.area());
    clear_modal_area(frame, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Reply to Comment ")
        .title_bottom(
            Line::from(" Enter: Reply | Esc: Cancel ").style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Re: ", Style::default().fg(Color::DarkGray)),
            Span::styled(parent_excerpt.as_str(), Style::default().fg(Color::White)),
        ])),
        layout[0],
    );
    draw_text_field(frame, layout[1], " Reply * ", body, true);
}

fn draw_edit_comment_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::EditComment { body, .. }) = &app.modal_state else {
        return;
    };

    let area = centered_rect(64, 10, frame.area());
    clear_modal_area(frame, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Edit Comment ")
        .title_bottom(
            Line::from(" Enter: Save | Esc: Cancel ").style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(inner);
    draw_text_field(frame, layout[0], " Comment * ", body, true);
}

fn draw_delete_comment_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::DeleteComment {
        excerpt,
        confirm_index,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    let area = centered_rect(60, 10, frame.area());
    clear_modal_area(frame, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(" Delete Comment ")
        .title_bottom(
            Line::from(" ←→/hl: Select | Enter: Apply | Esc: Cancel ")
                .style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Delete: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                excerpt.as_str(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "This cannot be undone.",
            Style::default().fg(Color::Red),
        ))),
        layout[1],
    );
    draw_confirm_buttons(frame, layout[3], *confirm_index, "Cancel", "Delete");
}

fn draw_react_comment_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::ReactComment {
        emojis,
        selected_index,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    let area = centered_rect(50, 8, frame.area());
    clear_modal_area(frame, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" React ")
        .title_bottom(
            Line::from(" ←→/hl: Select | Enter: React | Esc: Cancel ")
                .style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Pick a reaction:",
            Style::default().fg(Color::DarkGray),
        ))),
        layout[0],
    );

    let mut spans = vec![Span::raw("  ")];
    for (idx, emoji) in emojis.iter().enumerate() {
        let style = if idx == *selected_index {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        spans.push(Span::styled(format!(" {emoji} "), style));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), layout[1]);
}

fn draw_file_picker_modal(frame: &mut Frame, app: &App) {
    let Some(ModalState::FilePicker {
        dir,
        entries,
        selected_index,
        ..
    }) = &app.modal_state
    else {
        return;
    };

    let visible = super::app::PICKER_VISIBLE_ROWS as u16;
    // border(2) + dir行(1) + 区切り(1) + リスト
    let height = (visible + 4).min(frame.area().height);
    let area = centered_rect(70, height, frame.area());
    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Select File ")
        .title_bottom(
            Line::from(" j/k: Move | Enter/l: Open | h: Up | Esc: Cancel ")
                .style(Style::default().fg(Color::DarkGray)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    // 現在ディレクトリ（右寄せで切り詰め）。
    let dir_str = dir.to_string_lossy();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_str(&dir_str, layout[0].width as usize),
            Style::default().fg(Color::DarkGray),
        ))),
        layout[0],
    );

    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  (empty)",
                Style::default().fg(Color::DarkGray),
            ))),
            layout[1],
        );
        return;
    }

    // 実際に描画できる行数から可視窓を決め、選択行が常に収まるようにする。
    // （端末高に依存しない固定値でスクロールすると、低い端末で選択が画面外に出るため）
    let visible_rows = (layout[1].height as usize).max(1);
    let start = if *selected_index < visible_rows {
        0
    } else {
        *selected_index + 1 - visible_rows
    };
    let rows: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_rows)
        .map(|(idx, entry)| {
            let selected = idx == *selected_index;
            let icon = if entry.is_dir { "📁 " } else { "📄 " };
            let suffix = if entry.is_dir { "/" } else { "" };
            let style = if selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if entry.is_dir {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };
            let prefix = if selected { ">" } else { " " };
            ListItem::new(Line::from(Span::styled(
                format!("{prefix} {icon}{}{suffix}", entry.name),
                style,
            )))
        })
        .collect();
    frame.render_widget(List::new(rows), layout[1]);
}

fn draw_text_field(frame: &mut Frame, area: Rect, title: &str, text: &str, focused: bool) {
    let border = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let widget = Paragraph::new(text.to_string()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .title(title.to_string()),
    );
    frame.render_widget(widget, area);

    // フォーカス中は本物の端末カーソルを文末（表示幅基準）へ置く。
    // これで可視カーソルが出るうえ、日本語入力時に IME の変換ウィンドウが
    // 正しい位置に出る（全角は 2 セルとして数える）。caret は末尾固定。
    if focused {
        let inner_w = area.width.saturating_sub(2);
        let inner_h = area.height.saturating_sub(2);
        if inner_w > 0 && inner_h > 0 {
            let last_line = text.rsplit('\n').next().unwrap_or("");
            let line_idx = text.matches('\n').count() as u16;
            let col = (UnicodeWidthStr::width(last_line) as u16).min(inner_w.saturating_sub(1));
            let row = line_idx.min(inner_h.saturating_sub(1));
            frame.set_cursor_position((area.x + 1 + col, area.y + 1 + row));
        }
    }
}

fn draw_readonly_field(frame: &mut Frame, area: Rect, title: &str, text: &str, focused: bool) {
    let border = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let widget = Paragraph::new(Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(Color::White),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .title(title.to_string()),
    );
    frame.render_widget(widget, area);
}

fn draw_confirm_buttons(
    frame: &mut Frame,
    area: Rect,
    selected: usize,
    cancel_label: &str,
    confirm_label: &str,
) {
    let cancel_style = if selected == 0 {
        Style::default()
            .fg(Color::Black)
            .bg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let confirm_style = if selected == 1 {
        Style::default()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };

    let buttons = Paragraph::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(format!(" [ {cancel_label} ] "), cancel_style),
        Span::raw("    "),
        Span::styled(format!(" [ {confirm_label} ] "), confirm_style),
    ]));
    frame.render_widget(buttons, area);
}

fn draw_members(frame: &mut Frame, area: Rect, app: &mut App, border_color: Color) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" Members ");

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // Adjust scroll position
    let viewport_height = inner_area.height as usize;
    app.adjust_members_scroll(viewport_height);

    // Build member list items
    let is_active = app.active_pane == ActivePane::Content;
    let visible_members: Vec<ListItem> = app
        .members_list
        .iter()
        .enumerate()
        .skip(app.members_scroll_offset)
        .take(viewport_height)
        .map(|(idx, member)| {
            let is_cursor = idx == app.members_cursor;
            let is_current_user = member.is_current_user;

            let style = if is_cursor && is_active {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else if is_current_user {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let prefix = if is_current_user { "* " } else { "  " };
            let content = format!("{}{}", prefix, member.name);

            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(visible_members);
    frame.render_widget(list, inner_area);
}

/// codex ペインを契約併置型で描画する。
/// 左に対象ゴール／DoD（契約）、右に PTY 上で動く codex 本体。
fn draw_codex(frame: &mut Frame, area: Rect, app: &mut App) {
    // 左の Addness ペインは固定幅で広めに取り、進行が読み取りやすいようにする。
    let addness_w = (area.width / 3).clamp(28, 52);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(addness_w), Constraint::Min(0)])
        .split(area);

    // 同期の鼓動（スピナー＋最終同期からの経過秒）。可変借用前に App から読む。
    let sync_label = {
        const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let s = SPIN[(app.codex_sync_tick as usize) % SPIN.len()];
        match app.last_codex_sync {
            Some(t) => format!(" Addness ゴール  {s} 同期{}s前 ", t.elapsed().as_secs()),
            None => " Addness ゴール  ⟳ 同期待ち ".to_string(),
        }
    };

    // --- Addness ペイン（参照中 + ゴール状態 + DoD + 子ゴール + 更新ログ）---
    // 上段=ゴール/DoD/子ゴール、下段=Addnessの更新ログ。不変借用のまま描いて clone を避ける。
    if let Some(pane) = app.codex.as_ref() {
        let now = Instant::now();
        let recently = |at: Option<Instant>| {
            at.is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(4))
        };

        let status_panel_h = if chunks[0].height >= 26 { 11 } else { 10 };
        let panes = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(status_panel_h),
                Constraint::Min(0),
                Constraint::Length(8),
            ])
            .split(chunks[0]);

        draw_codex_status_panel(frame, panes[0], pane);

        let mut lines: Vec<Line> = Vec::new();

        // 「いま参照/書込中」インジケータ（codex の操作をリアルタイム表示）
        if let Some(action) = &pane.action {
            lines.push(Line::from(Span::styled(
                format!("» {action}"),
                Style::default()
                    .fg(COLOR_MEMORY)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        }

        // ゴール名
        lines.push(Line::from(Span::styled(
            pane.goal_title.as_str(),
            Style::default()
                .fg(COLOR_ADDNESS)
                .add_modifier(Modifier::BOLD),
        )));

        // ステータス（変化直後はハイライト）
        let status_style = if recently(pane.status_changed_at) {
            Style::default().fg(Color::Black).bg(COLOR_WARN)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::styled("状態: ", Style::default().fg(COLOR_MUTED)),
            Span::styled(pane.status_label.as_str(), status_style),
        ]));

        // DoD 進捗バー
        if !pane.dod_items.is_empty() {
            let met = pane.dod_checks.iter().filter(|c| **c == Some(true)).count();
            let total = pane.dod_items.len();
            let width = 10usize;
            let filled = met * width / total.max(1);
            let bar: String = "▓".repeat(filled) + &"░".repeat(width - filled);
            lines.push(Line::from(vec![
                Span::styled("DoD ", Style::default().fg(COLOR_MUTED)),
                Span::styled(bar, Style::default().fg(COLOR_SUCCESS)),
                Span::styled(format!(" {met}/{total}"), Style::default().fg(Color::White)),
            ]));
        }
        if let Some(n) = pane.deliverable_count {
            lines.push(Line::from(vec![
                Span::styled("成果物: ", Style::default().fg(COLOR_MUTED)),
                Span::styled(n.to_string(), Style::default().fg(COLOR_MEMORY)),
                Span::styled(
                    format!("  Trace {}", pane.trace_links.len()),
                    Style::default().fg(COLOR_MUTED),
                ),
            ]));
        }
        lines.push(Line::from(""));

        // DoD チェックリスト（更新直後はヘッダをハイライト）
        let dod_header_style = if recently(pane.dod_changed_at) {
            Style::default().fg(Color::Black).bg(COLOR_WARN)
        } else {
            Style::default().fg(COLOR_MUTED)
        };
        let dod_header = if pane.assessing {
            "── 完了基準 (DoD) ⟳判定中 ──"
        } else {
            "── 完了基準 (DoD) ──"
        };
        lines.push(Line::from(Span::styled(dod_header, dod_header_style)));
        if pane.dod_items.is_empty() {
            lines.push(Line::from(Span::styled(
                "（未設定 — codexと決めよう）",
                Style::default().fg(COLOR_WARN),
            )));
        } else {
            for (i, item) in pane.dod_items.iter().enumerate() {
                let (mark, style) = match pane.dod_checks.get(i).copied().flatten() {
                    Some(true) => ("[x]", Style::default().fg(COLOR_SUCCESS)),
                    Some(false) => ("[ ]", Style::default().fg(Color::Red)),
                    None => ("[ ]", Style::default().fg(COLOR_MUTED)),
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{mark} "), style),
                    Span::raw(item.as_str()),
                ]));
            }
        }

        if !pane.trace_links.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "── PR / Release ──",
                Style::default().fg(COLOR_MUTED),
            )));
            for link in &pane.trace_links {
                lines.push(Line::from(vec![
                    Span::styled("↗ ", Style::default().fg(COLOR_MEMORY)),
                    Span::styled(link.as_str(), Style::default().fg(Color::White)),
                ]));
            }
        }

        // 子ゴールのライブリスト（新着は数秒ハイライト）
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "── 子ゴール ({}) ・ コメント {} ──",
                pane.child_count.unwrap_or(0),
                pane.comment_count.unwrap_or(0)
            ),
            Style::default().fg(COLOR_MUTED),
        )));
        if pane.children.is_empty() {
            lines.push(Line::from(Span::styled(
                "（まだありません）",
                Style::default().fg(COLOR_MUTED),
            )));
        } else {
            for child in &pane.children {
                let is_new = child.new_until.is_some_and(|t| t > now);
                let title_style = if is_new {
                    Style::default().fg(Color::Black).bg(COLOR_SUCCESS)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(vec![
                    Span::raw(format!("{} ", child.icon)),
                    Span::styled(child.title.as_str(), title_style),
                ]));
            }
        }

        let contract = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(COLOR_ADDNESS))
                    .title(sync_label),
            )
            .wrap(ratatui::widgets::Wrap { trim: true });
        frame.render_widget(contract, panes[1]);

        // 下段: Addness の更新ログ（新しいものほど下。最新行は強調）
        let log_inner_h = panes[2].height.saturating_sub(2) as usize;
        let mut log_lines: Vec<Line> = if pane.activity.is_empty() {
            vec![Line::from(Span::styled(
                "codex の Addness 読込/書込がここに出ます",
                Style::default().fg(COLOR_MUTED),
            ))]
        } else {
            let n = pane.activity.len();
            pane.activity
                .iter()
                .enumerate()
                .skip(n.saturating_sub(log_inner_h.max(1)))
                .map(|(i, l)| {
                    let style = if i + 1 == n {
                        Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(l.as_str(), style))
                })
                .collect()
        };
        log_lines.truncate(log_inner_h.max(1));
        let log = Paragraph::new(log_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(COLOR_PANEL))
                    .title(" Addness メモリ "),
            )
            .wrap(ratatui::widgets::Wrap { trim: true });
        frame.render_widget(log, panes[2]);
    }

    // --- codex 端末ペイン ---
    let term_area = chunks[1];
    let rows = term_area.height.saturating_sub(2);
    let cols = term_area.width.saturating_sub(2);
    if let Some(pane) = app.codex.as_mut() {
        pane.resize(rows, cols);
        let (title, color) = if pane.finished {
            let t = if pane.scrollback > 0 {
                format!(
                    " codex 終了 ▲履歴 -{} — ↑↓/PgUp/PgDn/Home/End: ログ  Esc/qで戻る ",
                    pane.scrollback
                )
            } else {
                " codex 終了 — ↑↓: ログを遡る  [c]コメント [s]状態 [d]成果物 [v]DoD判定  Esc/q: 戻る "
                    .to_string()
            };
            (t, COLOR_SUCCESS)
        } else if pane.scrollback > 0 {
            (
                format!(
                    " codex 実行中 ▲履歴 -{} — Shift+↓/End or Esc: ライブへ戻る ",
                    pane.scrollback
                ),
                COLOR_WARN,
            )
        } else {
            (
                " codex 実行中 — F9:Addness再開  Shift+↑↓/PgUp/PgDn:ログ  F12:終了 ".to_string(),
                COLOR_CODEX,
            )
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .title(title);
        let term = PseudoTerminal::new(pane.screen()).block(block);
        frame.render_widget(term, term_area);

        // 本物の端末カーソルを codex のカーソル位置へ移動する。
        // tui-term は偽カーソルを描くだけなので、これをやらないと日本語入力時に
        // IME の変換ウィンドウが正しい位置に出ない（vt100 の桁は全角考慮済み）。
        // 履歴を遡っている間はカーソルを動かさない（過去位置に飛ぶのを防ぐ）。
        let screen = pane.screen();
        if pane.scrollback == 0 && !screen.hide_cursor() {
            let (crow, ccol) = screen.cursor_position();
            if crow < rows && ccol < cols {
                // term_area の枠（上・左の罫線）を 1 ずつオフセット。
                frame.set_cursor_position((term_area.x + 1 + ccol, term_area.y + 1 + crow));
            }
        }
    }
}

fn draw_codex_status_panel(frame: &mut Frame, area: Rect, pane: &CodexPane) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let value_width = inner_width.saturating_sub(6);
    let prompt_width = inner_width.saturating_sub(2);

    let (state, state_style) = if pane.finished {
        (
            "終了",
            Style::default()
                .fg(COLOR_SUCCESS)
                .add_modifier(Modifier::BOLD),
        )
    } else if pane.assessing {
        (
            "判定中",
            Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD),
        )
    } else if pane.action.is_some() {
        (
            "実行中",
            Style::default()
                .fg(COLOR_MEMORY)
                .add_modifier(Modifier::BOLD),
        )
    } else if pane.last_prompt().is_some() {
        (
            "対応中",
            Style::default()
                .fg(COLOR_CODEX)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            "待機",
            Style::default()
                .fg(COLOR_CODEX)
                .add_modifier(Modifier::BOLD),
        )
    };
    let work = codex_work_label(
        pane.finished,
        pane.assessing,
        pane.action.as_deref(),
        pane.last_prompt(),
        value_width,
    );
    let memory = codex_memory_label(
        pane.last_addness_read_at,
        pane.last_addness_write_at,
        value_width,
    );
    let memory_style = if pane.last_addness_write_at.is_some() {
        Style::default().fg(COLOR_SUCCESS)
    } else if pane.last_addness_read_at.is_some() {
        Style::default().fg(COLOR_MEMORY)
    } else {
        Style::default().fg(COLOR_MUTED)
    };

    let prompt = pane
        .last_prompt()
        .map(|p| prompt_preview(p, prompt_width))
        .unwrap_or_else(|| "（まだありません）".to_string());
    let prompt_style = if pane.last_prompt().is_some() {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(COLOR_MUTED)
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("状態 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(state, state_style),
        ]),
        Line::from(vec![
            Span::styled("作業 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(work, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("記憶 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(memory, memory_style),
        ]),
        Line::from(""),
        Line::from(Span::styled("最後の送信", Style::default().fg(COLOR_MUTED))),
        Line::from(Span::styled(prompt, prompt_style)),
    ];

    let panel = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_CODEX))
                .title(" Codex 現在地 "),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(panel, area);
}

fn codex_memory_label(
    last_read_at: Option<Instant>,
    last_write_at: Option<Instant>,
    max_width: usize,
) -> String {
    let label = if let Some(t) = last_write_at {
        format!("Addness書込 {}前", elapsed_compact(t))
    } else if let Some(t) = last_read_at {
        format!("Addness読込 {}前", elapsed_compact(t))
    } else {
        "Addness未読".to_string()
    };
    ellipsize_width(&label, max_width)
}

fn elapsed_compact(t: Instant) -> String {
    let secs = t.elapsed().as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m", secs / 60)
    }
}

fn codex_work_label(
    finished: bool,
    assessing: bool,
    action: Option<&str>,
    last_prompt: Option<&str>,
    max_width: usize,
) -> String {
    if finished {
        return ellipsize_width("履歴確認", max_width);
    }
    if assessing {
        return ellipsize_width("DoD自動判定", max_width);
    }
    if let Some(action) = action {
        return ellipsize_width(action, max_width);
    }
    if let Some(prompt) = last_prompt {
        return prompt_preview(&format!("依頼対応: {prompt}"), max_width);
    }
    ellipsize_width("入力待ち", max_width)
}

fn prompt_preview(prompt: &str, max_width: usize) -> String {
    let normalized = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    ellipsize_width(&normalized, max_width)
}

fn ellipsize_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let limit = max_width - 3;
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > limit {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::{codex_work_label, ellipsize_width, prompt_preview};
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn prompt_preview_collapses_whitespace() {
        assert_eq!(
            prompt_preview("  これを\n\n実行して\tください  ", 80),
            "これを 実行して ください"
        );
    }

    #[test]
    fn prompt_preview_truncates_to_width() {
        let preview = prompt_preview("abcdefghijklmnopqrstuvwxyz", 10);

        assert_eq!(preview, "abcdefg...");
        assert!(UnicodeWidthStr::width(preview.as_str()) <= 10);
    }

    #[test]
    fn ellipsize_width_respects_wide_characters() {
        let preview = ellipsize_width("日本語の長いプロンプト", 9);

        assert!(preview.ends_with("..."));
        assert!(UnicodeWidthStr::width(preview.as_str()) <= 9);
    }

    #[test]
    fn codex_work_label_prefers_action_over_last_prompt() {
        assert_eq!(
            codex_work_label(false, false, Some("ゴールを更新中"), Some("実装して"), 80),
            "ゴールを更新中"
        );
    }

    #[test]
    fn codex_work_label_uses_last_prompt_when_no_action() {
        let label = codex_work_label(false, false, None, Some("この不具合を直して"), 80);

        assert_eq!(label, "依頼対応: この不具合を直して");
    }

    #[test]
    fn codex_work_label_truncates_last_prompt() {
        let label = codex_work_label(false, false, None, Some("abcdefghijklmnopqrstuvwxyz"), 12);

        assert_eq!(label, "依頼対応:...");
        assert!(UnicodeWidthStr::width(label.as_str()) <= 12);
    }
}
