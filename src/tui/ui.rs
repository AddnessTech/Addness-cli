use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::agent::{
    AgentKind, CODEX_LOG_PREFIX_WIDTH, ChildGoal, CodexDecisionKind, CodexListPickerAction,
    CodexLogKind, CodexLogLine, CodexPane,
};
use super::app::{ActivePane, App, DeliverableFormField, FormField, ModalState};
use super::goal_tree::{CommentView, TreeRow};
use super::markdown::{self, MarkdownStyles};
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
const CODEX_TOOL_COMMAND_PREVIEW_WIDTH: usize = 56;
/// @メンションのファイル候補パレットで一度に表示する最大行数。
const MENTION_PALETTE_VISIBLE_ROWS: usize = 10;
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
        app.codex_status_scroll = 0;
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
            draw_codex_help_overlay(frame, app);
        } else {
            draw_help_overlay(frame, app);
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
                format!(" {} ", pane.kind().label()),
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
        let kind = app
            .codex
            .as_ref()
            .map(|c| c.kind())
            .unwrap_or(AgentKind::Codex);
        let name = kind.display_name();
        let finished = app.codex.as_ref().map(|c| c.finished).unwrap_or(true);
        let running = app
            .codex
            .as_ref()
            .map(|c| c.is_turn_running())
            .unwrap_or(false);
        let hint = if finished {
            " [c]コメント  [s]状態  [d]成果物(PR/Release)  [v]DoD判定  ?/Ctrl+Q:操作一覧  Esc/q:閉じる ".to_string()
        } else if running {
            format!(
                " {name} 実行中  |  Ctrl-T:表示切替  |  F7:turn一覧  |  入力+Enter:次ターン予約  |  Ctrl-C:中断 "
            )
        } else {
            let fkeys = if kind == AgentKind::ClaudeCode {
                "F2-F4:設定  |  F6:差分"
            } else {
                "F2-F6:設定/差分"
            };
            format!(
                " 入力してEnterで{name}へ送信  |  Ctrl-T:表示切替  |  F7:turn一覧  |  {fkeys}  |  ?/Ctrl+Q:操作一覧 "
            )
        };
        let status = Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(COLOR_CODEX),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_CODEX))
                .title(format!(" {} ", kind.label())),
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

fn draw_help_overlay(frame: &mut Frame, app: &mut App) {
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
            "codexで作業 / claude codeで作業 / 完了・再開 / コメント追加 / 成果物追加 / 編集 / 削除",
        ),
        kv(
            "コメント上",
            "返信 / 解決・未解決 / 編集 / 削除 / リアクション",
        ),
        blank(),
        section("codex連携 (o →「codexで作業」)"),
        kv("起動", "選択ゴールの文脈付きでcodexをペイン起動"),
        kv(
            "claude code連携",
            "o →「claude codeで作業」で同様にペイン起動（権限はF4=permission-modeで切替。F5のsandboxは使わない）",
        ),
        kv("F2 / F3", "モデル / 推論強度を切替"),
        kv("F4 / F5", "承認モード / sandboxを切替"),
        kv("F6", "作業ツリーのdiffビューを表示 / 戻る"),
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

    render_scrollable_overlay(
        frame,
        &mut app.help_scroll,
        lines,
        64,
        " Keybindings ",
        "? / Esc / q: Close",
    );
}

fn draw_codex_help_overlay(frame: &mut Frame, app: &mut App) {
    let kind = app
        .codex
        .as_ref()
        .map(|c| c.kind())
        .unwrap_or(AgentKind::Codex);
    let name = kind.display_name();
    let is_claude = kind == AgentKind::ClaudeCode;

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

    let mut lines: Vec<Line> = vec![
        section(&format!("{name} in Addness")),
        kv(
            "? / Ctrl+Q",
            "この操作一覧を表示 / 閉じる（? は入力欄が空のとき）",
        ),
        kv(
            "入力 + Enter",
            &format!("{name}へ依頼を送信（必要ならAddnessを参照）"),
        ),
        kv("入力中 Enter", "実行中なら次ターンに予約"),
        kv("Ctrl-C", &format!("実行中の{name}ターンを中断")),
    ];
    if is_claude {
        lines.push(kv("F2 / F3", "次ターンのモデル / effortを切替"));
        lines.push(kv("F4", "次ターンのpermission-modeを切替（F5は未対応）"));
    } else {
        lines.push(kv("F2 / F3", "次ターンのモデル / 推論強度を切替"));
        lines.push(kv("F4 / F5", "承認モード / sandboxを切替"));
    }
    lines.extend([
        kv("F6", "ファイル編集diffビューを表示 / ログへ戻る"),
        kv("F7", "turn一覧パネルを開く"),
        kv("F9", "Addnessの作業メモ・決定ログから再開"),
        kv("F12", &format!("{name}ペインを終了して戻る")),
        blank(),
        section("履歴 / 検索"),
        kv(
            "Trackpad/ホイール",
            &format!("ポインタ下の{name}履歴 / Addness枠をスクロール"),
        ),
        kv("↑↓ / PgUp/PgDn", &format!("{name}履歴をスクロール")),
        kv(
            "Ctrl+↑ / Ctrl+↓",
            &format!("入力中でも{name}履歴を1行スクロール（↑は履歴呼び戻し）"),
        ),
        kv("Home / End", "履歴先頭 / 最新へ移動"),
        kv("Ctrl-T", "表示を 会話 / 実行 / 失敗 / 全部 で切替"),
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
            "/turn <N>",
            "会話中でも指定turnを展開（close/toggle/all対応）",
        ),
        kv(
            "turn一覧",
            "Enter/o=展開 c=格納 Space=開閉 a=全展開 Esc=閉じる",
        ),
        blank(),
    ]);

    if is_claude {
        lines.extend([
            section("claude code commands"),
            kv(
                "/ 入力",
                "コマンド候補を入力欄の上に表示（Tab補完 / ↑↓選択）",
            ),
            kv("/sessions [N]", "Claude Codeセッション候補を一覧から選択"),
            kv(
                "/resume [N|id]",
                "セッションを一覧から選んで再開（f: fork）",
            ),
            kv("/resume-memo", "Addnessの作業メモ・決定ログから続きを再開"),
            kv("/resume-last [prompt]", "最新セッションを --resume で継続"),
            kv("/resume-last-all", "cwd外も含めて最新セッションを継続"),
            kv(
                "/resume-session <N|id> [prompt]",
                "番号またはidのセッションを継続",
            ),
            kv(
                "/resume-session-all",
                "cwd外も含めて番号またはidのセッションを継続",
            ),
            kv("/fork-last [prompt]", "最新セッションをforkして継続"),
            kv(
                "/fork-session <N|id> [prompt]",
                "番号またはidのセッションをfork",
            ),
            kv("/new / /clear", "新しいセッション開始 / 表示ログをクリア"),
            kv("/init", "AGENTS.md を作成 / 更新する初期化依頼を送信"),
            kv("/ide", "IDE連携コンテキストの利用可否を表示"),
            kv("/compact / /plan", "会話圧縮 / 実装前計画を送信"),
            kv("/skills", "ローカルskill一覧 / skill使用依頼"),
            kv("/exec|/e <prompt>", "Goal modeを通さず直接送信"),
            blank(),
        ]);
    } else {
        lines.extend([
            section("codex commands"),
            kv(
                "/ 入力",
                "コマンド候補を入力欄の上に表示（Tab補完 / ↑↓選択）",
            ),
            kv("/sessions [N]", "Codex session候補を一覧から選択"),
            kv(
                "/resume [N|id]",
                "セッションを一覧から選んで再開（f: fork）",
            ),
            kv("/resume-memo", "Addnessの作業メモ・決定ログから続きを再開"),
            kv(
                "/codex-resume <args>",
                "root codex resumeをno-alt-screenで実行",
            ),
            kv(
                "/resume-last",
                "最新Codex sessionをexec resume --lastで継続",
            ),
            kv("/resume-last-all", "cwd外も含めて最新Codexセッションを継続"),
            kv("/resume-session <N>", "番号またはidのCodexセッションを継続"),
            kv(
                "/resume-session-all",
                "cwd外も含めて番号またはidのセッションを継続",
            ),
            kv(
                "/resume-interactive-*",
                "root codex resumeをno-alt-screenで実行",
            ),
            kv("/side [prompt]", "現在セッションから別会話を開始"),
            kv("/fork <args>", "root codex forkをno-alt-screenで実行"),
            kv(
                "/fork-last / /fork-session",
                "最新または指定セッションをfork",
            ),
            kv("/fork-last-all", "cwd外も含めて最新Codexセッションをfork"),
            kv(
                "/fork-session-all",
                "cwd外も含めて番号またはidのセッションをfork",
            ),
            kv("/rename <title>", "現在のCodexセッション名を変更"),
            kv(
                "/archive/unarchive/delete",
                "番号またはidのセッションを管理",
            ),
            kv(
                "/codex <args>",
                "任意のCodexサブコマンドをTUIログに実行表示",
            ),
            kv(
                "/new / /clear",
                "新しいCodexセッション開始 / 表示ログをクリア",
            ),
            kv("/init", "AGENTS.md を作成 / 更新する初期化依頼を送信"),
            kv("/ide", "IDE連携コンテキストの利用可否を表示"),
            kv("/compact / /plan", "会話圧縮 / 実装前計画をCodexへ送信"),
            kv("/import", "Claude Code等の設定候補を検出 / AGENTS化依頼"),
            kv("/hooks", "Codex hook設定のoverrideを表示・設定"),
            kv("/skills", "ローカルCodex skill一覧 / skill使用依頼"),
            kv("/exec|/e <prompt>", "Goal modeを通さずCodexへ直接送信"),
            kv(
                "/interactive [prompt]",
                "root codex [PROMPT]をno-alt-screenで実行",
            ),
            kv(
                "/doctor / /features",
                "codex doctor / features list等を実行",
            ),
            kv("/mcp / /plugin", "MCP / plugin の一覧・管理コマンドを実行"),
            kv(
                "/apps",
                "Codex Desktop/app-server/remote controlの入口を表示",
            ),
            kv("/cloud", "Codex Cloud taskのlist/status/apply/diff等を実行"),
            kv("/login / /logout", "ログイン状態確認 / ログアウト"),
            kv("/version / /update", "codex --version / update を実行"),
            kv("/app / /app-server", "Codex Desktop起動 / app-server管理"),
            kv("/remote-control", "app-server remote control をstart/stop"),
            kv("/debug / /completion", "debug出力 / shell completion生成"),
            kv(
                "/mcp-server / /exec-server",
                "server系コマンド（空ならhelp表示）",
            ),
            kv("/sandbox-run", "Codex sandbox command を実行"),
            kv("/review <args>", "codex review を実行"),
            kv("/exec-review", "Codex reviewを機械判定向けに実行"),
            kv("/apply|/a <task_id>", "codex apply <task_id> を実行"),
            blank(),
        ]);
    }

    lines.extend([
        section("goal helpers"),
        kv("/goal <目標>", "Goal modeを開始 / 更新"),
        kv("/goal pause/resume", "Goal modeを一時停止 / 再開"),
        kv("/goal clear", "Goal modeを解除"),
        kv(
            "/organize [task]",
            "Addness子ゴールへ分解し、最初の実装単位へ進む",
        ),
        kv(
            "/remember <内容>",
            "プロジェクト固有メモをAddnessの作業メモへ保存",
        ),
        kv("/handoff [メモ]", "現在の会話をAddnessへ再開用に保存"),
        blank(),
    ]);

    if is_claude {
        lines.extend([
            section("claude code options for next turn"),
            kv("/settings", "モデル・effort・permission-mode設定を表示"),
            kv("/cd <dir>", "次の新規セッションの作業ルートを変更"),
            kv(
                "/model [name]",
                "モデルを一覧から選択 / 任意のmodel名を直接指定",
            ),
            kv(
                "/reasoning|/effort [level]",
                "effortを一覧から選択 / low, medium等を直接指定",
            ),
            kv(
                "/permissions|/approval [mode]",
                "permission-modeを一覧から選択または直接指定",
            ),
            kv("/add-dir <path>", "追加の書込許可ディレクトリを渡す"),
            kv(
                "/image <path>",
                "画像を次ターンへ添付（list/clear/remove N）",
            ),
            kv("/attachments", "画像添付と追加dirを一覧・追加・クリア"),
            blank(),
        ]);
    } else {
        lines.extend([
            section("codex options for next turn"),
            kv("/settings", "モデル・推論・承認・sandbox設定を表示"),
            kv("/cd <dir>", "次の新規Codexセッションの作業ルートを変更"),
            kv(
                "/model [name]",
                "モデルを一覧から選択 / 任意のmodel名を直接指定",
            ),
            kv(
                "/reasoning [effort]",
                "推論強度を一覧から選択 / low, medium等を直接指定",
            ),
            kv(
                "/approval|/approvals / /sandbox",
                "承認モード / sandboxを一覧から選択または直接指定",
            ),
            kv("/permissions", "承認/sandbox権限を表示・変更"),
            kv(
                "/personality",
                "friendly / pragmatic の通信スタイルを次ターンへ適用",
            ),
            kv(
                "/statusline",
                "通常Codexのstatus line項目・色設定を次ターンへ適用",
            ),
            kv(
                "/theme / /pets",
                "通常Codexのtheme / pet設定を次ターンへ適用",
            ),
            kv(
                "/vim / /raw",
                "Vim composer / raw output設定を次ターンへ適用",
            ),
            kv("/keymap", "通常Codex keymap overrideを次ターンへ適用"),
            kv("/memories", "Addness DB固定。作業メモは/rememberで保存"),
            kv("/color", "Codex実行の色設定を never / auto / always に変更"),
            kv("/search / /oss", "web search / OSS provider modeを切替"),
            kv("/remote", "remote app server接続先を指定 / clear"),
            kv(
                "/remote-auth-token-env",
                "remote認証tokenの環境変数名を指定 / clear",
            ),
            kv("/no-alt-screen", "Codexのno-alt-screenを切替"),
            kv("/local-provider", "config / lmstudio / ollama を切替・指定"),
            kv("/profile <name>", "Codex profileを次ターンへ適用"),
            kv(
                "/image <path>",
                "画像を次ターンへ添付（list/clear/remove N）",
            ),
            kv("/add-dir <path>", "追加の書込許可ディレクトリを渡す"),
            kv("/attachments", "画像添付と追加dirを一覧・追加・クリア"),
            kv(
                "/sandbox-add-read-dir",
                "通常Codexのread-dir指定を--add-dirとして渡す",
            ),
            kv(
                "/setup-default-sandbox",
                "workspace-write/on-requestのsandboxプリセット",
            ),
            kv("/config key=value", "任意のCodex config overrideを追加"),
            kv("/enable / /disable", "feature flagを有効化 / 無効化"),
            kv("/strict-config", "未知のconfig項目をCodexエラーとして扱う"),
            kv(
                "/ignore-* / /ephemeral",
                "user config・rules・git check・履歴保存を切替",
            ),
            kv(
                "/bypass / /bypass-hook-trust",
                "承認/sandbox回避・hook trust回避を切替",
            ),
            kv(
                "/output-schema",
                "最終応答JSON Schemaファイルを指定 / clear",
            ),
            kv(
                "/output-last-message",
                "最終メッセージ出力先ファイルを指定 / clear",
            ),
            blank(),
        ]);
    }

    lines.extend([
        section("tui helpers"),
        kv("/diff / /history", "diffビュー / セッション履歴を表示"),
        kv(
            "/rollout / /debug-config",
            "履歴パス / 現在config診断を表示",
        ),
        kv("/ps / /stop", "実行中turnと予約を表示 / 中断"),
        kv("/feedback", "送信用の診断情報ドラフトをログに表示"),
        kv("/test-approval", "承認UIの確認用リクエストを表示"),
        kv("/btw", "最後のassistant応答をMarkdownとして表示"),
        kv("/resume", "F9と同じ再開プロンプトを送信"),
        kv("/status / /usage / /help", "状態 / token使用量 / slash一覧"),
        blank(),
        section("終了後の還流"),
        kv("c / s / d / v", "コメント / 状態 / 成果物 / DoD判定"),
        kv("Esc / q", &format!("{name}ペインを閉じる")),
    ]);

    let title = format!(" {name} in Addness ");
    render_scrollable_overlay(
        frame,
        &mut app.help_scroll,
        lines,
        72,
        &title,
        "Ctrl+Q / Esc / q: Close",
    );
}

/// スクロール可能なオーバーレイを画面中央に描く共通ヘルパー。
/// `scroll` は実際の行数・枠内高さに応じてクランプし直し、呼び出し元の
/// 状態（`App::help_scroll` 等）へ書き戻す。フッターにスクロール位置を表示する。
fn render_scrollable_overlay(
    frame: &mut Frame,
    scroll: &mut usize,
    lines: Vec<Line<'static>>,
    percent_x: u16,
    title: &str,
    footer_hint: &str,
) {
    let total = lines.len();
    let full = frame.area();
    // 内容が少なければ内容に合わせて縮め、多ければ画面高（上下に少し余白）まで
    // 広げてスクロールで見せる。
    let max_height = full.height.saturating_sub(2).max(3);
    let height = (total as u16 + 2).min(max_height);
    let area = centered_rect(percent_x, height, full);
    clear_modal_area(frame, area);

    let inner_height = area.height.saturating_sub(2) as usize;
    let max_scroll = total.saturating_sub(inner_height);
    *scroll = (*scroll).min(max_scroll);

    let footer = if max_scroll > 0 {
        let shown_end = (*scroll + inner_height).min(total);
        format!(
            " {footer_hint}  |  ↑↓/PgUp/PgDn/Home/End:スクロール  {}-{}/{total} ",
            scroll.saturating_add(1).min(total.max(1)),
            shown_end
        )
    } else {
        format!(" {footer_hint} ")
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title.to_string())
        .title_bottom(Line::from(footer).style(Style::default().fg(Color::DarkGray)));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((*scroll as u16, 0)),
        area,
    );
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

fn bottom_rect(percent_x: u16, height: u16, bottom_margin: u16, area: Rect) -> Rect {
    let popup_width = area.width * percent_x / 100;
    let height = height.min(area.height);
    let bottom_margin = bottom_margin.min(area.height.saturating_sub(height));
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + area.height.saturating_sub(height + bottom_margin);
    Rect::new(x, y, popup_width, height)
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

fn draw_destructive_confirm_panel(
    frame: &mut Frame,
    action: &str,
    subject_label: &str,
    subject: &str,
    detail: &str,
    selected: usize,
) {
    let area = bottom_rect(76, 8, 3, frame.area());
    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_DANGER))
        .title(format!(" 確認待ち: {action} "))
        .title_bottom(
            Line::from(" n: キャンセル | y: 今回だけ | a: これからずっと許可 | ←→: 選択 | Enter ")
                .style(Style::default().fg(COLOR_MUTED)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "? ",
                Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                action.to_string(),
                Style::default()
                    .fg(COLOR_TEXT_STRONG)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{subject_label}: "),
                Style::default().fg(COLOR_WARN),
            ),
            Span::styled(
                ellipsize_width(
                    subject,
                    layout[1]
                        .width
                        .saturating_sub((UnicodeWidthStr::width(subject_label) + 2) as u16)
                        as usize,
                ),
                Style::default().fg(COLOR_TEXT_STRONG),
            ),
        ])),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("実行内容: ", Style::default().fg(COLOR_WARN)),
            Span::styled(
                ellipsize_width(detail, layout[2].width.saturating_sub(10) as usize),
                Style::default().fg(COLOR_TEXT),
            ),
        ])),
        layout[2],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "! この操作は取り消せません。",
            Style::default()
                .fg(COLOR_DANGER)
                .add_modifier(Modifier::BOLD),
        ))),
        layout[3],
    );
    draw_confirm_buttons(
        frame,
        layout[4],
        selected,
        "N キャンセル",
        "Y 今回だけ削除",
        "A これからずっと許可",
    );
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

    draw_destructive_confirm_panel(
        frame,
        "ゴール削除",
        "ゴール",
        goal_title,
        "このゴールを削除します。",
        *confirm_index,
    );
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

    draw_destructive_confirm_panel(
        frame,
        "成果物削除",
        "成果物",
        deliverable_name,
        "この成果物を削除します。",
        *confirm_index,
    );
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

    draw_destructive_confirm_panel(
        frame,
        "コメント削除",
        "コメント",
        excerpt,
        "このコメントを削除します。",
        *confirm_index,
    );
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
    always_label: &str,
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
    let always_style = if selected == 2 {
        Style::default()
            .fg(Color::Black)
            .bg(COLOR_WARN)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(COLOR_WARN)
    };

    let buttons = Paragraph::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(format!(" [ {cancel_label} ] "), cancel_style),
        Span::raw("    "),
        Span::styled(format!(" [ {confirm_label} ] "), confirm_style),
        Span::raw("    "),
        Span::styled(format!(" [ {always_label} ] "), always_style),
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
/// 左に対象ゴール／DoD（契約）、右に Codex の会話履歴を描画する。
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
            12
        } else if chunks[0].height >= 22 {
            10
        } else {
            8
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

        draw_codex_status_panel(frame, panes[0], pane, &mut app.codex_status_scroll);

        let mut lines: Vec<Line> = Vec::new();

        // 「いま参照/書込中」インジケータ（codex の操作をリアルタイム表示）。
        // ターン内の作業インジケータを優先し、なければ設定変更等の恒常メッセージを見せる。
        if let Some(action) = pane.work_action().or_else(|| pane.status_note()) {
            lines.push(Line::from(Span::styled(
                format!("» {action}"),
                Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        }
        if let Some(active) = pane.active_work_package_label() {
            lines.push(Line::from(vec![
                Span::styled("作業単位: ", Style::default().fg(COLOR_MUTED)),
                Span::styled(
                    active,
                    Style::default()
                        .fg(COLOR_ADDNESS)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
        }
        if let Some(queue) = pane.queued_work_package_label() {
            lines.push(Line::from(vec![
                Span::styled("待機: ", Style::default().fg(COLOR_MUTED)),
                Span::styled(queue, Style::default().fg(COLOR_WARN)),
            ]));
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

        // 子ゴールは作業分解の入口なので、DoD/PRより前に出して初期表示から見えるようにする。
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
            for (idx, child) in pane.children.iter().enumerate() {
                lines.extend(codex_child_goal_lines(
                    child,
                    now,
                    idx + 1,
                    pane.child_goal_is_active(child),
                ));
            }
        }

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
                format!("（未設定 — {}と決めよう）", pane.kind().label()),
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
        let name = pane.kind().display_name();
        let (title, color) = if pane.finished {
            let t = if pane.scrollback > 0 {
                format!(" {name} 終了 — ↑↓/PgUp/PgDn/Home/End: 履歴  Esc/qで戻る ")
            } else {
                format!(
                    " {name} 終了 — ↑↓: 履歴  [c]コメント [s]状態 [d]成果物 [v]DoD判定  Esc/q: 戻る "
                )
            };
            (t, COLOR_PANEL)
        } else if let Some(decision) = pane.decision_banner() {
            (codex_decision_title_hint(pane.kind(), decision), COLOR_WARN)
        } else if pane.diff_view().is_some() {
            (
                format!(" {name} 差分表示 — F6:会話へ戻る  ↑↓/PgUp/PgDn/Home/End:差分 "),
                COLOR_CODEX,
            )
        } else if pane.is_turn_running() {
            (
                format!(
                    " {name} 実行中 — F7:turn一覧  Ctrl-T:表示切替  F6:差分  Ctrl+↑↓:スクロール  Ctrl-C:中断 "
                ),
                COLOR_WARN,
            )
        } else if pane.scrollback > 0 {
            (format!(" {name} 履歴表示 — Esc: 最新へ戻る "), COLOR_PANEL)
        } else {
            let fkeys = if pane.kind() == AgentKind::ClaudeCode {
                "F2-F4:設定  F6:差分"
            } else {
                "F2-F6:設定/差分"
            };
            (
                format!(
                    " {name} 入力待ち — F7:turn一覧  Ctrl-T:表示切替  Ctrl+↑↓:スクロール  {fkeys}  F9:再開 "
                ),
                COLOR_PANEL,
            )
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .title(title);
        draw_codex_exec_panel(frame, term_area, block, pane);
        if pane.turn_picker_open() {
            draw_codex_turn_picker(frame, term_area, pane);
        }
        if pane.list_picker_open() {
            draw_codex_list_picker(frame, term_area, pane);
        }
    }
}

/// 汎用リストピッカー（モデル・reasoning・approval・sandbox・セッション選択）の
/// ボトム中央モーダル。turn ピッカーと同じ配置・配色で、選択行に `>`、現在値に `*` を出す。
fn draw_codex_list_picker(frame: &mut Frame, area: Rect, pane: &CodexPane) {
    let Some(picker) = pane.list_picker() else {
        return;
    };
    let height = (picker.items.len() as u16 + 2).clamp(5, area.height.saturating_sub(2).max(5));
    let picker_area = bottom_rect(82, height, 2, area);
    clear_modal_area(frame, picker_area);

    let hints = if picker.action == CodexListPickerAction::ResumeSession {
        " ↑↓/jk: 選択 | Enter: 確定 | f: fork | Esc/q: 閉じる "
    } else {
        " ↑↓/jk: 選択 | Enter: 確定 | Esc/q: 閉じる "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_CODEX))
        .title(picker.title.clone())
        .title_bottom(Line::from(hints).style(Style::default().fg(COLOR_MUTED)));
    let inner = block.inner(picker_area);
    frame.render_widget(block, picker_area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // 候補が表示高さを超える場合は、選択行が見えるようにウィンドウをずらす。
    let visible = inner.height as usize;
    let start = picker
        .selected
        .saturating_sub(visible.saturating_sub(1))
        .min(picker.items.len().saturating_sub(visible));
    let mut lines = Vec::new();
    for (index, item) in picker.items.iter().enumerate().skip(start).take(visible) {
        let selected = index == picker.selected;
        let marker = if selected { ">" } else { " " };
        let current = if item.current { "*" } else { " " };
        let style = if selected {
            Style::default()
                .fg(COLOR_TEXT_STRONG)
                .add_modifier(Modifier::BOLD)
        } else if item.current {
            Style::default().fg(COLOR_SUCCESS)
        } else {
            Style::default().fg(COLOR_TEXT)
        };
        let text = if item.detail.is_empty() {
            format!("{marker} [{current}] {}", item.label)
        } else {
            format!("{marker} [{current}] {}  {}", item.label, item.detail)
        };
        lines.push(Line::from(Span::styled(
            ellipsize_width(&text, inner.width as usize),
            style,
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_codex_turn_picker(frame: &mut Frame, area: Rect, pane: &CodexPane) {
    let items = pane.turn_picker_items();
    let selected_turn = pane.turn_picker_selected_turn();
    let height = (items.len() as u16 + 4).clamp(6, area.height.saturating_sub(2).max(6));
    let picker_area = bottom_rect(82, height, 2, area);
    clear_modal_area(frame, picker_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_CODEX))
        .title(" Turn一覧 ")
        .title_bottom(
            Line::from(
                " ↑↓/jk: 選択 | Enter/o: 展開 | c: 格納 | Space: 開閉 | a: 全展開 | Esc/q: 閉じる ",
            )
            .style(Style::default().fg(COLOR_MUTED)),
        );
    let inner = block.inner(picker_area);
    frame.render_widget(block, picker_area);

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        "格納されたturnを会話中でも明示的に展開できます",
        Style::default().fg(COLOR_MUTED),
    )));
    for item in items {
        let selected = selected_turn == Some(item.turn);
        let marker = if selected { ">" } else { " " };
        let state = if item.current {
            "実行中"
        } else if item.collapsed {
            "格納"
        } else {
            "展開"
        };
        let style = if selected {
            Style::default()
                .fg(COLOR_TEXT_STRONG)
                .add_modifier(Modifier::BOLD)
        } else if item.collapsed {
            Style::default().fg(COLOR_WARN)
        } else {
            Style::default().fg(COLOR_TEXT)
        };
        let text = format!(
            "{marker} Turn {:>2} [{state}] {}",
            item.turn,
            prompt_preview(&item.title, inner.width.saturating_sub(18) as usize)
        );
        lines.push(Line::from(Span::styled(text, style)));
    }
    if lines.len() > inner.height as usize {
        lines.truncate(inner.height as usize);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_codex_exec_panel(frame: &mut Frame, area: Rect, block: Block<'_>, pane: &mut CodexPane) {
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let input_h = codex_input_panel_height(pane, inner.width, inner.height);
    let header_h = if inner.height >= 7 {
        2
    } else {
        u16::from(inner.height >= 6)
    };
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
            Paragraph::new(codex_header_lines(
                pane,
                header_chunk.width as usize,
                header_chunk.height as usize,
            )),
            header_chunk,
        );
    }

    let history_width = history_chunk.width as usize;
    let history_height = history_chunk.height as usize;
    let diff_view = pane.diff_view().map(str::to_string);
    let (history_lines, total_history_lines) = if let Some(diff) = diff_view.as_deref() {
        codex_visible_diff_lines(diff, history_width, pane.scrollback, history_height)
    } else {
        let streaming_line = pane.streaming_assistant_line();
        let filtered_log = pane.filtered_log_lines();
        codex_visible_log_lines(
            &filtered_log,
            history_width,
            pane.scrollback,
            history_height,
            streaming_line,
        )
    };
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
                "  Esc/q:戻る  c/s/d/v:還流  F7:turn一覧  /turn <N>",
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
            let status_style = if pane.decision_banner().is_some() {
                Style::default()
                    .fg(COLOR_DANGER)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(COLOR_MUTED)
            };
            let mut lines = Vec::with_capacity(prompt_lines.len() + 1);
            lines.push(Line::from(Span::styled(status, status_style)));
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

    // 入力が `/xxx` のときはスラッシュコマンドパレット、`@xxx` のときは
    // ファイル候補パレットを入力欄の直上に重ねる（両立しない）。
    if pane.slash_palette_active() {
        draw_codex_slash_palette(frame, input_chunk, history_chunk, pane);
    } else if pane.mention_palette_active() {
        draw_codex_mention_palette(frame, input_chunk, history_chunk, pane);
    }
}

/// 入力欄の直上に、現在の `@` 入力に一致するファイル候補を表示する。
fn draw_codex_mention_palette(
    frame: &mut Frame,
    input_chunk: Rect,
    history_chunk: Rect,
    pane: &CodexPane,
) {
    let suggestions = pane.mention_palette_suggestions();
    if suggestions.is_empty() {
        return;
    }
    let total = suggestions.len();
    let selected = pane.mention_palette_selected();

    let avail = history_chunk.height as usize;
    if avail < 3 {
        return;
    }
    let rows = total.min(MENTION_PALETTE_VISIBLE_ROWS).min(avail - 2);
    if rows == 0 {
        return;
    }
    let start = if selected >= rows {
        selected + 1 - rows
    } else {
        0
    };
    let end = (start + rows).min(total);

    let ph = (rows + 2) as u16;
    let area = Rect {
        x: input_chunk.x,
        y: input_chunk.y.saturating_sub(ph),
        width: input_chunk.width,
        height: ph,
    };

    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_CODEX))
        .style(Style::default().bg(COLOR_INPUT_BG))
        .title(Span::styled(
            format!(" @ ファイル  {total}件 "),
            Style::default().fg(COLOR_CODEX),
        ))
        .title_bottom(
            Line::from(" Tab/Enter:確定  ↑↓:選択  Esc:消す ")
                .style(Style::default().fg(COLOR_MUTED)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = (start..end)
        .map(|i| {
            let candidate = &suggestions[i];
            let is_sel = i == selected;
            let (marker, name_style) = if is_sel {
                (
                    "▸ ",
                    Style::default()
                        .fg(COLOR_TEXT_STRONG)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("  ", Style::default().fg(COLOR_TEXT))
            };
            let icon = if candidate.is_dir { "📁 " } else { "📄 " };
            Line::from(vec![
                Span::styled(marker, name_style),
                Span::styled(icon, Style::default().fg(COLOR_MUTED)),
                Span::styled(candidate.display.clone(), name_style),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(COLOR_INPUT_BG)),
        inner,
    );
}

/// 入力欄の直上に、現在の `/` 入力に一致するスラッシュコマンド候補を表示する。
fn draw_codex_slash_palette(
    frame: &mut Frame,
    input_chunk: Rect,
    history_chunk: Rect,
    pane: &CodexPane,
) {
    let suggestions = pane.slash_palette_suggestions();
    if suggestions.is_empty() {
        return;
    }
    let total = suggestions.len();
    let selected = pane.slash_palette_selected();

    // 入力欄の上（履歴領域）に確保できる高さ内で行数を決める。枠に 2 行使う。
    let avail = history_chunk.height as usize;
    if avail < 3 {
        return;
    }
    let rows = total.min(8).min(avail - 2);
    if rows == 0 {
        return;
    }
    // 選択中の候補が窓に入るようスクロール位置を決める。
    let start = if selected >= rows {
        selected + 1 - rows
    } else {
        0
    };
    let end = (start + rows).min(total);

    let ph = (rows + 2) as u16;
    let area = Rect {
        x: input_chunk.x,
        y: input_chunk.y.saturating_sub(ph),
        width: input_chunk.width,
        height: ph,
    };

    clear_modal_area(frame, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_CODEX))
        .style(Style::default().bg(COLOR_INPUT_BG))
        .title(Span::styled(
            format!(" / コマンド  {total}件 "),
            Style::default().fg(COLOR_CODEX),
        ))
        .title_bottom(
            Line::from(" Tab:補完  ↑↓:選択  Esc:消す ").style(Style::default().fg(COLOR_MUTED)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let name_w = 16usize;
    let lines: Vec<Line> = (start..end)
        .map(|i| {
            let (name, desc) = suggestions[i];
            let is_sel = i == selected;
            let (marker, name_style) = if is_sel {
                (
                    "▸ ",
                    Style::default()
                        .fg(COLOR_TEXT_STRONG)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("  ", Style::default().fg(COLOR_TEXT))
            };
            Line::from(vec![
                Span::styled(format!("{marker}{name:<name_w$}"), name_style),
                Span::styled(format!("  {desc}"), Style::default().fg(COLOR_MUTED)),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(COLOR_INPUT_BG)),
        inner,
    );
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

pub(crate) fn codex_input_panel_height(
    pane: &CodexPane,
    inner_width: u16,
    inner_height: u16,
) -> u16 {
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
    decision: &super::agent::CodexDecisionBanner,
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
    decision: &super::agent::CodexDecisionBanner,
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
        CodexDecisionKind::YesNo => "確認待ち: Yes/No ",
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

fn codex_decision_title_hint(
    kind: AgentKind,
    decision: &super::agent::CodexDecisionBanner,
) -> String {
    let keys = match decision.kind {
        CodexDecisionKind::YesNo => "y/n",
        CodexDecisionKind::Dangerous => "a/y または d/n",
        CodexDecisionKind::Approval | CodexDecisionKind::Permission => {
            if decision.always_choice().is_some() {
                "a/y・d/n・l:ずっと許可"
            } else {
                "a/y または d/n"
            }
        }
    };
    format!(
        " {} 確認待ち — 下の確認欄で選択 / {keys} ",
        kind.display_name()
    )
}

fn codex_decision_choice_line(
    decision: &super::agent::CodexDecisionBanner,
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

fn decision_choice_text(decision: &super::agent::CodexDecisionBanner, is_accept: bool) -> String {
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

fn decision_always_choice_text(decision: &super::agent::CodexDecisionBanner) -> Option<String> {
    decision
        .always_choice()
        .map(|(key, label)| format!("[{}] {label}", key.to_ascii_uppercase()))
}

fn codex_header_lines(pane: &CodexPane, max_width: usize, max_rows: usize) -> Vec<Line<'static>> {
    let run_state = pane.run_state();
    let focus = codex_current_activity_label(pane, max_width.saturating_sub(6));
    // 承認待ちは「作業を止めて人間の応答を待っている」状態なので、単なる実行中（WARN）より
    // 目立つ危険色（DANGER）にして見逃しを防ぐ。
    let focus_style = if pane.decision_banner().is_some() {
        Style::default()
            .fg(COLOR_DANGER)
            .add_modifier(Modifier::BOLD)
    } else if pane.is_turn_running() {
        Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(COLOR_TEXT_STRONG)
    };
    let mut parts = vec![
        format!("状態: {}", run_state.label()),
        format!("Turn {}", pane.turn_count()),
        format!("表示:{}", pane.log_filter_display_label()),
    ];
    if let Some(usage) = pane.usage_header_label() {
        parts.push(usage);
    }
    if pane.collapsed_turn_count() > 0 {
        parts.push(format!("格納{}", pane.collapsed_turn_count()));
    }
    if pane.subagent_running_count() > 0 {
        parts.push(format!("Sub:{}", pane.subagent_running_count()));
    }
    if pane.diff_view().is_some() {
        parts.push("差分表示中".to_string());
    }
    if pane.is_search_editing() {
        let query = if pane.search_query().is_empty() {
            "入力中".to_string()
        } else {
            format!("{}*", pane.search_query())
        };
        parts.push(format!("検索:{query}"));
    } else if !pane.search_query().is_empty() {
        parts.push(format!("検索:{}", pane.search_query()));
    }
    parts.push("Ctrl-T:表示切替 F7:turn一覧".to_string());
    if max_rows <= 1 {
        let text = format!(" 今: {focus} | {}", parts.join(" | "));
        return vec![Line::from(Span::styled(
            ellipsize_width(&text, max_width),
            focus_style,
        ))];
    }

    let summary = format!(" {}", parts.join(" | "));
    let summary_style = if pane.decision_banner().is_some() {
        Style::default()
            .fg(COLOR_DANGER)
            .add_modifier(Modifier::BOLD)
    } else if pane.is_turn_running() {
        Style::default().fg(COLOR_WARN)
    } else {
        Style::default().fg(COLOR_MUTED)
    };
    vec![
        Line::from(vec![
            Span::styled(" 今 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(focus, focus_style),
        ]),
        Line::from(Span::styled(
            ellipsize_width(&summary, max_width),
            summary_style,
        )),
    ]
}

#[cfg(test)]
fn codex_header_line(pane: &CodexPane, max_width: usize) -> Line<'static> {
    codex_header_lines(pane, max_width, 1)
        .into_iter()
        .next()
        .unwrap_or_else(|| Line::from(""))
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

fn codex_child_goal_lines(
    child: &ChildGoal,
    now: Instant,
    ordinal: usize,
    active: bool,
) -> Vec<Line<'static>> {
    let is_new = child.new_until.is_some_and(|t| t > now);
    let title_style = if active {
        Style::default()
            .fg(Color::Black)
            .bg(COLOR_ADDNESS)
            .add_modifier(Modifier::BOLD)
    } else if is_new {
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
    let ordinal_style = if active {
        Style::default()
            .fg(COLOR_ADDNESS)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(COLOR_MUTED)
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{}{ordinal:>2}. ", if active { ">" } else { "" }),
            ordinal_style,
        ),
        Span::styled(format!("{} ", child.icon), marker_style),
        Span::styled(child.title.clone(), title_style),
        Span::styled(format!("  {}", child.status_label), status_style),
    ])];
    if let Some(dod) = child
        .description
        .as_deref()
        .map(str::trim)
        .filter(|dod| !dod.is_empty())
    {
        lines.push(Line::from(vec![
            Span::styled("     DoD ", Style::default().fg(COLOR_MUTED)),
            Span::styled(prompt_preview(dod, 96), Style::default().fg(COLOR_MUTED)),
        ]));
    }
    lines
}

#[derive(Clone)]
struct RenderedCodexLine {
    line: Line<'static>,
}

fn codex_log_lines(
    lines: &[&CodexLogLine],
    max_width: usize,
    streaming: Option<&CodexLogLine>,
) -> Vec<RenderedCodexLine> {
    lines
        .iter()
        .flat_map(|line| {
            let mut rendered = Vec::new();
            if line.kind == CodexLogKind::Turn {
                rendered.push(codex_separator_line(line.kind, max_width));
            }
            rendered.extend(codex_log_entry_lines(line, max_width, streaming));
            rendered
        })
        .collect()
}

/// assistant 行を Markdown 整形して描画すべきか判定する。
/// ストリーミング中（未完成）の行はプレーン表示のままとする。
fn is_completed_assistant(line: &CodexLogLine, streaming: Option<&CodexLogLine>) -> bool {
    line.kind == CodexLogKind::Assistant
        && !streaming.is_some_and(|current| std::ptr::eq(current, line))
}

fn codex_visible_log_lines(
    lines: &[&CodexLogLine],
    max_width: usize,
    scrollback: usize,
    viewport_height: usize,
    streaming: Option<&CodexLogLine>,
) -> (Vec<RenderedCodexLine>, usize) {
    if lines.is_empty() || viewport_height == 0 {
        return (Vec::new(), 0);
    }

    let counts = lines
        .iter()
        .map(|line| codex_log_line_rendered_count(line, max_width, streaming))
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
    let visible = codex_log_lines(&selected, max_width, streaming)
        .into_iter()
        .skip(skip)
        .take(take)
        .collect();
    (visible, total)
}

fn codex_visible_diff_lines(
    diff: &str,
    max_width: usize,
    scrollback: usize,
    viewport_height: usize,
) -> (Vec<RenderedCodexLine>, usize) {
    if viewport_height == 0 {
        return (Vec::new(), 0);
    }
    let all = codex_diff_lines(diff, max_width);
    let total = all.len();
    let max_scrollback = total.saturating_sub(viewport_height);
    let scrollback = scrollback.min(max_scrollback);
    let view_end = total.saturating_sub(scrollback);
    let view_start = view_end.saturating_sub(viewport_height);
    (all[view_start..view_end].to_vec(), total)
}

fn codex_diff_lines(diff: &str, max_width: usize) -> Vec<RenderedCodexLine> {
    let content_width = max_width.saturating_sub(CODEX_LOG_PREFIX_WIDTH).max(1);
    let mut lines = Vec::new();
    for raw in diff.replace('\r', "").lines() {
        let style = codex_diff_line_style(raw);
        for (idx, part) in wrap_log_text(raw, content_width).into_iter().enumerate() {
            let prefix = if idx == 0 { "差分 | " } else { "     | " };
            let prefix_style = if idx == 0 {
                Style::default()
                    .fg(COLOR_CODEX)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(COLOR_PANEL)
            };
            lines.push(RenderedCodexLine {
                line: Line::from(vec![
                    Span::styled(prefix, prefix_style),
                    Span::styled(part, style),
                ]),
            });
        }
    }
    if lines.is_empty() {
        lines.push(RenderedCodexLine {
            line: Line::from(Span::styled(
                "差分 | 差分はありません。",
                Style::default().fg(COLOR_MUTED),
            )),
        });
    }
    lines
}

fn codex_diff_line_style(line: &str) -> Style {
    let trimmed = line.trim_start();
    if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
        Style::default()
            .fg(COLOR_SUCCESS)
            .add_modifier(Modifier::BOLD)
    } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
        Style::default()
            .fg(COLOR_DANGER)
            .add_modifier(Modifier::BOLD)
    } else if trimmed.starts_with("@@") {
        Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
    } else if trimmed.starts_with("diff ")
        || trimmed.starts_with("index ")
        || trimmed.starts_with("---")
        || trimmed.starts_with("+++")
        || trimmed.starts_with("## ")
        || trimmed.contains("omitted")
    {
        Style::default().fg(COLOR_MUTED)
    } else {
        Style::default().fg(COLOR_TEXT)
    }
}

/// 完成済み assistant 行の Markdown 描画結果キャッシュ。
/// キーは（テキスト内容のハッシュ, 折り返し幅）。ストリーミング中は 20ms ごとに全画面を
/// 再描画するため、完成済み行を毎フレーム再パースすると CPU を浪費する。行数カウントと
/// 実描画が同一のキャッシュ結果を参照することで「カウント＝描画行数」不変条件も保たれる。
/// 幅変更（リサイズ）時はキー不一致となり自然に再計算される。
const ASSISTANT_MD_CACHE_MAX: usize = 4096;

/// 1 メッセージ分の Markdown 描画結果（視覚行ごとのスパン列）。
type RenderedMarkdown = Vec<Vec<Span<'static>>>;

thread_local! {
    static ASSISTANT_MD_CACHE: RefCell<HashMap<(u64, usize), RenderedMarkdown>> =
        RefCell::new(HashMap::new());
}

fn hash_markdown_text(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// 完成済み assistant テキストの Markdown 描画結果をキャッシュ経由で得て、`f` に渡す。
/// 未キャッシュならレンダリングして格納する（サイズ上限超過時は一括クリア）。
fn with_cached_assistant_markdown<R>(
    text: &str,
    content_width: usize,
    f: impl FnOnce(&[Vec<Span<'static>>]) -> R,
) -> R {
    let key = (hash_markdown_text(text), content_width);
    ASSISTANT_MD_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(&key) {
            let rendered =
                markdown::render_assistant_markdown(text, content_width, &codex_markdown_styles());
            if cache.len() >= ASSISTANT_MD_CACHE_MAX {
                cache.clear();
            }
            cache.insert(key, rendered);
        }
        f(cache.get(&key).expect("just inserted"))
    })
}

/// 完成済み assistant 行の描画行数（キャッシュ利用）。返り値は必ず 1 以上。
fn cached_assistant_markdown_count(text: &str, content_width: usize) -> usize {
    with_cached_assistant_markdown(text, content_width, |lines| lines.len().max(1))
}

/// 完成済み assistant 行の描画スパン列（キャッシュ利用）。呼び出し側で所有権が必要なので複製する。
fn cached_assistant_markdown_lines(text: &str, content_width: usize) -> Vec<Vec<Span<'static>>> {
    with_cached_assistant_markdown(text, content_width, |lines| lines.to_vec())
}

fn codex_log_line_rendered_count(
    line: &CodexLogLine,
    max_width: usize,
    streaming: Option<&CodexLogLine>,
) -> usize {
    let separator = usize::from(line.kind == CodexLogKind::Turn);
    let content_width = max_width.saturating_sub(CODEX_LOG_PREFIX_WIDTH).max(1);
    if is_completed_assistant(line, streaming) {
        // 描画（codex_log_entry_lines）と同じキャッシュ結果を参照して行数を数え、整合させる。
        return separator + cached_assistant_markdown_count(&line.text, content_width);
    }
    separator + wrapped_log_line_count(&codex_log_display_text(line), content_width)
}

fn codex_separator_line(kind: CodexLogKind, max_width: usize) -> RenderedCodexLine {
    let (head, fill, color) = match kind {
        CodexLogKind::Turn => ("-----+ ", '-', COLOR_PANEL),
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

fn codex_log_entry_lines(
    line: &CodexLogLine,
    max_width: usize,
    streaming: Option<&CodexLogLine>,
) -> Vec<RenderedCodexLine> {
    let (prefix, prefix_style, text_style) = codex_log_prefix(line);
    let continuation = "     | ";
    let content_width = max_width.saturating_sub(CODEX_LOG_PREFIX_WIDTH).max(1);
    if is_completed_assistant(line, streaming) {
        return codex_assistant_markdown_lines(&line.text, content_width, prefix, prefix_style);
    }
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

/// assistant メッセージのテーマ配色。
fn codex_markdown_styles() -> MarkdownStyles {
    MarkdownStyles {
        text: Style::default().fg(COLOR_TEXT),
        heading: Style::default()
            .fg(COLOR_ADDNESS)
            .add_modifier(Modifier::BOLD),
        strong: Style::default()
            .fg(COLOR_TEXT_STRONG)
            .add_modifier(Modifier::BOLD),
        emphasis: Style::default()
            .fg(COLOR_TEXT)
            .add_modifier(Modifier::ITALIC),
        inline_code: Style::default().fg(COLOR_WARN).bg(COLOR_INPUT_BG),
        code_block: Style::default().fg(COLOR_TEXT).bg(COLOR_INPUT_BG),
        code_label: Style::default().fg(COLOR_MUTED),
        quote: Style::default()
            .fg(COLOR_MUTED)
            .add_modifier(Modifier::ITALIC),
        quote_marker: Style::default().fg(COLOR_PANEL),
        list_marker: Style::default()
            .fg(COLOR_ADDNESS)
            .add_modifier(Modifier::BOLD),
        rule: Style::default().fg(COLOR_PANEL),
        link: Style::default()
            .fg(COLOR_ADDNESS)
            .add_modifier(Modifier::UNDERLINED),
    }
}

/// 完成済み assistant メッセージを Markdown 整形し、プレフィックス列付きの
/// 描画行に変換する。折り返しは `render_assistant_markdown` 側で実施済み。
fn codex_assistant_markdown_lines(
    text: &str,
    content_width: usize,
    prefix: &'static str,
    prefix_style: Style,
) -> Vec<RenderedCodexLine> {
    let continuation = "     | ";
    let content_lines = cached_assistant_markdown_lines(text, content_width);
    let mut lines = Vec::with_capacity(content_lines.len().max(1));
    for (idx, mut spans) in content_lines.into_iter().enumerate() {
        let (prefix_text, marker_style) = if idx == 0 {
            (prefix, prefix_style)
        } else {
            (continuation, Style::default().fg(COLOR_PANEL))
        };
        let mut line_spans = vec![Span::styled(prefix_text, marker_style)];
        line_spans.append(&mut spans);
        lines.push(RenderedCodexLine {
            line: Line::from(line_spans),
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
    } else if trimmed.starts_with("更新:")
        || trimmed.starts_with("追加:")
        || trimmed.starts_with("削除:")
        || trimmed.starts_with("移動:")
        || trimmed.starts_with("update:")
        || trimmed.starts_with("add:")
        || trimmed.starts_with("delete:")
        || trimmed.starts_with("move:")
        || trimmed.ends_with("files changed")
        || trimmed.ends_with("件のファイル変更")
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
            "依頼 | ",
            Style::default()
                .fg(COLOR_ADDNESS)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        ),
        CodexLogKind::Assistant => (
            "返答 | ",
            Style::default()
                .fg(COLOR_TEXT_STRONG)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        ),
        CodexLogKind::Tool => codex_tool_prefix(&line.text),
        CodexLogKind::Turn => (
            "turn | ",
            Style::default()
                .fg(COLOR_CODEX)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD),
        ),
        CodexLogKind::System => (
            "     | ",
            Style::default().fg(COLOR_MUTED),
            Style::default().fg(COLOR_MUTED),
        ),
        CodexLogKind::Error => (
            "失敗 | ",
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
            "編集 | ",
            Style::default()
                .fg(COLOR_CODEX)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        )
    } else if text.starts_with("DIFF ") {
        (
            "差分 | ",
            Style::default()
                .fg(COLOR_MUTED)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        )
    } else if text.starts_with("FAIL ") || text.contains("exit ") && !text.contains("exit 0") {
        (
            "失敗 | ",
            Style::default()
                .fg(COLOR_DANGER)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_DANGER),
        )
    } else if text.starts_with("OK ") || text.contains("exit 0") {
        (
            "完了 | ",
            Style::default()
                .fg(COLOR_SUCCESS)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        )
    } else if text.contains("output_delta") || text.contains('\n') {
        (
            "出力 | ",
            Style::default().fg(COLOR_MUTED),
            Style::default().fg(COLOR_TEXT),
        )
    } else if text.starts_with("RUNNING ") {
        (
            "実行 | ",
            Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD),
            Style::default().fg(COLOR_TEXT),
        )
    } else {
        (
            "作業 | ",
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
        "実行中"
    } else {
        "実行"
    };
    let compact = compact_tool_command_name(command);
    let command = ellipsize_width(&compact, CODEX_TOOL_COMMAND_PREVIEW_WIDTH);
    format!("• {verb}: {command}")
}

fn compact_tool_command_name(command: &str) -> String {
    if looks_like_code_edit_display_text(command)
        && let Some(path) = code_edit_changes(command)
            .first()
            .map(|change| change.path.clone())
    {
        return path;
    }
    if let Some(preview) = addness_tool_command_preview(command) {
        return preview;
    }
    let concise = concise_command_name(command);
    if !concise.is_empty() {
        return concise;
    }
    let (_, command) = split_tool_state_prefix(command.trim());
    let first_line = command.lines().next().unwrap_or("").trim();
    ellipsize_width(first_line, CODEX_TOOL_COMMAND_PREVIEW_WIDTH)
}

fn addness_tool_command_preview(command: &str) -> Option<String> {
    let (_, command) = split_tool_state_prefix(command.trim());
    let first_line = command.lines().next().unwrap_or("").trim();
    let rest = addness_command_rest_from_line(first_line)?;
    if rest.is_empty() {
        Some("addness".to_string())
    } else {
        Some(format!("addness {rest}"))
    }
}

fn addness_command_rest_from_line(line: &str) -> Option<&str> {
    let mut command = line.trim();
    if let Some(rest) = command.strip_prefix('$') {
        command = rest.trim_start();
    }

    let lower = command.to_ascii_lowercase();
    if lower == "addness" {
        return Some("");
    }
    if lower.starts_with("addness ") {
        return Some(command["addness ".len()..].trim());
    }

    for marker in [
        "\"$addness_bin\" ",
        "'$addness_bin' ",
        "$addness_bin ",
        "${addness_bin} ",
    ] {
        if lower.starts_with(marker) {
            return Some(command[marker.len()..].trim());
        }
    }

    if let Some(idx) = lower.find("/addness ") {
        return Some(command[idx + "/addness ".len()..].trim());
    }
    if let Some(idx) = lower.find(" addness ") {
        return Some(command[idx + " addness ".len()..].trim());
    }

    None
}

fn tool_output_tree_preview(output: &str) -> String {
    match non_empty_line_count(output) {
        0 => "出力なし".to_string(),
        1 => "出力1行".to_string(),
        n => format!("出力{n}行"),
    }
}

fn non_empty_line_count(text: &str) -> usize {
    text.lines().filter(|line| !line.trim().is_empty()).count()
}

fn compact_command_result(kind: &str, output: &str) -> Option<String> {
    let lower = output.to_ascii_lowercase();
    if output.lines().any(|line| line.contains("Finished ")) || lower.trim() == "ok" {
        return Some(format!("{kind}: 成功"));
    }
    if output
        .lines()
        .any(|line| line.contains("test result:") && line.to_ascii_lowercase().contains(" ok."))
    {
        return Some(format!("{kind}: 成功"));
    }
    if lower.contains("fail") || lower.contains("error") {
        return Some(format!("{kind}: 失敗"));
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
        let first_line = text.lines().next().unwrap_or("コード編集").trim();
        let title = first_line.strip_prefix("EDIT ").unwrap_or(first_line);
        let title = if title.is_empty() || title.contains("*** Begin Patch") {
            "コード編集"
        } else {
            title
        };
        return Some(format!("{title}\n  コード編集"));
    }

    let first = changes.first()?;
    let title = if changes.len() == 1 {
        format!("{}: {}", first.action, first.path)
    } else {
        format!("{}件のファイル変更", changes.len())
    };
    let mut lines = vec![title];
    if changes.len() > 1 {
        for change in changes.iter().take(3) {
            lines.push(format!("  {}: {}", change.action, change.path));
        }
        let omitted = changes.len().saturating_sub(3);
        if omitted > 0 {
            lines.push(format!("  ... 他{omitted}件"));
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
            ("*** Update File: ", "更新"),
            ("*** Add File: ", "追加"),
            ("*** Delete File: ", "削除"),
            ("*** Move to: ", "移動"),
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
        out.push(format!("... 差分行を{omitted}行省略"));
    }
    out
}

fn special_tool_summary(head: &str, output: &str) -> Option<String> {
    let lower = head.to_ascii_lowercase();

    if lower.contains("cargo test") {
        return compact_command_result("テスト", output);
    }

    if lower.contains("cargo clippy") {
        return compact_command_result("clippy", output);
    }

    if lower.contains("cargo build") {
        return compact_command_result("ビルド", output);
    }

    if lower.contains("git diff") || lower.contains("git status") {
        if output.trim().is_empty() {
            return Some("差分: 出力なし".to_string());
        }
        return Some(format!("差分: {}行", non_empty_line_count(output)));
    }

    if looks_like_addness_command(head)
        && let Some(summary) = json_output_summary(output)
    {
        return Some(format!("addness: {summary}"));
    }

    if looks_like_codex_management_command(head) {
        return Some(codex_management_output_summary(head, output));
    }

    None
}

fn looks_like_codex_management_command(head: &str) -> bool {
    let lower = head.to_ascii_lowercase();
    [
        "codex mcp",
        "codex plugin",
        "codex cloud",
        "codex login",
        "codex logout",
        "codex update",
        "codex app",
        "codex app-server",
        "codex remote-control",
        "codex debug",
        "codex completion",
        "codex mcp-server",
        "codex exec-server",
        "codex features",
        "codex doctor",
    ]
    .iter()
    .any(|prefix| lower.contains(prefix))
}

fn codex_management_output_summary(head: &str, output: &str) -> String {
    let lower = head.to_ascii_lowercase();
    let noun = if lower.contains(" mcp") {
        "mcp"
    } else if lower.contains(" plugin") {
        "plugins"
    } else if lower.contains(" cloud") {
        "cloud"
    } else if lower.contains(" login") || lower.contains(" logout") {
        "auth"
    } else if lower.contains(" app-server")
        || lower.contains(" remote-control")
        || lower.contains(" mcp-server")
        || lower.contains(" exec-server")
    {
        "server"
    } else if lower.contains(" debug") {
        "debug"
    } else if lower.contains(" completion") {
        "completion"
    } else if lower.contains(" features") {
        "features"
    } else if lower.contains(" doctor") {
        "doctor"
    } else {
        "codex"
    };

    if let Some(summary) = json_output_summary(output) {
        return format!("{noun}: {summary}");
    }
    let count = non_empty_line_count(output);
    if count == 0 {
        format!("{noun}: 出力なし")
    } else if count == 1 {
        format!("{noun}: {}", output.trim())
    } else {
        format!("{noun}: {count}行")
    }
}

fn looks_like_addness_command(head: &str) -> bool {
    if addness_tool_command_preview(head).is_some() {
        return true;
    }
    let lower = head.to_ascii_lowercase();
    lower.contains("addness")
        || lower.contains("$addness_bin")
        || lower.contains(" goal get ")
        || lower.contains(" goal list ")
        || lower.contains(" goal children ")
        || lower.contains(" comment list ")
        || lower.contains(" deliverable ")
        || lower.contains(" link ")
        || lower.contains(" status ")
        || lower.contains(" summary ")
}

fn json_output_summary(output: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(output.trim()).ok()?;
    if value.get("goal").is_some() || value.get("title").is_some() {
        return Some("ゴール".to_string());
    }
    match value {
        Value::Array(items) => Some(format!("一覧{}件", items.len())),
        Value::Object(map) => Some(format!("{}項目", map.len())),
        _ => Some("値".to_string()),
    }
}

fn codex_runtime_status(pane: &CodexPane, max_width: usize) -> String {
    let state = pane.run_state_elapsed_label();
    let view = format!("表示:{} Ctrl-Tで切替", pane.log_filter_display_label());
    let fixed_width = UnicodeWidthStr::width(state.as_str())
        .saturating_add(UnicodeWidthStr::width(view.as_str()))
        .saturating_add(8);
    let detail = codex_current_activity_label(pane, max_width.saturating_sub(fixed_width));
    ellipsize_width(&format!("  {state}  {detail}  |  {view}"), max_width)
}

/// 実行中スピナーのコマ送り文字。Addness ゴール同期の鼓動表示（`draw_codex` 内 SPIN 配列）と
/// 揃えた点字スピナーを使う。
const RUN_SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// tick を実行中スピナーの1文字へ変換する純関数。tick は単調増加のカウンタであればよく、
/// 実際に何ミリ秒間隔で進めるかは呼び出し側（`CodexPane::advance_activity_spin`）が決める。
/// 副作用を持たないため、コマ送りの見た目はここだけをテストすれば検証できる。
fn run_spinner_glyph(tick: u64) -> &'static str {
    RUN_SPINNER_FRAMES[(tick as usize) % RUN_SPINNER_FRAMES.len()]
}

fn codex_current_activity_label(pane: &CodexPane, max_width: usize) -> String {
    let detail = if pane.is_turn_running() {
        let spin = run_spinner_glyph(pane.activity_spin_tick());
        let queued = pane.queued_prompt_count();
        let input_hint = if queued > 0 {
            format!(" / 予約{queued}件")
        } else {
            " / Enterで次ターン予約".to_string()
        };
        if let Some(decision) = pane.decision_banner() {
            // 承認待ちは他の実行中状態より目に留まりやすいラベルにする（色は呼び出し側で強調）。
            format!("⏸ 承認待ち: {}", decision.message)
        } else if let Some(command) = pane.current_command() {
            let elapsed = pane
                .current_command_elapsed_secs()
                .map(|secs| format!(" {secs}s"))
                .unwrap_or_default();
            // 常駐 codex の実行中コマンドは末尾のライブ出力行を薄く添える。
            let live = pane
                .codex_appserver_live_output()
                .last()
                .map(|line| format!(" › {line}"))
                .unwrap_or_default();
            format!(
                "{spin} {}{elapsed}{live}{input_hint}",
                codex_command_activity_summary(command)
            )
        } else if let Some(action) = pane.work_action() {
            format!("{spin} 作業中: {action}{input_hint}")
        } else {
            format!(
                "{spin} {}が考えています{input_hint}",
                pane.kind().display_name()
            )
        }
    } else {
        codex_work_label(
            pane.finished,
            pane.assessing,
            pane.status_note(),
            pane.last_prompt(),
            max_width,
        )
    };
    ellipsize_width(&detail, max_width)
}

fn codex_command_activity_summary(command: &str) -> String {
    let kind = codex_command_activity_kind(command);
    let subject = codex_command_subject(command);
    if subject.is_empty() {
        kind.to_string()
    } else {
        format!("{kind} ({subject})")
    }
}

fn codex_command_activity_kind(command: &str) -> &'static str {
    let lower = command.to_ascii_lowercase();
    if looks_like_code_edit_display_text(command) {
        return "ファイルを編集中";
    }
    if looks_like_addness_command(command) {
        return if lower.contains(" update ")
            || lower.contains(" create ")
            || lower.contains(" deliverable ")
            || lower.contains(" link ")
            || lower.contains(" notification ")
        {
            "Addnessへ記録中"
        } else {
            "Addnessを確認中"
        };
    }
    if lower.contains("cargo test")
        || lower.contains("go test")
        || lower.contains("npm test")
        || lower.contains("pnpm test")
        || lower.contains("yarn test")
        || lower.contains("pytest")
    {
        return "テストを実行中";
    }
    if lower.contains("cargo clippy")
        || lower.contains(" clippy")
        || lower.contains("eslint")
        || lower.contains(" lint")
    {
        return "lintを確認中";
    }
    if lower.contains("cargo fmt")
        || lower.contains("rustfmt")
        || lower.contains("prettier")
        || lower.contains(" format")
    {
        return "フォーマットを確認中";
    }
    if lower.contains("cargo build")
        || lower.contains("go build")
        || lower.contains("npm run build")
        || lower.contains("pnpm build")
        || lower.contains("yarn build")
        || lower.contains("swift build")
    {
        return "ビルド中";
    }
    if lower.contains("git diff")
        || lower.contains("git status")
        || lower.contains("git log")
        || lower.contains("git show")
    {
        return "差分を確認中";
    }
    if lower.contains("git add")
        || lower.contains("git commit")
        || lower.contains("git push")
        || lower.contains("git fetch")
        || lower.contains("git pull")
        || lower.contains("git merge")
        || lower.contains("git rebase")
        || lower.contains("git cherry-pick")
        || lower.contains("git tag")
    {
        return "Git操作中";
    }
    if lower.starts_with("rg ")
        || lower.contains(" rg ")
        || lower.starts_with("grep ")
        || lower.contains(" grep ")
        || lower.starts_with("find ")
        || lower.contains(" find ")
        || lower.starts_with("ls ")
        || lower.contains(" ls ")
        || lower.starts_with("sed ")
        || lower.contains(" sed ")
        || lower.starts_with("cat ")
        || lower.contains(" cat ")
        || lower.starts_with("head ")
        || lower.contains(" head ")
        || lower.starts_with("tail ")
        || lower.contains(" tail ")
        || lower.starts_with("nl ")
        || lower.contains(" nl ")
        || lower.starts_with("wc ")
        || lower.contains(" wc ")
    {
        return "ファイルを確認中";
    }
    if lower.contains("curl ") || lower.contains(" gh ") || lower.starts_with("gh ") {
        return "外部情報を確認中";
    }
    "コマンド実行中"
}

fn codex_command_subject(command: &str) -> String {
    if looks_like_code_edit_display_text(command)
        && let Some(path) = code_edit_changes(command)
            .first()
            .map(|change| change.path.clone())
    {
        return ellipsize_width(&path, 36);
    }
    if looks_like_addness_command(command) {
        if let Some(preview) = addness_tool_command_preview(command) {
            return ellipsize_width(&preview, 36);
        }
        return "addness".to_string();
    }
    concise_command_name(command)
}

fn concise_command_name(command: &str) -> String {
    let (_, command) = split_tool_state_prefix(command.trim());
    let first_line = command.lines().next().unwrap_or("").trim();
    let first_line = if let Some(rest) = first_line.strip_prefix('[') {
        rest.split_once(']')
            .map(|(_, tail)| tail.trim())
            .unwrap_or(first_line)
    } else {
        first_line
    };
    if first_line.is_empty() {
        return String::new();
    }

    let parts = first_line.split_whitespace().collect::<Vec<_>>();
    let keep = match parts.as_slice() {
        ["cargo", sub, ..] => vec!["cargo", *sub],
        ["codex", sub, ..] => vec!["codex", *sub],
        ["git", sub, ..] => vec!["git", *sub],
        ["go", sub, ..] => vec!["go", *sub],
        ["npm", "run", sub, ..] => vec!["npm", "run", *sub],
        ["pnpm", sub, ..] => vec!["pnpm", *sub],
        ["yarn", sub, ..] => vec!["yarn", *sub],
        [cmd, ..] => vec![*cmd],
        [] => Vec::new(),
    };
    if keep.is_empty() {
        String::new()
    } else {
        ellipsize_width(&keep.join(" "), 36)
    }
}

fn draw_codex_status_panel(frame: &mut Frame, area: Rect, pane: &CodexPane, scroll: &mut usize) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let value_width = inner_width.saturating_sub(8);
    let prompt_width = inner_width.saturating_sub(2);

    let run_state = pane.run_state();
    let state_style = match run_state {
        super::agent::CodexRunState::Completed => {
            Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD)
        }
        // 承認待ちは人間の応答をブロックしている状態なので、単なる実行中より目立つ危険色にする。
        super::agent::CodexRunState::Confirming => Style::default()
            .fg(COLOR_DANGER)
            .add_modifier(Modifier::BOLD),
        super::agent::CodexRunState::CommandRunning | super::agent::CodexRunState::Thinking => {
            Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
        }
        super::agent::CodexRunState::InputWaiting => {
            Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD)
        }
    };
    let focus = codex_current_activity_label(pane, prompt_width);
    let focus_style = if pane.decision_banner().is_some() {
        Style::default()
            .fg(COLOR_DANGER)
            .add_modifier(Modifier::BOLD)
    } else if pane.current_command().is_some() || pane.is_turn_running() {
        Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(COLOR_TEXT_STRONG)
    };
    let memory_mode = ellipsize_width(&pane.memory_mode_label(), value_width);
    let memory_mode_style = if pane.memory_mode_is_addness_safe() {
        Style::default().fg(COLOR_ADDNESS)
    } else {
        Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD)
    };
    let sync = codex_memory_label(
        pane.last_addness_read_at,
        pane.last_addness_write_at,
        pane.last_addness_read_label.as_deref(),
        pane.last_addness_write_label.as_deref(),
        value_width,
    );
    let sync_style = if pane.last_addness_write_at.is_some() || pane.last_addness_read_at.is_some()
    {
        Style::default().fg(COLOR_ADDNESS)
    } else {
        Style::default().fg(COLOR_MUTED)
    };
    let view = ellipsize_width(
        &format!(
            "{} / {} / 格納{}",
            pane.log_filter_display_label(),
            pane.history_label(),
            pane.collapsed_turn_count()
        ),
        value_width,
    );
    let settings = ellipsize_width(
        &format!("{} / {}", pane.settings_label(), pane.diff_label()),
        value_width,
    );
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
            Span::styled("今 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(focus, focus_style),
        ]),
        Line::from(vec![
            Span::styled("状態 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(pane.run_state_elapsed_label(), state_style),
            Span::styled(
                format!("  Turn {}", pane.turn_count()),
                Style::default().fg(COLOR_MUTED),
            ),
        ]),
        Line::from(vec![
            Span::styled("表示 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(view, Style::default().fg(COLOR_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("記憶 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(memory_mode, memory_mode_style),
        ]),
        Line::from(vec![
            Span::styled("同期 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(sync, sync_style),
        ]),
        Line::from(vec![
            Span::styled("設定 ", Style::default().fg(COLOR_MUTED)),
            Span::styled(settings, Style::default().fg(COLOR_TEXT)),
        ]),
    ];
    if let Some(active) = pane.active_work_package_label() {
        lines.insert(
            1,
            Line::from(vec![
                Span::styled("作業単位 ", Style::default().fg(COLOR_MUTED)),
                Span::styled(
                    active,
                    Style::default()
                        .fg(COLOR_ADDNESS)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
        );
    }
    if let Some(queue) = pane.queued_work_package_label() {
        lines.insert(
            if pane.active_work_package_label().is_some() {
                2
            } else {
                1
            },
            Line::from(vec![
                Span::styled("待機 ", Style::default().fg(COLOR_MUTED)),
                Span::styled(queue, Style::default().fg(COLOR_WARN)),
            ]),
        );
    }
    if let Some(label) = pane.goal_mode_label() {
        lines.push(Line::from(vec![
            Span::styled("Goal ", Style::default().fg(COLOR_MUTED)),
            Span::styled(
                ellipsize_width(&label, value_width),
                Style::default().fg(COLOR_ADDNESS),
            ),
        ]));
    }
    if area.height >= 11 {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", pane.kind().display_name()),
                Style::default().fg(COLOR_MUTED),
            ),
            Span::styled(assistant, assistant_style),
        ]));
    }
    // 直近アクション履歴（パンくず）。「今」は最新 1 件しか見せないため、コマンドが高速連続
    // 実行されると途中のアクションが一瞬で上書きされ見逃される。パネル高さに応じて件数を絞って
    // 直近 N 件を古い順に添える。
    let recent_actions = pane.recent_action_breadcrumbs();
    let recent_actions_visible = codex_recent_action_visible_count(area.height);
    if recent_actions_visible > 0 && !recent_actions.is_empty() {
        lines.push(Line::from(Span::styled(
            "直近",
            Style::default().fg(COLOR_MUTED),
        )));
        let start = recent_actions.len().saturating_sub(recent_actions_visible);
        for label in &recent_actions[start..] {
            lines.push(Line::from(Span::styled(
                ellipsize_width(&format!("  {label}"), prompt_width),
                Style::default().fg(COLOR_MUTED),
            )));
        }
    }
    // サブエージェント稼働状況（Claude Code の Task/Agent ツール起動）。
    // 実行中件数の集計行 + 実行中を優先した各エージェントの行を、パネル高さに応じて表示する。
    let subagent_running = pane.subagent_running_count();
    let subagent_visible = codex_subagent_visible_count(area.height);
    let subagent_lines = pane.subagent_status_lines(subagent_visible);
    if !subagent_lines.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("サブエージェント ", Style::default().fg(COLOR_MUTED)),
            Span::styled(
                format!("{subagent_running}体稼働中"),
                if subagent_running > 0 {
                    Style::default().fg(COLOR_WARN)
                } else {
                    Style::default().fg(COLOR_MUTED)
                },
            ),
        ]));
        for label in &subagent_lines {
            lines.push(Line::from(Span::styled(
                ellipsize_width(&format!("  {label}"), prompt_width),
                Style::default().fg(COLOR_MUTED),
            )));
        }
    }
    // 実行中コマンドのライブ出力（末尾最大3行）を薄色ブロックで添える。
    // コマンド完了で消え、確定した Tool 行だけがログに残る。
    let live_output = pane.codex_appserver_live_output();
    if pane.current_command().is_some() && !live_output.is_empty() {
        lines.push(Line::from(Span::styled(
            "実行中の出力",
            Style::default().fg(COLOR_MUTED),
        )));
        for out_line in &live_output {
            lines.push(Line::from(Span::styled(
                ellipsize_width(&format!("  {out_line}"), prompt_width),
                Style::default().fg(COLOR_MUTED),
            )));
        }
    }
    lines.push(Line::from(""));
    lines.extend(codex_dashboard_shortcut_lines(prompt_width));
    if area.height >= 13 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "最後の送信",
            Style::default().fg(COLOR_MUTED),
        )));
        lines.push(Line::from(Span::styled(prompt, prompt_style)));
    }

    let inner_h = area.height.saturating_sub(2) as usize;
    let inner_w = area.width.saturating_sub(2) as usize;
    let max_scroll = rendered_lines_height(&lines, inner_w).saturating_sub(inner_h.max(1));
    *scroll = (*scroll).min(max_scroll);

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
                .title({
                    let name = pane.kind().display_name();
                    if pane.decision_banner().is_some() {
                        format!(" {name} 作業ダッシュボード ▲確認待ち ")
                    } else if pane.is_turn_running() {
                        format!(" {name} 作業ダッシュボード ●実行中 ")
                    } else {
                        format!(" {name} 作業ダッシュボード ")
                    }
                }),
        )
        .scroll(((*scroll).min(u16::MAX as usize) as u16, 0))
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(panel, area);
}

/// パネル高さに応じて、状態パネルに表示する直近アクションパンくずの件数を決める。
/// 実際の呼び出し元（draw 関数）ではパネル高さは 8/10/12 の 3 段階（`chunks[0].height` に応じて
/// 選ばれる）が中心のため、その範囲では 0〜3 件に絞り、他の固定セクション（状態/表示/記憶/同期/
/// 設定やショートカット等）を圧迫しないようにする。将来パネルがより高くなった場合は最大 5 件
/// （リングバッファの保持上限 `RECENT_ACTIONS_CAP`）まで増やす。
fn codex_recent_action_visible_count(height: u16) -> usize {
    match height {
        0..=8 => 0,
        9..=10 => 2,
        11..=13 => 3,
        14..=17 => 4,
        _ => 5,
    }
}

/// パネル高さに応じて、状態パネルに表示するサブエージェント一覧の件数を決める。
/// `codex_recent_action_visible_count` と同じ段階を踏むが、直近アクションの後に追加される
/// セクションのため、低い高さではより控えめ（1 件から）に出す。
fn codex_subagent_visible_count(height: u16) -> usize {
    match height {
        0..=8 => 0,
        9..=10 => 1,
        11..=13 => 2,
        14..=17 => 3,
        _ => 5,
    }
}

fn codex_dashboard_shortcut_lines(max_width: usize) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "次の操作",
            Style::default()
                .fg(COLOR_TEXT_STRONG)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            ellipsize_width("  Enter:送信  F7:turn一覧  Ctrl-T:表示切替", max_width),
            Style::default().fg(COLOR_TEXT),
        )),
        Line::from(Span::styled(
            ellipsize_width(
                "  F6:差分  /organize:分解  /work next/all:着手  /dual",
                max_width,
            ),
            Style::default().fg(COLOR_TEXT),
        )),
        Line::from(Span::styled(
            ellipsize_width(
                "  /remember /handoff:保存  Ctrl+Q:全体ヘルプ  /settings",
                max_width,
            ),
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
    status_note: Option<&str>,
    last_prompt: Option<&str>,
    max_width: usize,
) -> String {
    if finished {
        return ellipsize_width("履歴確認", max_width);
    }
    if assessing {
        return ellipsize_width("DoD自動判定", max_width);
    }
    if let Some(note) = status_note {
        return ellipsize_width(note, max_width);
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
        ActivePane, App, CODEX_EDIT_DIFF_PREVIEW_LINES, COLOR_DANGER, COLOR_EVENT, COLOR_SUCCESS,
        COLOR_WARN, RUN_SPINNER_FRAMES, cached_assistant_markdown_count,
        cached_assistant_markdown_lines, code_edit_diff_preview, codex_activity_lines,
        codex_child_goal_lines, codex_current_activity_label, codex_dashboard_shortcut_lines,
        codex_decision_choice_line, codex_decision_input_lines, codex_decision_title_hint,
        codex_diff_lines, codex_header_line, codex_header_lines, codex_input_prompt_render,
        codex_log_entry_lines, codex_log_lines, codex_markdown_styles,
        codex_recent_action_visible_count, codex_runtime_status, codex_subagent_visible_count,
        codex_visible_log_lines, codex_work_label, draw_status_bar, ellipsize_width, markdown,
        prompt_preview, run_spinner_glyph, summarize_tool_display_text,
    };
    use crate::api::ApiClient;
    use crate::tui::agent::{
        AgentKind, CODEX_LOG_PREFIX_WIDTH, ChildGoal, CodexDecisionBanner, CodexDecisionKind,
        CodexLogKind, CodexLogLine, CodexPane,
    };
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Modifier;
    use ratatui::text::{Line, Span};
    use ratatui::{Terminal, backend::TestBackend};
    use std::time::Instant;
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

        let lines = codex_log_entry_lines(&entry, CODEX_LOG_PREFIX_WIDTH + 3, None);

        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0].line), "返答 | abc");
        assert_eq!(line_text(&lines[1].line), "     | def");
    }

    #[test]
    fn codex_log_entry_lines_marks_failed_tool_and_strips_event_noise() {
        let entry = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "exec_command_end (exit 1): cargo test\nfailed".to_string(),
        };

        let lines = codex_log_entry_lines(&entry, 80, None);

        assert_eq!(line_text(&lines[0].line), "失敗 | • 実行: cargo test");
        assert_eq!(line_text(&lines[1].line), "     |   └ テスト: 失敗");
        assert_eq!(lines[0].line.spans[1].content.as_ref(), "•");
        assert_eq!(lines[0].line.spans[1].style.fg, Some(COLOR_DANGER));
    }

    #[test]
    fn codex_log_entry_lines_colors_tool_bullet_by_state() {
        let running = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "RUNNING cargo test".to_string(),
        };
        let running_lines = codex_log_entry_lines(&running, 80, None);
        assert_eq!(running_lines[0].line.spans[1].content.as_ref(), "•");
        assert_eq!(running_lines[0].line.spans[1].style.fg, Some(COLOR_WARN));

        let ok = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "exec_command_end (exit 0): cargo test\nok".to_string(),
        };
        let ok_lines = codex_log_entry_lines(&ok, 80, None);
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

        let lines = codex_log_entry_lines(&entry, 240, None);
        let rendered = lines
            .iter()
            .map(|line| line_text(&line.line))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("完了 | • 実行: curl"));
        assert!(rendered.contains("└ 出力1行"));
        assert!(!rendered.contains("└ 0123456789"));
        assert!(!rendered.contains(&"0123456789 ".repeat(25)));
        assert!(!rendered.contains("RUNNING"));
        assert!(!rendered.contains("OK"));
        assert!(!rendered.contains("FAIL"));
    }

    #[test]
    fn codex_log_entry_lines_marks_code_edits_like_codex() {
        let entry = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "EDIT *** Begin Patch\n*** Update File: src/tui/ui.rs\n@@\n-old line\n+new line\n*** End Patch".to_string(),
        };

        let lines = codex_log_entry_lines(&entry, 80, None);

        assert_eq!(line_text(&lines[0].line), "編集 | 更新: src/tui/ui.rs");
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
    fn code_edit_diff_preview_caps_and_reports_omitted() {
        let mut text = String::from("*** Update File: big.rs\n");
        for i in 0..20 {
            text.push_str(&format!("+line{i}\n"));
        }
        let preview = code_edit_diff_preview(&text, CODEX_EDIT_DIFF_PREVIEW_LINES);
        // 8 行までのプレビュー + 省略行。
        assert_eq!(preview.len(), CODEX_EDIT_DIFF_PREVIEW_LINES + 1);
        assert_eq!(preview[0], "+line0");
        assert_eq!(
            preview[CODEX_EDIT_DIFF_PREVIEW_LINES],
            format!("... 差分行を{}行省略", 20 - CODEX_EDIT_DIFF_PREVIEW_LINES)
        );
    }

    #[test]
    fn summarize_tool_display_text_highlights_cargo_test_result() {
        let text = summarize_tool_display_text("cargo test\ntest result: ok. 86 passed; 0 failed;");

        assert_eq!(text, "• 実行: cargo test\n  └ テスト: 成功");
    }

    #[test]
    fn summarize_tool_display_text_uses_codex_like_tree_output() {
        let text = summarize_tool_display_text(
            "cargo fmt -- --check\nDiff in /repo/src/tui/ui.rs:3163:\n-old\n+new",
        );

        assert_eq!(text, "• 実行: cargo fmt\n  └ 出力3行");
    }

    #[test]
    fn summarize_tool_display_text_highlights_addness_json_title() {
        let text = summarize_tool_display_text(
            r#"addness goal get goal-1 --json
{"id":"goal-1","title":"AddnessTUI改善"}"#,
        );

        assert_eq!(
            text,
            "• 実行: addness goal get goal-1 --json\n  └ addness: ゴール"
        );
    }

    #[test]
    fn summarize_tool_display_text_keeps_addness_bin_subcommand() {
        let text = summarize_tool_display_text(
            r#""$ADDNESS_BIN" goal get goal-1 --json
{"id":"goal-1","title":"AddnessTUI改善"}"#,
        );

        assert_eq!(
            text,
            "• 実行: addness goal get goal-1 --json\n  └ addness: ゴール"
        );
    }

    #[test]
    fn summarize_tool_display_text_keeps_prompted_addness_subcommand() {
        let text = summarize_tool_display_text(
            r#"$ addness link progress --goal goal-1 --message ok --json
{"name":"progress"}"#,
        );

        assert_eq!(
            text,
            "• 実行: addness link progress --goal goal-1 --message ok --json\n  └ addness: 1項目"
        );
    }

    #[test]
    fn summarize_tool_display_text_highlights_codex_management_output() {
        let text = summarize_tool_display_text("OK codex mcp list\nserver-a\nserver-b");

        assert_eq!(text, "• 実行: codex mcp\n  └ mcp: 2行");
    }

    #[test]
    fn summarize_tool_display_text_highlights_codex_management_json() {
        let text = summarize_tool_display_text(
            r#"OK codex cloud list
[{"id":"task-1"},{"id":"task-2"}]"#,
        );

        assert_eq!(text, "• 実行: codex cloud\n  └ cloud: 一覧2件");
    }

    #[test]
    fn codex_header_line_shows_filter_and_search_state() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.cycle_log_filter();
        pane.begin_search();
        assert!(pane.handle_search_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)));

        let text = line_text(&codex_header_line(&pane, 200));

        assert!(text.contains("状態: 完了"));
        assert!(text.contains("表示:実行"));
        assert!(text.contains("検索:c*"));
        assert!(text.contains("Ctrl-T:表示切替"));
        assert!(!text.contains("filter:"));
    }

    #[test]
    fn codex_header_lines_make_current_activity_primary() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.set_status_note("ファイルを確認中");

        let lines = codex_header_lines(&pane, 120, 2)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("今"));
        assert!(lines[0].contains("ファイルを確認中"));
        assert!(lines[1].contains("状態: 入力待ち"));
        assert!(lines[1].contains("表示:会話"));
    }

    #[test]
    fn codex_diff_lines_color_additions_and_deletions() {
        let lines = codex_diff_lines("@@\n-old\n+new", 80);
        let rendered = lines
            .iter()
            .map(|line| line_text(&line.line))
            .collect::<Vec<_>>();

        assert!(rendered.contains(&"差分 | @@".to_string()));
        assert!(!rendered.iter().any(|line| line.starts_with("DIFF | ")));
        assert_eq!(lines[1].line.spans[1].style.fg, Some(COLOR_DANGER));
        assert_eq!(lines[2].line.spans[1].style.fg, Some(COLOR_SUCCESS));
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

        assert_eq!(lines[0], "確認待ち: Yes/No 続行しますか?");
        assert!(lines[1].contains("[Y] Yes"));
        assert!(lines[1].contains("[N] No"));
        assert!(!lines[1].contains("これからずっと許可"));
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
        assert!(rendered.contains("[L] これからずっと許可"));
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

        assert!(codex_decision_title_hint(AgentKind::Codex, &approval).contains("l:ずっと許可"));
        assert!(!codex_decision_title_hint(AgentKind::Codex, &yes_no).contains("ずっと許可"));
        assert!(codex_decision_title_hint(AgentKind::Codex, &yes_no).contains("y/n"));
        assert!(codex_decision_title_hint(AgentKind::Codex, &approval).contains("下の確認欄"));
        assert!(!codex_decision_title_hint(AgentKind::Codex, &approval).contains("上部で内容確認"));
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
        assert!(line_text(&lines[1]).contains("[L] これからずっと許可"));
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
        assert!(text.contains("[L] これからずっと許可"));
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
        assert!(!text.contains("これからずっと許可"));
    }

    #[test]
    fn command_output_lines_stay_readable() {
        let entry = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "exec_command_end (exit 0): cargo test\nok".to_string(),
        };
        let lines = codex_log_entry_lines(&entry, 80, None);

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
        let lines = codex_log_entry_lines(&entry, 80, None);

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
        let lines = codex_log_entry_lines(&entry, 80, None);
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
    fn system_log_lines_use_unlabeled_muted_prefix() {
        let entry = CodexLogLine {
            kind: CodexLogKind::System,
            text: "Codex セッションを開始しました".to_string(),
        };
        let lines = codex_log_entry_lines(&entry, 80, None);

        assert!(line_text(&lines[0].line).starts_with("     | "));
        assert!(!line_text(&lines[0].line).starts_with("INFO | "));
    }

    #[test]
    fn codex_log_lines_adds_turn_separator_without_tool_separator() {
        let turn = CodexLogLine {
            kind: CodexLogKind::Turn,
            text: "Turn 1".to_string(),
        };
        let tool = CodexLogLine {
            kind: CodexLogKind::Tool,
            text: "exec_command_begin: cargo test".to_string(),
        };
        let lines = codex_log_lines(&[&turn, &tool], 24, None)
            .into_iter()
            .map(|line| line_text(&line.line))
            .collect::<Vec<_>>();

        assert!(lines.iter().any(|line| line.starts_with("-----+ ")));
        assert!(!lines.iter().any(|line| line.starts_with(".....+ ")));
        assert!(lines.iter().any(|line| line.starts_with("作業 | ")));
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
        let full = codex_log_lines(&refs, 40, None);
        let (visible, total) = codex_visible_log_lines(&refs, 40, 3, 5, None);

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
    fn assistant_markdown_cache_returns_same_result_on_hit() {
        let text = "# 見出し\n\nこれは **強調** を含む段落です。\n\n- 項目1\n- 項目2";
        let width = 24;
        let plain = |lines: &[Vec<Span<'static>>]| {
            lines
                .iter()
                .map(|spans: &Vec<Span<'static>>| {
                    spans
                        .iter()
                        .map(|s| s.content.to_string())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        };

        // 2 回呼んでも同一結果（キャッシュヒット）。
        let first = cached_assistant_markdown_lines(text, width);
        let second = cached_assistant_markdown_lines(text, width);
        assert_eq!(plain(&first), plain(&second));

        // カウントと描画行数が一致する（「カウント＝描画行数」不変条件）。
        assert_eq!(cached_assistant_markdown_count(text, width), first.len());

        // 直接レンダリングした結果とも行数が一致する。
        let direct = markdown::render_assistant_markdown(text, width, &codex_markdown_styles());
        assert_eq!(first.len(), direct.len());

        // 幅が変わればキーが変わり、行数も再計算される（折り返しが変わる）。
        let narrow = cached_assistant_markdown_lines(text, 8);
        assert_eq!(cached_assistant_markdown_count(text, 8), narrow.len());
    }

    #[test]
    fn codex_activity_lines_wraps_long_update_rows() {
        let lines = codex_activity_lines(&["abcdefghijklmnopqrstuvwxyz".to_string()], 10);

        assert!(lines.len() >= 3);
        assert_eq!(line_text(&lines[0]), "abcdefghij");
    }

    #[test]
    fn codex_child_goal_lines_show_work_package_dod() {
        let child = ChildGoal {
            id: "goal-1".to_string(),
            title: "承認UIを整理".to_string(),
            description: Some("ユーザーが何を承認するか一目で分かる状態".to_string()),
            icon: "[~]",
            status_label: "進行中".to_string(),
            is_completed: false,
            new_until: None,
        };

        let lines = codex_child_goal_lines(&child, Instant::now(), 1, false)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], " 1. [~] 承認UIを整理  進行中");
        assert!(lines[1].contains("DoD"));
        assert!(lines[1].contains("ユーザーが何を承認するか"));
    }

    #[test]
    fn codex_child_goal_lines_skip_empty_dod() {
        let child = ChildGoal {
            id: "goal-1".to_string(),
            title: "小さい修正".to_string(),
            description: Some("  ".to_string()),
            icon: "[ ]",
            status_label: "未着手".to_string(),
            is_completed: false,
            new_until: None,
        };

        assert_eq!(
            codex_child_goal_lines(&child, Instant::now(), 1, false).len(),
            1
        );
    }

    #[test]
    fn codex_child_goal_lines_mark_active_work_package() {
        let child = ChildGoal {
            id: "goal-1".to_string(),
            title: "子ゴール着手導線".to_string(),
            description: Some("/work nextで着手できる状態".to_string()),
            icon: "[ ]",
            status_label: "未着手".to_string(),
            is_completed: false,
            new_until: None,
        };

        let lines = codex_child_goal_lines(&child, Instant::now(), 2, true)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();

        assert_eq!(lines[0], "> 2. [ ] 子ゴール着手導線  未着手");
        assert!(lines[1].contains("/work next"));
    }

    #[test]
    fn codex_runtime_status_shows_current_action() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.set_status_note("ゴール文脈を読込中");

        let status = codex_runtime_status(&pane, 80);

        assert!(status.contains("入力待ち"));
        assert!(status.contains("表示:会話"));
        assert!(status.contains("ゴール文脈を読込中"));
    }

    #[test]
    fn codex_recent_action_visible_count_scales_with_panel_height() {
        // パネルが低いときは直近アクションのパンくずを表示せず、他の固定セクションを圧迫しない。
        assert_eq!(codex_recent_action_visible_count(0), 0);
        assert_eq!(codex_recent_action_visible_count(8), 0);
        // 実際の呼び出し元で使われる高さ（8/10/12）では 0〜3 件に収まる。
        assert_eq!(codex_recent_action_visible_count(9), 2);
        assert_eq!(codex_recent_action_visible_count(10), 2);
        assert_eq!(codex_recent_action_visible_count(11), 3);
        assert_eq!(codex_recent_action_visible_count(12), 3);
        assert_eq!(codex_recent_action_visible_count(13), 3);
        // 高さが増えるにつれ、リングバッファの保持上限 5 件まで段階的に表示件数を増やす。
        assert_eq!(codex_recent_action_visible_count(14), 4);
        assert_eq!(codex_recent_action_visible_count(17), 4);
        assert_eq!(codex_recent_action_visible_count(18), 5);
        assert_eq!(codex_recent_action_visible_count(u16::MAX), 5);
    }

    #[test]
    fn codex_header_lines_show_running_subagent_count() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.test_add_running_subagent("調査タスク");
        pane.test_add_running_subagent("実装タスク");

        let text = line_text(&codex_header_line(&pane, 200));
        assert!(text.contains("Sub:2"));
    }

    #[test]
    fn codex_header_lines_omit_sub_count_when_no_subagent_running() {
        let pane = CodexPane::test_with_output(8, 80, 0, "");
        let text = line_text(&codex_header_line(&pane, 200));
        assert!(!text.contains("Sub:"));
    }

    #[test]
    fn codex_subagent_visible_count_scales_with_panel_height() {
        assert_eq!(codex_subagent_visible_count(0), 0);
        assert_eq!(codex_subagent_visible_count(8), 0);
        assert_eq!(codex_subagent_visible_count(9), 1);
        assert_eq!(codex_subagent_visible_count(10), 1);
        assert_eq!(codex_subagent_visible_count(11), 2);
        assert_eq!(codex_subagent_visible_count(13), 2);
        assert_eq!(codex_subagent_visible_count(14), 3);
        assert_eq!(codex_subagent_visible_count(17), 3);
        assert_eq!(codex_subagent_visible_count(18), 5);
        assert_eq!(codex_subagent_visible_count(u16::MAX), 5);
    }

    #[test]
    fn run_spinner_glyph_cycles_through_all_frames_and_wraps() {
        // 純関数: 同じtickは同じ文字を返し、周期(フレーム数)で元へ戻る。
        let frames: Vec<&str> = (0..RUN_SPINNER_FRAMES.len() as u64)
            .map(run_spinner_glyph)
            .collect();
        assert_eq!(frames, RUN_SPINNER_FRAMES.to_vec());
        assert_eq!(
            run_spinner_glyph(RUN_SPINNER_FRAMES.len() as u64),
            run_spinner_glyph(0)
        );
        assert_eq!(run_spinner_glyph(7), run_spinner_glyph(7));
    }

    #[test]
    fn codex_current_activity_label_shows_spinner_while_turn_running() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.test_set_turn_running(true);
        pane.set_work_action("作業中");

        let label = codex_current_activity_label(&pane, 80);
        assert!(label.starts_with(run_spinner_glyph(0)));

        pane.test_set_turn_running(false);
        let idle_label = codex_current_activity_label(&pane, 80);
        assert!(!idle_label.starts_with(run_spinner_glyph(0)));
    }

    #[test]
    fn codex_current_activity_label_marks_confirming_state_distinctly() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.test_set_turn_running(true);
        pane.test_set_pending_decision(CodexDecisionKind::Approval, "Run cargo test?");

        let label = codex_current_activity_label(&pane, 80);
        assert!(label.contains("⏸ 承認待ち"));
        assert!(label.contains("Run cargo test?"));
    }

    #[test]
    fn codex_command_activity_summary_groups_commands_for_main_view() {
        let summary = super::codex_command_activity_summary(
            "cargo test --workspace --all-features -- --nocapture very_long_filter",
        );

        assert_eq!(summary, "テストを実行中 (cargo test)");
        assert!(!summary.contains("--workspace"));
        assert_eq!(
            super::codex_command_activity_summary(
                "*** Begin Patch\n*** Update File: src/tui/ui.rs\n@@\n*** End Patch",
            ),
            "ファイルを編集中 (src/tui/ui.rs)"
        );
    }

    #[test]
    fn codex_work_label_prefers_status_note_over_last_prompt() {
        assert_eq!(
            codex_work_label(false, false, Some("model: gpt-5"), Some("実装して"), 80),
            "model: gpt-5"
        );
    }

    #[test]
    fn codex_work_label_uses_last_prompt_when_no_status_note() {
        let label = codex_work_label(false, false, None, Some("この不具合を直して"), 80);

        assert_eq!(label, "依頼対応: この不具合を直して");
    }

    #[test]
    fn codex_current_activity_label_falls_back_command_then_work_action_then_generic() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.test_set_turn_running(true);

        // work_action も current_command も無い → 汎用文言。
        let generic = codex_current_activity_label(&pane, 120);
        assert!(generic.contains("考えています"), "label={generic}");

        // work_action があればそれを使う。
        pane.set_work_action("ツール実行: Bash");
        let with_action = codex_current_activity_label(&pane, 120);
        assert!(
            with_action.contains("作業中: ツール実行: Bash"),
            "label={with_action}"
        );

        // current_command があれば work_action より優先する。
        pane.test_set_current_command("cargo test");
        let with_command = codex_current_activity_label(&pane, 120);
        assert!(
            with_command.contains("テストを実行中"),
            "label={with_command}"
        );
        assert!(!with_command.contains("作業中:"), "label={with_command}");
    }

    #[test]
    fn codex_current_activity_label_uses_status_note_when_idle() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.set_status_note("model: gpt-5");
        // 実行中に残った作業インジケータがあっても、非実行中の表示には使わない。
        pane.set_work_action("ツール実行: Bash");

        let label = codex_current_activity_label(&pane, 120);
        assert_eq!(label, "model: gpt-5");
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
            text.contains("Ctrl+Q"),
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

        assert!(text.contains("次の操作"));
        assert!(text.contains("Ctrl+Q"));
        assert!(text.contains("F7:turn一覧"));
        assert!(text.contains("/organize"));
        assert!(text.contains("/work next"));
        assert!(text.contains("/dual"));
        assert!(text.contains("/remember"));
        assert!(text.contains("/handoff"));
        assert!(text.contains("/settings"));
        assert!(!text.contains("初回は子ゴール"));
        assert!(!text.contains("/mcp"));
        assert!(!text.contains("/cloud"));
        assert!(!text.contains("Ctrl-O"));
    }
}
