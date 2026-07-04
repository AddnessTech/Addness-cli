use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{ActivePane, App, DeliverableFormField, FormField, ModalState};
use super::codex_pane::{
    CODEX_LOG_PREFIX_WIDTH, CodexDecisionKind, CodexLogKind, CodexLogLine, CodexPane,
};
use super::goal_tree::{CommentView, TreeRow};
use crate::api::{DeliverableType, GoalStatus, Member, MemberId};

const COLOR_TEXT: Color = Color::Rgb(220, 224, 230);
const COLOR_TEXT_STRONG: Color = Color::Rgb(236, 238, 242);
const COLOR_ADDNESS: Color = Color::Rgb(128, 154, 181);
const COLOR_CODEX: Color = Color::Rgb(178, 181, 190);
const COLOR_MEMORY: Color = Color::Rgb(139, 161, 154);
const COLOR_SUCCESS: Color = Color::Rgb(137, 169, 143);
const COLOR_WARN: Color = Color::Rgb(202, 164, 91);
const COLOR_DANGER: Color = Color::Rgb(208, 112, 112);
const COLOR_MUTED: Color = Color::Rgb(112, 122, 134);
const COLOR_EVENT: Color = Color::Rgb(142, 150, 160);
const COLOR_PANEL: Color = Color::Rgb(76, 84, 96);
const COLOR_INPUT_BG: Color = Color::Rgb(27, 30, 36);
const CODEX_TOOL_COMMAND_PREVIEW_WIDTH: usize = 96;
const CODEX_EDIT_DIFF_PREVIEW_LINES: usize = 8;

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
        app.codex_terminal_area = None;
        app.codex_status_area = None;
        app.codex_contract_area = None;
        app.codex_activity_area = None;
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
        if app.codex.is_some() {
            draw_codex_help_overlay(frame);
        } else {
            draw_help_overlay(frame);
        }
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
        let running = app
            .codex
            .as_ref()
            .map(|c| c.is_turn_running())
            .unwrap_or(false);
        let hint = if finished {
            " [c]コメント  [s]状態  [d]成果物(PR/Release)  [v]DoD判定  Ctrl+?:操作一覧  Esc/q:閉じる "
        } else if running {
            " codex exec --json 実行中  |  入力+Enter:次ターン予約  |  Ctrl-O:turn  |  Ctrl-C:中断  |  Ctrl+?:操作一覧  |  F12:終了 "
        } else {
            " 入力してEnterでcodex exec --json  |  空Enter/Ctrl-O:turn  |  Trackpad/矢印:履歴  |  Ctrl+?:操作一覧  |  F12:終了 "
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
        kv(
            "Trackpad/ホイール",
            "ポインタ下のcodex/Addness枠をスクロール",
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

fn draw_codex_help_overlay(frame: &mut Frame) {
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
                format!("  {key:<18}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(desc.to_string(), Style::default().fg(Color::White)),
        ])
    };
    let blank = || Line::from("");

    let lines: Vec<Line> = vec![
        section("Codex in Addness"),
        kv("Ctrl+?", "この操作一覧を表示 / 閉じる"),
        kv("入力 + Enter", "Codexへ依頼を送信（初回は子ゴールを作成）"),
        kv("入力中 Enter", "実行中なら次ターンに予約"),
        kv("Ctrl-C", "実行中のCodexターンを中断"),
        kv("F9", "Addnessの作業メモ・決定ログから再開"),
        kv("F12", "Codexペインを終了して戻る"),
        blank(),
        section("履歴 / 検索"),
        kv(
            "Trackpad/ホイール",
            "ポインタ下のCodex履歴 / Addness枠をスクロール",
        ),
        kv("↑↓ / PgUp/PgDn", "Codex履歴をスクロール"),
        kv("Home / End", "履歴先頭 / 最新へ移動"),
        kv("Ctrl-T", "履歴表示を All / Talk / Tools / Errors で切替"),
        kv("Ctrl-F", "履歴検索を開始"),
        kv("Ctrl-L", "検索解除"),
        blank(),
        section("turn開閉"),
        kv("Alt-e", "最新または表示中のturnを開閉"),
        kv("Ctrl-O", "入力可能状態でも最新turnを開閉"),
        kv(
            "Enter / Space",
            "履歴表示中・終了後・空入力時に表示中turnを開閉",
        ),
        kv("e / E", "表示中turnを開閉 / 古いturnを一括開閉"),
        kv("Ctrl-E", "古いturnを一括開閉"),
        kv(
            "1..9 / Ctrl-1..9",
            "履歴中は数字、いつでもCtrl+数字で指定turnを開閉",
        ),
        blank(),
        section("slash commands"),
        kv("/goal <目標>", "Goal modeを開始 / 更新"),
        kv("/goal pause/resume", "Goal modeを一時停止 / 再開"),
        kv("/goal clear", "Goal modeを解除"),
        kv("/status / /help", "状態表示 / slash一覧"),
        blank(),
        section("終了後の還流"),
        kv("c / s / d / v", "コメント / 状態 / 成果物 / DoD判定"),
        kv("Esc / q", "Codexペインを閉じる"),
    ];

    let height = (lines.len() as u16 + 2).min(frame.area().height);
    let area = centered_rect(72, height, frame.area());
    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Codex in Addness ")
        .title_bottom(
            Line::from(" Ctrl+? / Esc / q: Close ").style(Style::default().fg(Color::DarkGray)),
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
/// 左に対象ゴール／DoD（契約）、右に `codex exec --json` の会話履歴を描画する。
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

        let status_panel_h = if chunks[0].height >= 30 {
            15
        } else if chunks[0].height >= 24 {
            13
        } else {
            11
        };
        let panes = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(status_panel_h),
                Constraint::Min(0),
                Constraint::Length(8),
            ])
            .split(chunks[0]);
        app.codex_status_area = Some(panes[0]);
        app.codex_contract_area = Some(panes[1]);
        app.codex_activity_area = Some(panes[2]);

        draw_codex_status_panel(frame, panes[0], pane);

        let mut lines: Vec<Line> = Vec::new();

        // 「いま参照/書込中」インジケータ（codex の操作をリアルタイム表示）
        if let Some(action) = &pane.action {
            lines.push(Line::from(Span::styled(
                format!("» {action}"),
                Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD),
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
            Style::default().fg(COLOR_TEXT)
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
                Span::styled(format!(" {met}/{total}"), Style::default().fg(COLOR_TEXT)),
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
                    Some(false) => ("[ ]", Style::default().fg(COLOR_MUTED)),
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
                    Span::styled("↗ ", Style::default().fg(COLOR_MUTED)),
                    Span::styled(link.as_str(), Style::default().fg(COLOR_TEXT)),
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
                    Style::default().fg(Color::Black).bg(COLOR_WARN)
                } else if child.is_completed {
                    Style::default()
                        .fg(COLOR_MUTED)
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default().fg(COLOR_TEXT)
                };
                let marker_style = if child.is_completed {
                    Style::default().fg(COLOR_SUCCESS)
                } else {
                    Style::default().fg(COLOR_MUTED)
                };
                let status_style = if child.is_completed {
                    Style::default().fg(COLOR_SUCCESS)
                } else if child.status_label == "進行中" {
                    Style::default().fg(COLOR_WARN)
                } else {
                    Style::default().fg(COLOR_MUTED)
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", child.icon), marker_style),
                    Span::styled(child.title.as_str(), title_style),
                    Span::styled(format!("  {}", child.status_label), status_style),
                ]));
            }
        }

        let contract_inner_h = panes[1].height.saturating_sub(2) as usize;
        let contract_inner_w = panes[1].width.saturating_sub(2) as usize;
        let max_contract_scroll =
            rendered_lines_height(&lines, contract_inner_w).saturating_sub(contract_inner_h.max(1));
        app.codex_contract_scroll = app.codex_contract_scroll.min(max_contract_scroll);
        let contract_title = sync_label;
        let contract = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(COLOR_PANEL))
                    .title(contract_title),
            )
            .scroll((app.codex_contract_scroll.min(u16::MAX as usize) as u16, 0))
            .wrap(ratatui::widgets::Wrap { trim: true });
        frame.render_widget(contract, panes[1]);

        // 下段: Addness の更新ログ（新しいものほど下。最新行は強調）
        let log_inner_h = panes[2].height.saturating_sub(2) as usize;
        let mut log_lines: Vec<Line> = if pane.activity.is_empty() {
            app.codex_activity_scroll = 0;
            vec![Line::from(Span::styled(
                "body/DoD/子ゴール/通知の読込・書込がここに出ます",
                Style::default().fg(COLOR_MUTED),
            ))]
        } else {
            let view_h = log_inner_h.max(1);
            let log_inner_w = panes[2].width.saturating_sub(2) as usize;
            let all_activity_lines = codex_activity_lines(&pane.activity, log_inner_w);
            let n = all_activity_lines.len();
            let max_activity_scroll = n.saturating_sub(view_h);
            app.codex_activity_scroll = app.codex_activity_scroll.min(max_activity_scroll);
            let end = n.saturating_sub(app.codex_activity_scroll);
            let start = end.saturating_sub(view_h);
            all_activity_lines[start..end].to_vec()
        };
        log_lines.truncate(log_inner_h.max(1));
        let log_title = " Addness 更新 ".to_string();
        let log = Paragraph::new(log_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_PANEL))
                .title(log_title),
        );
        frame.render_widget(log, panes[2]);
    }

    // --- codex 会話ペイン ---
    let term_area = chunks[1];
    app.codex_terminal_area = Some(term_area);
    let rows = term_area.height.saturating_sub(2);
    let cols = term_area.width.saturating_sub(2);
    if let Some(pane) = app.codex.as_mut() {
        pane.resize(rows, cols);
        let (title, color) = if pane.finished {
            let t = if pane.scrollback > 0 {
                " codex exec 終了 — ↑↓/PgUp/PgDn/Home/End: ログ  Esc/qで戻る ".to_string()
            } else {
                " codex exec 終了 — ↑↓: ログ  [c]コメント [s]状態 [d]成果物 [v]DoD判定  Esc/q: 戻る "
                    .to_string()
            };
            (t, COLOR_PANEL)
        } else if let Some(decision) = pane.decision_banner() {
            (codex_decision_title_hint(decision), COLOR_WARN)
        } else if pane.is_turn_running() {
            (
                " codex exec --json 実行中 — JSONLをAddnessで表示  Ctrl-C:ターン中断  F12:終了 "
                    .to_string(),
                COLOR_WARN,
            )
        } else if pane.scrollback > 0 {
            (
                " codex exec --json — Esc: ライブへ戻る ".to_string(),
                COLOR_PANEL,
            )
        } else {
            let thread = pane
                .thread_id()
                .map(|id| format!("  thread:{}", short_thread_id(id)))
                .unwrap_or_default();
            (
                format!(" codex exec --json 入力待ち{thread}  F9:Addness再開  F12:終了 "),
                COLOR_PANEL,
            )
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .title(title);
        draw_codex_exec_panel(frame, term_area, block, pane);
    }
}

fn draw_codex_exec_panel(frame: &mut Frame, area: Rect, block: Block<'_>, pane: &mut CodexPane) {
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let input_h = codex_input_panel_height(pane, inner.width, inner.height);
    let header_h = u16::from(inner.height >= 6);
    let mut constraints = Vec::new();
    if header_h > 0 {
        constraints.push(Constraint::Length(header_h));
    }
    constraints.push(Constraint::Min(0));
    constraints.push(Constraint::Length(input_h));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);
    let mut chunk_index = 0usize;
    let header_chunk = if header_h > 0 {
        let chunk = chunks[chunk_index];
        chunk_index += 1;
        Some(chunk)
    } else {
        None
    };
    let history_chunk = chunks[chunk_index];
    let input_chunk = chunks[chunk_index + 1];

    if let Some(header_chunk) = header_chunk {
        frame.render_widget(
            Paragraph::new(codex_header_line(pane, header_chunk.width as usize)),
            header_chunk,
        );
    }

    let filtered_log = pane.filtered_log_lines();
    let history_width = history_chunk.width as usize;
    let history_height = history_chunk.height as usize;
    let (history_lines, total_history_lines) = codex_visible_log_lines(
        &filtered_log,
        history_width,
        pane.scrollback,
        history_height,
    );
    pane.sync_rendered_history_metrics(total_history_lines, history_height);
    let history = if history_lines.is_empty() {
        Paragraph::new(Line::from(Span::styled(
            "入力待ち",
            Style::default().fg(COLOR_MUTED),
        )))
    } else {
        Paragraph::new(
            history_lines
                .into_iter()
                .map(|line| line.line)
                .collect::<Vec<_>>(),
        )
    };
    frame.render_widget(history, history_chunk);

    let input_width = input_chunk.width.saturating_sub(2) as usize;
    let status_width = input_chunk.width as usize;
    let status = codex_runtime_status(pane, status_width);
    let search_prefix = "  search: ";
    let mut input_cursor = None;
    let input_lines = if pane.is_search_editing() {
        let prompt = format!(
            "{search_prefix}{}",
            ellipsize_width(
                pane.search_query(),
                input_width.saturating_sub(search_prefix.len())
            )
        );
        vec![Line::from(Span::styled(
            prompt,
            Style::default().fg(COLOR_TEXT),
        ))]
    } else if let Some(decision) = pane.decision_banner() {
        codex_decision_input_lines(decision, input_width, input_chunk.height)
    } else {
        let input_style = if pane.is_turn_running() {
            Style::default().fg(COLOR_WARN)
        } else {
            Style::default().fg(COLOR_TEXT)
        };
        let prompt_lines = if pane.finished {
            vec![Line::from(Span::styled(
                "  Esc/q:戻る  c/s/d/v:還流  Enter/Space/e/1..9:turn",
                input_style,
            ))]
        } else {
            let prompt_rows = if input_chunk.height <= 1 {
                1
            } else {
                input_chunk.height.saturating_sub(1) as usize
            };
            let render = codex_input_prompt_render(
                pane.input_line(),
                pane.input_cursor(),
                input_width,
                prompt_rows,
                input_style,
            );
            let cursor_row_offset = if input_chunk.height <= 1 { 0 } else { 1 };
            input_cursor = Some((
                render.cursor_col as u16,
                cursor_row_offset + render.cursor_row as u16,
            ));
            render.lines
        };
        if input_chunk.height <= 1 {
            prompt_lines
        } else {
            let mut lines = Vec::with_capacity(prompt_lines.len() + 1);
            lines.push(Line::from(Span::styled(
                status,
                Style::default().fg(COLOR_MUTED),
            )));
            lines.extend(prompt_lines);
            lines
        }
    };
    frame.render_widget(
        Paragraph::new(input_lines).style(Style::default().bg(COLOR_INPUT_BG)),
        input_chunk,
    );

    if pane.is_search_editing() && pane.scrollback == 0 {
        let cursor_col = (UnicodeWidthStr::width(search_prefix)
            + UnicodeWidthStr::width(pane.search_query()))
        .min(input_chunk.width.saturating_sub(1) as usize) as u16;
        let cursor_row = if input_chunk.height <= 1 {
            input_chunk.y
        } else {
            input_chunk.y + 1
        };
        frame.set_cursor_position((input_chunk.x + cursor_col, cursor_row));
    } else if !pane.finished
        && pane.scrollback == 0
        && !(pane.is_turn_running() && pane.decision_banner().is_some())
        && let Some((cursor_col, cursor_row)) = input_cursor
    {
        frame.set_cursor_position((
            input_chunk.x + cursor_col.min(input_chunk.width.saturating_sub(1)),
            input_chunk.y + cursor_row.min(input_chunk.height.saturating_sub(1)),
        ));
    }
}

struct CodexInputPromptRender {
    lines: Vec<Line<'static>>,
    cursor_col: usize,
    cursor_row: usize,
}

#[derive(Clone)]
struct CodexInputVisualLine {
    text: String,
    start: usize,
    end: usize,
}

fn codex_input_panel_height(pane: &CodexPane, inner_width: u16, inner_height: u16) -> u16 {
    let base = if inner_height >= 4 { 2 } else { 1 };
    if pane.decision_banner().is_some() {
        let height = if inner_height >= 14 {
            6
        } else if inner_height >= 10 {
            5
        } else if inner_height >= 6 {
            4
        } else {
            base
        };
        return height.min(inner_height.max(1));
    }
    if pane.finished || pane.is_search_editing() || inner_height < 4 {
        return base;
    }

    let max_height: u16 = if inner_height >= 16 {
        6
    } else if inner_height >= 10 {
        4
    } else {
        2
    };
    let max_prompt_rows = max_height.saturating_sub(1).max(1) as usize;
    let prompt_width = inner_width.saturating_sub(4).max(1) as usize;
    let prompt_rows = codex_input_visual_lines(pane.input_line(), prompt_width)
        .len()
        .min(max_prompt_rows)
        .max(1) as u16;

    (prompt_rows + 1).clamp(base, max_height)
}

fn codex_input_prompt_render(
    input: &str,
    cursor: usize,
    max_width: usize,
    max_rows: usize,
    style: Style,
) -> CodexInputPromptRender {
    let text_width = max_width.saturating_sub(2).max(1);
    let visual_lines = codex_input_visual_lines(input, text_width);
    let cursor = cursor.min(input.len());
    let cursor_line_index = visual_lines
        .iter()
        .rposition(|line| cursor >= line.start && cursor <= line.end)
        .unwrap_or(0);
    let rows = max_rows.max(1);
    let start = cursor_line_index.saturating_add(1).saturating_sub(rows);
    let end = (start + rows).min(visual_lines.len());

    let mut cursor_col = 0usize;
    let mut cursor_row = 0usize;
    let lines = visual_lines[start..end]
        .iter()
        .enumerate()
        .map(|(row, line)| {
            let absolute_row = start + row;
            let prefix = if absolute_row == 0 { "> " } else { "  " };
            if absolute_row == cursor_line_index {
                cursor_col = UnicodeWidthStr::width(prefix)
                    + codex_input_text_width(&input[line.start..cursor.min(line.end)]);
                cursor_row = row;
            }
            Line::from(vec![
                Span::styled(prefix.to_string(), style),
                Span::styled(line.text.clone(), style),
            ])
        })
        .collect::<Vec<_>>();

    CodexInputPromptRender {
        lines,
        cursor_col,
        cursor_row,
    }
}

fn codex_input_visual_lines(input: &str, max_width: usize) -> Vec<CodexInputVisualLine> {
    let max_width = max_width.max(1);
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut text = String::new();
    let mut width = 0usize;

    for (idx, ch) in input.char_indices() {
        if ch == '\n' {
            lines.push(CodexInputVisualLine {
                text: std::mem::take(&mut text),
                start,
                end: idx,
            });
            start = idx + ch.len_utf8();
            width = 0;
            continue;
        }

        let ch_width = codex_input_char_width(ch);
        if width > 0 && width + ch_width > max_width {
            lines.push(CodexInputVisualLine {
                text: std::mem::take(&mut text),
                start,
                end: idx,
            });
            start = idx;
            width = 0;
        }
        text.push(ch);
        width += ch_width;
    }

    lines.push(CodexInputVisualLine {
        text,
        start,
        end: input.len(),
    });
    lines
}

fn codex_input_char_width(ch: char) -> usize {
    if ch == '\t' {
        4
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

fn codex_input_text_width(text: &str) -> usize {
    text.chars().map(codex_input_char_width).sum()
}

fn codex_decision_input_lines(
    decision: &super::codex_pane::CodexDecisionBanner,
    max_width: usize,
    input_height: u16,
) -> Vec<Line<'static>> {
    let color = codex_decision_color(&decision.kind);
    let label = codex_decision_input_label(&decision.kind);
    let label_width = UnicodeWidthStr::width(label);

    if input_height <= 1 {
        let mut choices = format!(
            "  {}  {}",
            decision_choice_text(decision, true),
            decision_choice_text(decision, false)
        );
        if let Some(always) = decision_always_choice_text(decision) {
            choices.push_str("  ");
            choices.push_str(&always);
        }
        let choices_width = UnicodeWidthStr::width(choices.as_str());
        let message_width = max_width.saturating_sub(label_width + choices_width);
        return vec![Line::from(vec![
            Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                ellipsize_width(&decision.message, message_width),
                Style::default().fg(COLOR_TEXT),
            ),
            Span::styled(
                choices,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ])];
    }

    if input_height == 2 {
        return vec![
            Line::from(vec![
                Span::styled(
                    label,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    ellipsize_width(&decision.message, max_width.saturating_sub(label_width)),
                    Style::default().fg(COLOR_TEXT_STRONG),
                ),
            ]),
            codex_decision_choice_line(decision, max_width),
        ];
    }

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            "  ? ",
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            label.trim_end().to_string(),
            Style::default()
                .fg(COLOR_TEXT_STRONG)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    let message_rows = input_height.saturating_sub(2) as usize;
    lines.extend(codex_decision_message_lines(
        decision,
        max_width,
        message_rows,
        color,
    ));
    while lines.len() + 1 < input_height as usize {
        lines.push(Line::from(""));
    }
    lines.push(codex_decision_choice_line(decision, max_width));
    lines
}

fn codex_decision_message_lines(
    decision: &super::codex_pane::CodexDecisionBanner,
    max_width: usize,
    max_lines: usize,
    color: Color,
) -> Vec<Line<'static>> {
    if max_lines == 0 {
        return Vec::new();
    }
    let subject = codex_decision_subject_label(&decision.kind);
    let text = format!("  {subject}: {}", decision.message);
    let mut parts = wrap_log_text(&text, max_width);
    let truncated = parts.len() > max_lines;
    parts.truncate(max_lines);
    if truncated && let Some(last) = parts.last_mut() {
        *last = ellipsize_width(&format!("{last}..."), max_width);
    }

    parts
        .into_iter()
        .enumerate()
        .map(|(idx, part)| {
            if idx > 0 {
                return Line::from(Span::styled(part, Style::default().fg(COLOR_TEXT_STRONG)));
            }
            let Some((head, tail)) = part.split_once(':') else {
                return Line::from(Span::styled(part, Style::default().fg(COLOR_TEXT_STRONG)));
            };
            Line::from(vec![
                Span::styled(
                    format!("{head}:"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(tail.to_string(), Style::default().fg(COLOR_TEXT_STRONG)),
            ])
        })
        .collect()
}

fn codex_decision_input_label(kind: &CodexDecisionKind) -> &'static str {
    match kind {
        CodexDecisionKind::Approval => "確認待ち: 承認 ",
        CodexDecisionKind::Permission => "確認待ち: 権限 ",
        CodexDecisionKind::Dangerous => "確認待ち: 危険操作 ",
        CodexDecisionKind::YesNo => "確認待ち ",
    }
}

fn codex_decision_subject_label(kind: &CodexDecisionKind) -> &'static str {
    match kind {
        CodexDecisionKind::Approval => "実行予定",
        CodexDecisionKind::Permission => "権限要求",
        CodexDecisionKind::Dangerous => "危険操作",
        CodexDecisionKind::YesNo => "確認内容",
    }
}

fn codex_decision_color(kind: &CodexDecisionKind) -> Color {
    match kind {
        CodexDecisionKind::Permission | CodexDecisionKind::Dangerous => COLOR_DANGER,
        CodexDecisionKind::Approval | CodexDecisionKind::YesNo => COLOR_WARN,
    }
}

fn codex_decision_title_hint(decision: &super::codex_pane::CodexDecisionBanner) -> String {
    let keys = match decision.kind {
        CodexDecisionKind::YesNo => "y/n",
        CodexDecisionKind::Dangerous => "a/y または d/n",
        CodexDecisionKind::Approval | CodexDecisionKind::Permission => {
            if decision.always_choice().is_some() {
                "a/y・d/n・l:常に許可"
            } else {
                "a/y または d/n"
            }
        }
    };
    format!(" codex exec 確認待ち — 下の確認欄で選択 / {keys} ")
}

fn codex_decision_choice_line(
    decision: &super::codex_pane::CodexDecisionBanner,
    max_width: usize,
) -> Line<'static> {
    let accept = decision_choice_text(decision, true);
    let deny = decision_choice_text(decision, false);
    let always = decision_always_choice_text(decision);
    let hint_text = if let Some(always) = always.as_deref() {
        format!("  {accept}    {deny}    {always}    キーを押すと選択")
    } else {
        format!("  {accept}    {deny}    キーを押すと選択")
    };
    let hint = ellipsize_width(&hint_text, max_width);
    let accept_len = UnicodeWidthStr::width(accept.as_str());
    let deny_len = UnicodeWidthStr::width(deny.as_str());
    let always_len = always
        .as_deref()
        .map(UnicodeWidthStr::width)
        .unwrap_or_default();
    let accept_style = Style::default()
        .fg(Color::Black)
        .bg(COLOR_SUCCESS)
        .add_modifier(Modifier::BOLD);
    let deny_style = Style::default()
        .fg(COLOR_TEXT_STRONG)
        .bg(if matches!(decision.kind, CodexDecisionKind::YesNo) {
            COLOR_PANEL
        } else {
            COLOR_DANGER
        })
        .add_modifier(Modifier::BOLD);
    let always_style = Style::default()
        .fg(COLOR_WARN)
        .bg(COLOR_PANEL)
        .add_modifier(Modifier::BOLD);

    let mut spans = vec![
        Span::styled("  ", Style::default()),
        Span::styled(accept, accept_style),
        Span::styled("    ", Style::default()),
        Span::styled(deny, deny_style),
    ];
    let mut suffix_start = 2 + accept_len + 4 + deny_len;
    if let Some(always) = always {
        spans.push(Span::styled("    ", Style::default()));
        spans.push(Span::styled(always, always_style));
        suffix_start += 4 + always_len;
    }
    if UnicodeWidthStr::width(hint.as_str()) > suffix_start {
        let suffix = hint.chars().skip(suffix_start).collect::<String>();
        spans.push(Span::styled(suffix, Style::default().fg(COLOR_MUTED)));
    }
    Line::from(spans)
}

fn decision_choice_text(
    decision: &super::codex_pane::CodexDecisionBanner,
    is_accept: bool,
) -> String {
    let (key, label) = if is_accept {
        (decision.accept_key, decision.accept_label)
    } else {
        (decision.deny_key, decision.deny_label)
    };
    let keys = match (&decision.kind, is_accept) {
        (CodexDecisionKind::YesNo, _) => key.to_ascii_uppercase().to_string(),
        (_, true) => format!("{}/Y", key.to_ascii_uppercase()),
        (_, false) => format!("{}/N", key.to_ascii_uppercase()),
    };
    format!("[{keys}] {label}")
}

fn decision_always_choice_text(
    decision: &super::codex_pane::CodexDecisionBanner,
) -> Option<String> {
    decision
        .always_choice()
        .map(|(key, label)| format!("[{}] {label}", key.to_ascii_uppercase()))
}

fn codex_header_line(pane: &CodexPane, max_width: usize) -> Line<'static> {
    let run_state = pane.run_state();
    let state = run_state.code();
    let search = if pane.search_query().is_empty() {
        if pane.is_search_editing() {
            "search:input".to_string()
        } else {
            "search:off".to_string()
        }
    } else if pane.is_search_editing() {
        format!("search:{}*", pane.search_query())
    } else {
        format!("search:{}", pane.search_query())
    };
    let thread = pane
        .thread_id()
        .map(short_thread_id)
        .unwrap_or_else(|| "new".to_string());
    let text = format!(
        " {state} {} | Turn {} | fold:{} | filter:{} | {search} | thread:{thread} | {} | Ctrl-T/F/L/O e/E 1..9",
        run_state.label(),
        pane.turn_count(),
        pane.collapsed_turn_count(),
        pane.log_filter_label(),
        pane.history_label()
    );
    let style = if pane.decision_banner().is_some() || pane.is_turn_running() {
        Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(COLOR_MUTED)
    };
    Line::from(Span::styled(ellipsize_width(&text, max_width), style))
}

fn rendered_lines_height(lines: &[Line<'_>], width: usize) -> usize {
    lines
        .iter()
        .map(|line| rendered_text_height(&line_text_plain(line), width))
        .sum()
}

fn rendered_text_height(text: &str, width: usize) -> usize {
    let width = width.max(1);
    let mut total = 0usize;
    for segment in text.replace('\r', "").split('\n') {
        if segment.is_empty() {
            total += 1;
        } else {
            let segment_width = UnicodeWidthStr::width(segment);
            total += segment_width.saturating_add(width - 1) / width;
        }
    }
    total.max(1)
}

fn line_text_plain(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn codex_activity_lines(activity: &[String], width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let last_index = activity.len().saturating_sub(1);
    let mut out = Vec::new();
    for (idx, text) in activity.iter().enumerate() {
        let style = if idx == last_index {
            Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(COLOR_TEXT)
        };
        for part in wrap_log_text(text, width) {
            out.push(Line::from(Span::styled(part, style)));
        }
    }
    out
}

#[derive(Clone)]
struct RenderedCodexLine {
    line: Line<'static>,
}

fn codex_log_lines(lines: &[&CodexLogLine], max_width: usize) -> Vec<RenderedCodexLine> {
    lines
        .iter()
        .flat_map(|line| {
            let mut rendered = Vec::new();
            if matches!(line.kind, CodexLogKind::Turn | CodexLogKind::Tool) {
                rendered.push(codex_separator_line(line.kind, max_width));
            }
            rendered.extend(codex_log_entry_lines(line, max_width));
            rendered
        })
        .collect()
}

fn codex_visible_log_lines(
    lines: &[&CodexLogLine],
    max_width: usize,
    scrollback: usize,
    viewport_height: usize,
) -> (Vec<RenderedCodexLine>, usize) {
    if lines.is_empty() || viewport_height == 0 {
        return (Vec::new(), 0);
    }

    let counts = lines
        .iter()
        .map(|line| codex_log_line_rendered_count(line, max_width))
        .collect::<Vec<_>>();
    let total = counts.iter().sum::<usize>();
    let max_scrollback = total.saturating_sub(viewport_height);
    let scrollback = scrollback.min(max_scrollback);
    let view_end = total.saturating_sub(scrollback);
    let view_start = view_end.saturating_sub(viewport_height);

    let mut offset = 0usize;
    let mut selected = Vec::new();
    let mut selected_start = None;
    for (line, count) in lines.iter().zip(counts.iter().copied()) {
        let next = offset.saturating_add(count);
        if next > view_start && offset < view_end {
            selected_start.get_or_insert(offset);
            selected.push(*line);
        }
        offset = next;
    }

    let selected_start = selected_start.unwrap_or(view_start);
    let skip = view_start.saturating_sub(selected_start);
    let take = view_end.saturating_sub(view_start);
    let visible = codex_log_lines(&selected, max_width)
        .into_iter()
        .skip(skip)
        .take(take)
        .collect();
    (visible, total)
}

fn codex_log_line_rendered_count(line: &CodexLogLine, max_width: usize) -> usize {
    let separator = usize::from(matches!(line.kind, CodexLogKind::Turn | CodexLogKind::Tool));
    let content_width = max_width.saturating_sub(CODEX_LOG_PREFIX_WIDTH).max(1);
    separator + wrapped_log_line_count(&codex_log_display_text(line), content_width)
}

fn codex_separator_line(kind: CodexLogKind, max_width: usize) -> RenderedCodexLine {
    let (head, fill, color) = match kind {
        CodexLogKind::Turn => ("-----+ ", '-', COLOR_PANEL),
        CodexLogKind::Tool => (".....+ ", '.', COLOR_PANEL),
        _ => ("     | ", '-', COLOR_PANEL),
    };
    let fill_width = max_width
        .saturating_sub(UnicodeWidthStr::width(head))
        .max(1);
    RenderedCodexLine {
        line: Line::from(vec![
            Span::styled(head, Style::default().fg(color)),
            Span::styled(
                fill.to_string().repeat(fill_width),
                Style::default().fg(color),
            ),
        ]),
    }
}

fn codex_log_entry_lines(line: &CodexLogLine, max_width: usize) -> Vec<RenderedCodexLine> {
    let (prefix, prefix_style, text_style) = codex_log_prefix(line);
    let continuation = "     | ";
    let content_width = max_width.saturating_sub(CODEX_LOG_PREFIX_WIDTH).max(1);
    let text = codex_log_display_text(line);
    let is_edit_tool = matches!(line.kind, CodexLogKind::Tool) && line.text.starts_with("EDIT ");
    let wrapped = wrap_log_text(&text, content_width);
    let mut lines = Vec::with_capacity(wrapped.len().max(1));
    for (idx, part) in wrapped.into_iter().enumerate() {
        let prefix_text = if idx == 0 { prefix } else { continuation };
        let style = if idx == 0 {
            prefix_style
        } else {
            Style::default().fg(COLOR_PANEL)
        };
        let part_style = if is_edit_tool {
            codex_edit_diff_style(&part, text_style)
        } else {
            text_style
        };
        let mut spans = vec![Span::styled(prefix_text, style)];
        spans.extend(codex_log_content_spans(
            line.kind,
            part,
            part_style,
            prefix_style,
        ));
        lines.push(RenderedCodexLine {
            line: Line::from(spans),
        });
    }
    lines
}

fn codex_log_content_spans(
    kind: CodexLogKind,
    part: String,
    text_style: Style,
    marker_style: Style,
) -> Vec<Span<'static>> {
    if kind == CodexLogKind::Tool
        && let Some(rest) = part.strip_prefix("• ")
    {
        return vec![
            Span::styled("•", marker_style),
            Span::styled(format!(" {rest}"), text_style),
        ];
    }
    vec![Span::styled(part, text_style)]
}

fn codex_edit_diff_style(part: &str, fallback: Style) -> Style {
    let trimmed = part.trim_start();
    if trimmed.starts_with('+') {
        Style::default()
            .fg(COLOR_SUCCESS)
            .add_modifier(Modifier::BOLD)
    } else if trimmed.starts_with('-') {
        Style::default()
            .fg(COLOR_DANGER)
            .add_modifier(Modifier::BOLD)
    } else if trimmed.starts_with("@@") {
        Style::default().fg(COLOR_MUTED)
    } else if trimmed.starts_with("update:")
        || trimmed.starts_with("add:")
        || trimmed.starts_with("delete:")
        || trimmed.starts_with("move:")
        || trimmed.ends_with("files changed")
    {
        Style::default()
            .fg(COLOR_TEXT_STRONG)
            .add_modifier(Modifier::BOLD)
    } else {
        fallback
    }
}

fn codex_log_prefix(line: &CodexLogLine) -> (&'static str, Style, Style) {
    match line.kind {
        CodexLogKind::User => (
            "YOU  | ",
            Style::default()
                .fg(COLOR_ADDNESS)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        ),
        CodexLogKind::Assistant => (
            "CODEX| ",
            Style::default()
                .fg(COLOR_TEXT_STRONG)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        ),
        CodexLogKind::Tool => codex_tool_prefix(&line.text),
        CodexLogKind::Turn => (
            "TURN | ",
            Style::default()
                .fg(COLOR_CODEX)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD),
        ),
        CodexLogKind::System => (
            "INFO | ",
            Style::default().fg(COLOR_MUTED),
            Style::default().fg(COLOR_MUTED),
        ),
        CodexLogKind::Error => (
            "ERR! | ",
            Style::default()
                .fg(COLOR_DANGER)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_DANGER),
        ),
        CodexLogKind::Event => (
            "     | ",
            Style::default().fg(COLOR_EVENT),
            Style::default().fg(COLOR_EVENT),
        ),
    }
}

fn codex_tool_prefix(text: &str) -> (&'static str, Style, Style) {
    if text.starts_with("EDIT ") {
        (
            "EDIT | ",
            Style::default()
                .fg(COLOR_CODEX)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        )
    } else if text.starts_with("DIFF ") {
        (
            "DIFF | ",
            Style::default()
                .fg(COLOR_MUTED)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        )
    } else if text.starts_with("FAIL ") || text.contains("exit ") && !text.contains("exit 0") {
        (
            "FAIL | ",
            Style::default()
                .fg(COLOR_DANGER)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_DANGER),
        )
    } else if text.starts_with("OK ") || text.contains("exit 0") {
        (
            "OK   | ",
            Style::default()
                .fg(COLOR_SUCCESS)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        )
    } else if text.contains("output_delta") || text.contains('\n') {
        (
            "OUT  | ",
            Style::default().fg(COLOR_MUTED),
            Style::default().fg(COLOR_TEXT),
        )
    } else if text.starts_with("RUNNING ") {
        (
            "RUN  | ",
            Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        )
    } else {
        (
            "RUN  | ",
            Style::default().fg(COLOR_MUTED),
            Style::default().fg(COLOR_TEXT),
        )
    }
}

fn codex_log_display_text(line: &CodexLogLine) -> String {
    let text = line.text.replace('\r', "");
    if !matches!(line.kind, CodexLogKind::Tool) {
        return text;
    }
    summarize_tool_display_text(strip_tool_event_prefix(&text))
}

fn strip_tool_event_prefix(text: &str) -> &str {
    let Some((head, tail)) = text.split_once(": ") else {
        return text;
    };
    let head = head.to_ascii_lowercase();
    if head.contains("exec")
        || head.contains("tool")
        || head.contains("function")
        || head.contains("mcp")
        || head.contains("apply_patch")
        || head.contains("shell")
    {
        tail
    } else {
        text
    }
}

fn wrap_log_text(text: &str, max_width: usize) -> Vec<String> {
    let max_width = max_width.max(1);
    let normalized = text.replace('\r', "");
    let mut out = Vec::new();
    for segment in normalized.split('\n') {
        if segment.is_empty() {
            out.push(String::new());
            continue;
        }

        let mut line = String::new();
        let mut width = 0usize;
        for ch in segment.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width > 0 && width + ch_width > max_width {
                out.push(line);
                line = String::new();
                width = 0;
            }
            line.push(ch);
            width += ch_width;
        }
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn wrapped_log_line_count(text: &str, max_width: usize) -> usize {
    let max_width = max_width.max(1);
    let normalized = text.replace('\r', "");
    let mut total = 0usize;
    for segment in normalized.split('\n') {
        if segment.is_empty() {
            total += 1;
            continue;
        }

        let mut width = 0usize;
        let mut count = 1usize;
        for ch in segment.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width > 0 && width + ch_width > max_width {
                count += 1;
                width = 0;
            }
            width += ch_width;
        }
        total += count;
    }
    total.max(1)
}

fn summarize_tool_display_text(text: &str) -> String {
    let normalized = text.replace('\r', "");
    if let Some(summary) = code_edit_display_text(&normalized) {
        return summary;
    }
    let (state, normalized) = split_tool_state_prefix(&normalized);
    let Some((head, tail)) = normalized.split_once('\n') else {
        return tool_command_tree_head(state, normalized.trim());
    };
    let output = tail.trim();
    let head_line = tool_command_tree_head(state, head.trim());
    if output.is_empty() {
        return head_line;
    }

    if let Some(summary) = special_tool_summary(head, output) {
        return format!("{head_line}\n  └ {summary}");
    }

    let preview = tool_output_tree_preview(output);
    format!("{head_line}\n  └ {preview}")
}

fn split_tool_state_prefix(text: &str) -> (Option<&str>, &str) {
    for state in ["RUNNING", "OK", "FAIL", "DIFF"] {
        if let Some(rest) = text.strip_prefix(state)
            && rest.chars().next().is_some_and(char::is_whitespace)
        {
            return (Some(state), rest.trim_start());
        }
    }
    (None, text)
}

fn tool_command_tree_head(state: Option<&str>, command: &str) -> String {
    let verb = if state == Some("RUNNING") {
        "Running"
    } else {
        "Ran"
    };
    let command = ellipsize_width(command, CODEX_TOOL_COMMAND_PREVIEW_WIDTH);
    format!("• {verb} {command}")
}

fn tool_output_tree_preview(output: &str) -> String {
    match non_empty_line_count(output) {
        0 => "output: empty".to_string(),
        1 => "output: 1 line".to_string(),
        n => format!("output: {n} lines"),
    }
}

fn non_empty_line_count(text: &str) -> usize {
    text.lines().filter(|line| !line.trim().is_empty()).count()
}

fn compact_command_result(kind: &str, output: &str) -> Option<String> {
    let lower = output.to_ascii_lowercase();
    if output.lines().any(|line| line.contains("Finished ")) || lower.trim() == "ok" {
        return Some(format!("{kind}: ok"));
    }
    if output
        .lines()
        .any(|line| line.contains("test result:") && line.to_ascii_lowercase().contains(" ok."))
    {
        return Some(format!("{kind}: ok"));
    }
    if lower.contains("fail") || lower.contains("error") {
        return Some(format!("{kind}: failed"));
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeEditChange {
    action: &'static str,
    path: String,
}

fn code_edit_display_text(text: &str) -> Option<String> {
    if !looks_like_code_edit_display_text(text) {
        return None;
    }
    let changes = code_edit_changes(text);
    if changes.is_empty() {
        let first_line = text.lines().next().unwrap_or("code edit").trim();
        let title = first_line.strip_prefix("EDIT ").unwrap_or(first_line);
        let title = if title.is_empty() || title.contains("*** Begin Patch") {
            "code edit"
        } else {
            title
        };
        return Some(format!("{title}\n  code edit"));
    }

    let first = changes.first()?;
    let title = if changes.len() == 1 {
        format!("{}: {}", first.action, first.path)
    } else {
        format!("{} files changed", changes.len())
    };
    let mut lines = vec![title];
    if changes.len() > 1 {
        for change in changes.iter().take(3) {
            lines.push(format!("  {}: {}", change.action, change.path));
        }
        let omitted = changes.len().saturating_sub(3);
        if omitted > 0 {
            lines.push(format!("  ... +{omitted} more"));
        }
    }
    lines.extend(code_edit_diff_preview(text, CODEX_EDIT_DIFF_PREVIEW_LINES));
    Some(lines.join("\n"))
}

fn looks_like_code_edit_display_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.starts_with("edit ")
        || lower.contains("apply_patch")
        || text.contains("*** Begin Patch")
        || text.contains("*** Update File:")
        || text.contains("*** Add File:")
        || text.contains("*** Delete File:")
}

fn code_edit_changes(text: &str) -> Vec<CodeEditChange> {
    let mut changes = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let change = [
            ("*** Update File: ", "update"),
            ("*** Add File: ", "add"),
            ("*** Delete File: ", "delete"),
            ("*** Move to: ", "move"),
        ]
        .into_iter()
        .find_map(|(prefix, action)| {
            trimmed.strip_prefix(prefix).map(|path| CodeEditChange {
                action,
                path: path.to_string(),
            })
        });

        if let Some(change) = change
            && !changes
                .iter()
                .any(|existing: &CodeEditChange| existing == &change)
        {
            changes.push(change);
        }
    }
    changes
}

fn code_edit_diff_preview(text: &str, max_lines: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut omitted = 0usize;

    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("***") {
            continue;
        }
        let is_diff_line =
            trimmed.starts_with("@@") || trimmed.starts_with('+') || trimmed.starts_with('-');
        if !is_diff_line {
            continue;
        }
        if out.len() >= max_lines {
            omitted += 1;
            continue;
        }
        out.push(ellipsize_width(trimmed, 140));
    }

    if omitted > 0 {
        out.push(format!("... +{omitted} diff lines"));
    }
    out
}

fn special_tool_summary(head: &str, output: &str) -> Option<String> {
    let lower = head.to_ascii_lowercase();

    if lower.contains("cargo test") {
        return compact_command_result("tests", output);
    }

    if lower.contains("cargo clippy") {
        return compact_command_result("clippy", output);
    }

    if lower.contains("cargo build") {
        return compact_command_result("build", output);
    }

    if lower.contains("git diff") || lower.contains("git status") {
        if output.trim().is_empty() {
            return Some("git: no output".to_string());
        }
        return Some(format!("git: {} lines", non_empty_line_count(output)));
    }

    if looks_like_addness_command(head)
        && let Some(summary) = json_output_summary(output)
    {
        return Some(format!("addness: {summary}"));
    }

    None
}

fn looks_like_addness_command(head: &str) -> bool {
    let lower = head.to_ascii_lowercase();
    lower.contains("addness")
        || lower.contains("$addness_bin")
        || lower.contains(" goal get ")
        || lower.contains(" goal list ")
        || lower.contains(" goal children ")
        || lower.contains(" comment list ")
        || lower.contains(" deliverable ")
}

fn json_output_summary(output: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(output.trim()).ok()?;
    if value.get("goal").is_some() || value.get("title").is_some() {
        return Some("goal".to_string());
    }
    match value {
        Value::Array(items) => Some(format!("json array {} items", items.len())),
        Value::Object(map) => Some(format!("json object {} keys", map.len())),
        _ => Some("json value".to_string()),
    }
}

fn codex_runtime_status(pane: &CodexPane, max_width: usize) -> String {
    let state = if pane.finished {
        "DONE"
    } else if pane.is_turn_running() {
        "RUNNING"
    } else {
        "READY"
    };
    let history = pane.history_label();
    let fixed_width = UnicodeWidthStr::width(state)
        .saturating_add(UnicodeWidthStr::width(history.as_str()))
        .saturating_add(8);
    let detail = if pane.is_turn_running() {
        let queued = pane.queued_prompt_count();
        let input_hint = if queued > 0 {
            format!(" / 予約{queued}件")
        } else {
            " / Enterで次ターン予約".to_string()
        };
        if let Some(command) = pane.current_command() {
            ellipsize_width(
                &format!("● 実行中: {command}{input_hint}"),
                max_width.saturating_sub(fixed_width),
            )
        } else {
            ellipsize_width(
                &format!("● Codex応答中{input_hint}"),
                max_width.saturating_sub(fixed_width),
            )
        }
    } else {
        codex_work_label(
            pane.finished,
            pane.assessing,
            pane.action.as_deref(),
            pane.last_prompt(),
            max_width.saturating_sub(fixed_width),
        )
    };
    ellipsize_width(&format!("  {state}  {detail}  |  {history}"), max_width)
}

fn short_thread_id(id: &str) -> String {
    if id.chars().count() <= 8 {
        return id.to_string();
    }
    id.chars().take(8).collect()
}

fn draw_codex_status_panel(frame: &mut Frame, area: Rect, pane: &CodexPane) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let value_width = inner_width.saturating_sub(8);
    let prompt_width = inner_width.saturating_sub(2);

    let run_state = pane.run_state();
    let state_style = match run_state {
        super::codex_pane::CodexRunState::Completed => {
            Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD)
        }
        super::codex_pane::CodexRunState::Confirming => {
            Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
        }
        super::codex_pane::CodexRunState::CommandRunning
        | super::codex_pane::CodexRunState::Thinking => {
            Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
        }
        super::codex_pane::CodexRunState::InputWaiting => {
            Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD)
        }
    };
    let work = if let Some(decision) = pane.decision_banner() {
        ellipsize_width(&format!("確認: {}", decision.message), value_width)
    } else {
        codex_work_label(
            pane.finished,
            pane.assessing,
            pane.action.as_deref(),
            pane.last_prompt(),
            value_width,
        )
    };
    let command = pane
        .current_command()
        .map(|command| {
            let elapsed = pane
                .current_command_elapsed_secs()
                .map(|secs| format!(" {secs}s"))
                .unwrap_or_default();
            ellipsize_width(&format!("{command}{elapsed}"), value_width)
        })
        .unwrap_or_else(|| {
            if pane.is_turn_running() {
                "Codex応答中".to_string()
            } else {
                "なし".to_string()
            }
        });
    let command_style = if pane.current_command().is_some() || pane.is_turn_running() {
        Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(COLOR_MUTED)
    };
    let memory = codex_memory_label(
        pane.last_addness_read_at,
        pane.last_addness_write_at,
        pane.last_addness_read_label.as_deref(),
        pane.last_addness_write_label.as_deref(),
        value_width,
    );
    let memory_style =
        if pane.last_addness_write_at.is_some() || pane.last_addness_read_at.is_some() {
            Style::default().fg(COLOR_ADDNESS)
        } else {
            Style::default().fg(COLOR_MUTED)
        };
    let history = ellipsize_width(
        &format!(
            "{} / 折畳{}",
            pane.history_label(),
            pane.collapsed_turn_count()
        ),
        value_width,
    );
    let goal_mode = pane
        .goal_mode_label()
        .map(|label| ellipsize_width(&label, value_width))
        .unwrap_or_else(|| "off".to_string());
    let goal_mode_style = if pane.goal_mode_label().is_some() {
        Style::default().fg(COLOR_ADDNESS)
    } else {
        Style::default().fg(COLOR_MUTED)
    };
    let assistant = pane
        .last_assistant_text()
        .map(|p| prompt_preview(p, prompt_width))
        .unwrap_or_else(|| "（まだありません）".to_string());
    let assistant_style = if pane.last_assistant_text().is_some() {
        Style::default().fg(COLOR_TEXT)
    } else {
        Style::default().fg(COLOR_MUTED)
    };

    let prompt = pane
        .last_prompt()
        .map(|p| prompt_preview(p, prompt_width))
        .unwrap_or_else(|| "（まだありません）".to_string());
    let prompt_style = if pane.last_prompt().is_some() {
        Style::default().fg(COLOR_TEXT)
    } else {
        Style::default().fg(COLOR_MUTED)
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("状態 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(run_state.label(), state_style),
            Span::styled(
                format!("  Turn {}", pane.turn_count()),
                Style::default().fg(COLOR_MUTED),
            ),
        ]),
        Line::from(vec![
            Span::styled("作業 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(work, Style::default().fg(COLOR_TEXT)),
        ]),
        Line::from(vec![
            Span::styled("コマンド ", Style::default().fg(COLOR_MUTED)),
            Span::styled(command, command_style),
        ]),
        Line::from(vec![
            Span::styled("記憶 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(memory, memory_style),
        ]),
        Line::from(vec![
            Span::styled("履歴 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(history, Style::default().fg(COLOR_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("Goal ", Style::default().fg(COLOR_MUTED)),
            Span::styled(goal_mode, goal_mode_style),
        ]),
        Line::from(vec![
            Span::styled("Codex ", Style::default().fg(COLOR_MUTED)),
            Span::styled(assistant, assistant_style),
        ]),
    ];
    lines.push(Line::from(""));
    lines.extend(codex_dashboard_shortcut_lines(prompt_width));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "最後の送信",
        Style::default().fg(COLOR_MUTED),
    )));
    lines.push(Line::from(Span::styled(prompt, prompt_style)));

    let panel = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(
                    if pane.is_turn_running() || pane.decision_banner().is_some() {
                        COLOR_WARN
                    } else {
                        COLOR_PANEL
                    },
                ))
                .title(if pane.decision_banner().is_some() {
                    " Codex 作業ダッシュボード ▲確認待ち "
                } else if pane.is_turn_running() {
                    " Codex 作業ダッシュボード ●実行中 "
                } else {
                    " Codex 作業ダッシュボード "
                }),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(panel, area);
}

fn codex_dashboard_shortcut_lines(max_width: usize) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "操作一覧",
            Style::default()
                .fg(COLOR_TEXT_STRONG)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            ellipsize_width("  /goal:目標  Ctrl+?:全体", max_width),
            Style::default().fg(COLOR_TEXT),
        )),
        Line::from(Span::styled(
            ellipsize_width("  Ctrl-O:turn  ↑↓/PgUp:履歴", max_width),
            Style::default().fg(COLOR_MUTED),
        )),
    ]
}

fn codex_memory_label(
    last_read_at: Option<Instant>,
    last_write_at: Option<Instant>,
    last_read_label: Option<&str>,
    last_write_label: Option<&str>,
    max_width: usize,
) -> String {
    let label = if let Some(t) = last_write_at {
        format!(
            "{} {}前",
            last_write_label.unwrap_or("Addness書込"),
            elapsed_compact(t)
        )
    } else if let Some(t) = last_read_at {
        format!(
            "{} {}前",
            last_read_label.unwrap_or("Addness読込"),
            elapsed_compact(t)
        )
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
    use super::{
        ActivePane, App, COLOR_DANGER, COLOR_EVENT, COLOR_SUCCESS, COLOR_WARN,
        codex_activity_lines, codex_dashboard_shortcut_lines, codex_decision_choice_line,
        codex_decision_input_lines, codex_decision_title_hint, codex_header_line,
        codex_input_prompt_render, codex_log_entry_lines, codex_log_lines, codex_runtime_status,
        codex_visible_log_lines, codex_work_label, draw_status_bar, ellipsize_width,
        prompt_preview, summarize_tool_display_text,
    };
    use crate::api::ApiClient;
    use crate::tui::codex_pane::{
        CODEX_LOG_PREFIX_WIDTH, CodexDecisionBanner, CodexDecisionKind, CodexLogKind, CodexLogLine,
        CodexPane,
    };
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Modifier;
    use ratatui::text::Line;
    use ratatui::{Terminal, backend::TestBackend};
    use unicode_width::UnicodeWidthStr;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn render_status_text(app: &App, width: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(width, 3)).unwrap();
        term.draw(|frame| draw_status_bar(frame, frame.area(), app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

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
    fn codex_log_entry_lines_wraps_assistant_response() {
        let entry = CodexLogLine {
            kind: CodexLogKind::Assistant,
            text: "abcdef".to_string(),
        };

        let lines = codex_log_entry_lines(&entry, CODEX_LOG_PREFIX_WIDTH + 3);

        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0].line), "CODEX| abc");
        assert_eq!(line_text(&lines[1].line), "     | def");
    }

    #[test]
    fn codex_log_entry_lines_marks_failed_tool_and_strips_event_noise() {
        let entry = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "exec_command_end (exit 1): cargo test\nfailed".to_string(),
        };

        let lines = codex_log_entry_lines(&entry, 80);

        assert_eq!(line_text(&lines[0].line), "FAIL | • Ran cargo test");
        assert_eq!(line_text(&lines[1].line), "     |   └ tests: failed");
        assert_eq!(lines[0].line.spans[1].content.as_ref(), "•");
        assert_eq!(lines[0].line.spans[1].style.fg, Some(COLOR_DANGER));
    }

    #[test]
    fn codex_log_entry_lines_colors_tool_bullet_by_state() {
        let running = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "RUNNING cargo test".to_string(),
        };
        let running_lines = codex_log_entry_lines(&running, 80);
        assert_eq!(running_lines[0].line.spans[1].content.as_ref(), "•");
        assert_eq!(running_lines[0].line.spans[1].style.fg, Some(COLOR_WARN));

        let ok = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "exec_command_end (exit 0): cargo test\nok".to_string(),
        };
        let ok_lines = codex_log_entry_lines(&ok, 80);
        assert_eq!(ok_lines[0].line.spans[1].content.as_ref(), "•");
        assert_eq!(ok_lines[0].line.spans[1].style.fg, Some(COLOR_SUCCESS));
    }

    #[test]
    fn codex_log_entry_lines_omits_large_tool_output_preview() {
        let entry = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: format!(
                "exec_command_end (exit 0): curl https://example.test\n{}",
                "0123456789 ".repeat(40)
            ),
        };

        let lines = codex_log_entry_lines(&entry, 240);
        let rendered = lines
            .iter()
            .map(|line| line_text(&line.line))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("OK   | • Ran curl https://example.test"));
        assert!(rendered.contains("└ output: 1 line"));
        assert!(!rendered.contains("└ 0123456789"));
        assert!(!rendered.contains(&"0123456789 ".repeat(25)));
    }

    #[test]
    fn codex_log_entry_lines_marks_code_edits_like_codex() {
        let entry = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "EDIT *** Begin Patch\n*** Update File: src/tui/ui.rs\n@@\n-old line\n+new line\n*** End Patch".to_string(),
        };

        let lines = codex_log_entry_lines(&entry, 80);

        assert_eq!(line_text(&lines[0].line), "EDIT | update: src/tui/ui.rs");
        assert_eq!(line_text(&lines[1].line), "     | @@");
        assert_eq!(line_text(&lines[2].line), "     | -old line");
        assert_eq!(line_text(&lines[3].line), "     | +new line");
        assert_eq!(lines[2].line.spans[1].style.fg, Some(COLOR_DANGER));
        assert_eq!(lines[3].line.spans[1].style.fg, Some(COLOR_SUCCESS));
        assert!(
            !lines[2].line.spans[1]
                .style
                .add_modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn summarize_tool_display_text_highlights_cargo_test_result() {
        let text = summarize_tool_display_text("cargo test\ntest result: ok. 86 passed; 0 failed;");

        assert_eq!(text, "• Ran cargo test\n  └ tests: ok");
    }

    #[test]
    fn summarize_tool_display_text_uses_codex_like_tree_output() {
        let text = summarize_tool_display_text(
            "cargo fmt -- --check\nDiff in /repo/src/tui/ui.rs:3163:\n-old\n+new",
        );

        assert_eq!(text, "• Ran cargo fmt -- --check\n  └ output: 3 lines");
    }

    #[test]
    fn summarize_tool_display_text_highlights_addness_json_title() {
        let text = summarize_tool_display_text(
            r#"addness goal get goal-1 --json
{"id":"goal-1","title":"AddnessTUI改善"}"#,
        );

        assert_eq!(
            text,
            "• Ran addness goal get goal-1 --json\n  └ addness: goal"
        );
    }

    #[test]
    fn codex_header_line_shows_filter_and_search_state() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.cycle_log_filter();
        pane.begin_search();
        assert!(pane.handle_search_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)));

        let text = line_text(&codex_header_line(&pane, 200));

        assert!(text.contains("filter:Talk"));
        assert!(text.contains("search:c*"));
        assert!(text.contains("履歴"));
    }

    #[test]
    fn codex_input_prompt_render_preserves_explicit_newlines() {
        let render = codex_input_prompt_render(
            "first\nsecond",
            "first\nsecond".len(),
            40,
            4,
            ratatui::style::Style::default(),
        );
        let lines = render.lines.iter().map(line_text).collect::<Vec<_>>();

        assert_eq!(lines, vec!["> first", "  second"]);
        assert_eq!(render.cursor_row, 1);
        assert_eq!(render.cursor_col, 8);
    }

    #[test]
    fn codex_input_prompt_render_keeps_cursor_line_visible_for_long_input() {
        let input = "abcdefghijklmnopqrstuvwxyz";
        let render =
            codex_input_prompt_render(input, input.len(), 10, 1, ratatui::style::Style::default());
        let text = line_text(&render.lines[0]);

        assert!(text.contains("yz"));
        assert!(!text.contains("abc"));
        assert_eq!(render.cursor_row, 0);
        assert!(render.cursor_col <= 9);
    }

    #[test]
    fn codex_decision_input_lines_use_clear_yes_no_choices() {
        let decision = CodexDecisionBanner {
            kind: CodexDecisionKind::YesNo,
            message: "続行しますか?".to_string(),
            accept_key: 'y',
            accept_label: "Yes",
            deny_key: 'n',
            deny_label: "No",
        };

        let lines = codex_decision_input_lines(&decision, 80, 2)
            .into_iter()
            .map(|line| line_text(&line))
            .collect::<Vec<_>>();

        assert_eq!(lines[0], "確認待ち 続行しますか?");
        assert!(lines[1].contains("[Y] Yes"));
        assert!(lines[1].contains("[N] No"));
        assert!(!lines[1].contains("常に許可"));
    }

    #[test]
    fn codex_decision_input_lines_use_bottom_question_area() {
        let decision = CodexDecisionBanner {
            kind: CodexDecisionKind::Approval,
            message: "cargo test --workspace".to_string(),
            accept_key: 'a',
            accept_label: "承認",
            deny_key: 'd',
            deny_label: "拒否",
        };

        let rendered = codex_decision_input_lines(&decision, 80, 5)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("? 確認待ち: 承認"));
        assert!(rendered.contains("実行予定: cargo test --workspace"));
        assert!(rendered.contains("[A/Y] 承認"));
        assert!(rendered.contains("[D/N] 拒否"));
        assert!(rendered.contains("[L] 常に許可"));
    }

    #[test]
    fn codex_decision_title_hint_mentions_always_allow_only_when_available() {
        let approval = CodexDecisionBanner {
            kind: CodexDecisionKind::Approval,
            message: "コマンドを実行しますか?".to_string(),
            accept_key: 'a',
            accept_label: "承認",
            deny_key: 'd',
            deny_label: "拒否",
        };
        let yes_no = CodexDecisionBanner {
            kind: CodexDecisionKind::YesNo,
            message: "続行しますか?".to_string(),
            accept_key: 'y',
            accept_label: "Yes",
            deny_key: 'n',
            deny_label: "No",
        };

        assert!(codex_decision_title_hint(&approval).contains("l:常に許可"));
        assert!(!codex_decision_title_hint(&yes_no).contains("常に許可"));
        assert!(codex_decision_title_hint(&yes_no).contains("y/n"));
        assert!(codex_decision_title_hint(&approval).contains("下の確認欄"));
        assert!(!codex_decision_title_hint(&approval).contains("上部で内容確認"));
    }

    #[test]
    fn codex_decision_input_lines_keep_prompt_in_two_rows() {
        let decision = CodexDecisionBanner {
            kind: CodexDecisionKind::Approval,
            message: "Run cargo test?".to_string(),
            accept_key: 'a',
            accept_label: "承認",
            deny_key: 'd',
            deny_label: "拒否",
        };

        let lines = codex_decision_input_lines(&decision, 80, 2);

        assert_eq!(line_text(&lines[0]), "確認待ち: 承認 Run cargo test?");
        assert!(line_text(&lines[1]).contains("[A/Y] 承認"));
        assert!(line_text(&lines[1]).contains("[D/N] 拒否"));
        assert!(line_text(&lines[1]).contains("[L] 常に許可"));
    }

    #[test]
    fn codex_decision_input_panel_uses_bottom_question_format() {
        let decision = CodexDecisionBanner {
            kind: CodexDecisionKind::Approval,
            message: "Run cargo test?".to_string(),
            accept_key: 'a',
            accept_label: "承認",
            deny_key: 'd',
            deny_label: "拒否",
        };

        let lines = codex_decision_input_lines(&decision, 80, 5);

        assert!(line_text(&lines[0]).contains("? 確認待ち: 承認"));
        assert!(line_text(&lines[1]).contains("実行予定: Run cargo test?"));
        assert!(line_text(lines.last().unwrap()).contains("[A/Y] 承認"));
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn compact_codex_decision_input_line_keeps_choices() {
        let decision = CodexDecisionBanner {
            kind: CodexDecisionKind::Permission,
            message: "Need filesystem permission".to_string(),
            accept_key: 'a',
            accept_label: "許可",
            deny_key: 'd',
            deny_label: "拒否",
        };

        let lines = codex_decision_input_lines(&decision, 80, 1);
        let text = line_text(&lines[0]);

        assert!(text.contains("確認待ち: 権限"));
        assert!(text.contains("[A/Y] 許可"));
        assert!(text.contains("[D/N] 拒否"));
        assert!(text.contains("[L] 常に許可"));
        assert_eq!(lines[0].spans[0].style.fg, Some(COLOR_DANGER));
    }

    #[test]
    fn dangerous_codex_decision_omits_always_allow_choice() {
        let decision = CodexDecisionBanner {
            kind: CodexDecisionKind::Dangerous,
            message: "delete files?".to_string(),
            accept_key: 'a',
            accept_label: "許可",
            deny_key: 'd',
            deny_label: "拒否",
        };

        let text = line_text(&codex_decision_choice_line(&decision, 80));

        assert!(text.contains("[A/Y] 許可"));
        assert!(text.contains("[D/N] 拒否"));
        assert!(!text.contains("常に許可"));
    }

    #[test]
    fn command_output_lines_stay_readable() {
        let entry = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "exec_command_end (exit 0): cargo test\nok".to_string(),
        };
        let lines = codex_log_entry_lines(&entry, 80);

        assert!(
            !lines[0].line.spans[0]
                .style
                .add_modifier
                .contains(Modifier::DIM)
        );
        assert!(
            !lines[1].line.spans[0]
                .style
                .add_modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn event_notice_lines_stay_readable() {
        let entry = CodexLogLine {
            kind: CodexLogKind::Event,
            text: "waiting for approval".to_string(),
        };
        let lines = codex_log_entry_lines(&entry, 80);

        assert!(
            !lines[0].line.spans[0]
                .style
                .add_modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn event_log_lines_use_unlabeled_muted_prefix() {
        let entry = CodexLogLine {
            kind: CodexLogKind::Event,
            text: "waiting for approval".to_string(),
        };
        let lines = codex_log_entry_lines(&entry, 80);
        let spans = &lines[0].line.spans;

        assert!(line_text(&lines[0].line).starts_with("     | "));
        assert!(!line_text(&lines[0].line).starts_with("EVT | "));
        assert!(!line_text(&lines[0].line).starts_with("INFO | "));
        assert_ne!(spans[0].style.fg, Some(COLOR_WARN));
        assert_ne!(spans[1].style.fg, Some(COLOR_WARN));
        assert_eq!(spans[0].style.fg, Some(COLOR_EVENT));
        assert!(!spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(spans[1].style.fg, Some(COLOR_EVENT));
    }

    #[test]
    fn codex_log_lines_adds_turn_and_tool_separators() {
        let turn = CodexLogLine {
            kind: CodexLogKind::Turn,
            text: "Turn 1".to_string(),
        };
        let tool = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "exec_command_begin: cargo test".to_string(),
        };
        let lines = codex_log_lines(&[&turn, &tool], 24)
            .into_iter()
            .map(|line| line_text(&line.line))
            .collect::<Vec<_>>();

        assert!(lines.iter().any(|line| line.starts_with("-----+ ")));
        assert!(lines.iter().any(|line| line.starts_with(".....+ ")));
    }

    #[test]
    fn codex_visible_log_lines_matches_full_render_window() {
        let entries = (0..30)
            .map(|i| CodexLogLine {
                kind: CodexLogKind::Assistant,
                text: format!("line {i}"),
            })
            .collect::<Vec<_>>();
        let refs = entries.iter().collect::<Vec<_>>();
        let full = codex_log_lines(&refs, 40);
        let (visible, total) = codex_visible_log_lines(&refs, 40, 3, 5);

        assert_eq!(total, full.len());
        let expected = full[full.len() - 8..full.len() - 3]
            .iter()
            .map(|line| line_text(&line.line))
            .collect::<Vec<_>>();
        let actual = visible
            .iter()
            .map(|line| line_text(&line.line))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn codex_activity_lines_wraps_long_update_rows() {
        let lines = codex_activity_lines(&["abcdefghijklmnopqrstuvwxyz".to_string()], 10);

        assert!(lines.len() >= 3);
        assert_eq!(line_text(&lines[0]), "abcdefghij");
    }

    #[test]
    fn codex_runtime_status_shows_current_action() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.action = Some("ゴール文脈を読込中".to_string());

        let status = codex_runtime_status(&pane, 80);

        assert!(status.contains("READY"));
        assert!(status.contains("ゴール文脈を読込中"));
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

    #[test]
    fn codex_status_bar_hides_scroll_diagnostic_without_last_label() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let mut app = App::new(client, rt.handle().clone());
        app.active_pane = ActivePane::Codex;
        app.codex_last_scroll_input = Some("mouse ScrollUp -> codex 0->3".to_string());

        let text = render_status_text(&app, 80);

        assert!(
            !text.contains("mouse ScrollUp -> codex 0->3"),
            "status should not expose release-noisy scroll diagnostics:\n{text}"
        );
        assert!(
            !text.contains("操作:"),
            "status should not expose operation diagnostics:\n{text}"
        );
        assert!(
            !text.contains("last:"),
            "status should not show last label:\n{text}"
        );
    }

    #[test]
    fn codex_status_bar_advertises_ctrl_help_overlay() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = ApiClient::new("t", "http://localhost").unwrap();
        let mut app = App::new(client, rt.handle().clone());
        app.active_pane = ActivePane::Codex;
        app.codex = Some(CodexPane::test_with_output(10, 80, 0, ""));

        let text = render_status_text(&app, 140);

        assert!(
            text.contains("Ctrl+?"),
            "codex status should expose the help shortcut:\n{text}"
        );
    }

    #[test]
    fn codex_dashboard_shortcuts_show_help_entrypoint() {
        let text = codex_dashboard_shortcut_lines(80)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("操作一覧"));
        assert!(text.contains("Ctrl+?"));
        assert!(text.contains("/goal"));
        assert!(text.contains("Ctrl-O"));
    }
}
