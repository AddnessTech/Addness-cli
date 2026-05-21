use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use std::collections::HashMap;

use super::app::{ActivePane, App, FormField, ModalState};
use super::goal_tree::TreeRow;
use crate::api::{DeliverableType, GoalStatus, Member, MemberId};

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

    draw_title_bar(frame, main_layout[0]);

    let content_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(0)])
        .split(main_layout[1]);

    draw_left_panel(frame, content_layout[0], app);
    draw_content(frame, content_layout[1], app);
    draw_status_bar(frame, main_layout[2], app);

    if app.show_org_popup {
        draw_org_popup(frame, app);
    }

    // Draw modals last (on top of everything)
    if let Some(ref modal) = app.modal_state {
        match modal {
            ModalState::CreateGoal { .. } => draw_create_goal_modal(frame, app),
            ModalState::EditGoal { .. } => draw_edit_goal_modal(frame, app),
            ModalState::DeleteGoal { .. } => draw_delete_goal_modal(frame, app),
        }
    }
}

fn draw_title_bar(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new(Line::from(vec![
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
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
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

    let visible = rows.iter().enumerate().skip(scroll).take(viewport_h);

    for (i, row) in visible {
        let y = inner.y + (i - scroll) as u16;
        if y >= inner.y + inner.height {
            break;
        }
        let line_area = Rect::new(inner.x, y, inner.width, 1);
        let is_cursor = i == cursor;

        let line = render_tree_row(row, is_cursor, inner.width as usize, &app.members);
        frame.render_widget(Paragraph::new(line), line_area);
    }
}

fn render_tree_row(
    row: &TreeRow,
    is_cursor: bool,
    width: usize,
    members: &HashMap<MemberId, Member>,
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
            depth,
            ..
        } => {
            let indent = "  ".repeat(*depth);
            let icon = if *expanded { "- " } else { "+ " };

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
                Span::styled(
                    format!("{indent}{icon}"),
                    Style::default().fg(Color::Cyan).bg(bg),
                ),
                Span::styled(title.to_string(), title_style),
            ];

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
                let content_len: usize = spans.iter().map(|s| s.content.len()).sum();
                if content_len < width {
                    spans.push(Span::styled(
                        " ".repeat(width - content_len),
                        Style::default().bg(bg),
                    ));
                }
            }

            Line::from(spans)
        }
        TreeRow::Detail {
            status,
            owner_name,
            description,
            is_completed,
            depth,
        } => {
            let indent = "  ".repeat(*depth);
            let status_str = format_status(*is_completed, *status);
            let owner_str = owner_name.unwrap_or("-");
            let desc = description.unwrap_or("");

            let text = if desc.is_empty() {
                format!("{indent}  {status_str} | {owner_str}")
            } else {
                format!("{indent}  {status_str} | {owner_str} | {desc}")
            };

            let mut spans = vec![Span::styled(
                text.clone(),
                Style::default().fg(Color::DarkGray).bg(bg),
            )];
            if is_cursor {
                pad_line(&mut spans, text.len(), width, bg);
            }
            Line::from(spans)
        }
        TreeRow::CommentHeader { count, depth } => {
            let indent = "  ".repeat(*depth);
            let text = format!(
                "{indent}  \u{1F4DD} {count} comment{}",
                if *count != 1 { "s" } else { "" }
            );

            let mut spans = vec![Span::styled(
                text.clone(),
                Style::default().fg(Color::Yellow).bg(bg),
            )];
            if is_cursor {
                pad_line(&mut spans, text.len(), width, bg);
            }
            Line::from(spans)
        }
        TreeRow::CommentOmitted { count, depth } => {
            let indent = "  ".repeat(*depth);
            let text = format!(
                "{indent}  ... {count} older comment{} hidden",
                if *count != 1 { "s" } else { "" }
            );

            let mut spans = vec![Span::styled(
                text.clone(),
                Style::default().fg(Color::DarkGray).bg(bg),
            )];
            if is_cursor {
                pad_line(&mut spans, text.len(), width, bg);
            }
            Line::from(spans)
        }
        TreeRow::CommentItem { comment, depth } => {
            let indent = "  ".repeat(*depth);
            let author = &comment.author.name;

            // Replace @uuid mentions with @member_name
            let content_with_mentions = replace_member_mentions(&comment.content, members);

            let content = truncate_str(
                &content_with_mentions,
                width.saturating_sub(indent.len() + author.len() + 4),
            );
            let text_len = indent.len() + author.len() + 2 + content.len();

            let mut spans = vec![
                Span::styled(format!("{indent}  "), Style::default().bg(bg)),
                Span::styled(
                    format!("{author}:"),
                    Style::default().fg(Color::Cyan).bg(bg),
                ),
                Span::styled(
                    format!(" {content}"),
                    Style::default().fg(Color::White).bg(bg),
                ),
            ];
            if is_cursor {
                pad_line(&mut spans, text_len + 1, width, bg);
            }
            Line::from(spans)
        }
        TreeRow::DeliverableHeader { count, depth } => {
            let indent = "  ".repeat(*depth);
            let text = format!(
                "{indent}  \u{1F4CE} {count} deliverable{}",
                if *count != 1 { "s" } else { "" }
            );

            let mut spans = vec![Span::styled(
                text.clone(),
                Style::default().fg(Color::Magenta).bg(bg),
            )];
            if is_cursor {
                pad_line(&mut spans, text.len(), width, bg);
            }
            Line::from(spans)
        }
        TreeRow::DeliverableItem { deliverable, depth } => {
            let indent = "  ".repeat(*depth);
            let icon = match deliverable.node_type {
                DeliverableType::Document => "\u{1F4C4}",
                DeliverableType::Folder => "\u{1F4C1}",
                DeliverableType::File => "\u{1F4CE}",
                DeliverableType::Link => "\u{1F517}",
            };
            let text = format!("{indent}  {icon} {}", deliverable.display_name);

            let mut spans = vec![Span::styled(
                text.clone(),
                Style::default().fg(Color::White).bg(bg),
            )];
            if is_cursor {
                pad_line(&mut spans, text.len(), width, bg);
            }
            Line::from(spans)
        }
    }
}

fn format_status(is_completed: bool, status: Option<&GoalStatus>) -> &'static str {
    if is_completed {
        "✅ 完了"
    } else {
        match status {
            Some(GoalStatus::InProgress) => "⏩ 進行中",
            Some(GoalStatus::Cancelled) => "⏸ 停止中",
            _ => "🔵 未着手",
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
    if s.len() <= max {
        s.to_string()
    } else if max > 3 {
        let end = s
            .char_indices()
            .nth(max - 3)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}...", &s[..end])
    } else {
        s.chars().take(max).collect()
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
    let pane_label = match app.active_pane {
        ActivePane::OrgSelector => "Org",
        ActivePane::Navigation => "Nav",
        ActivePane::Content => "Content",
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
// Org selection popup
// ---------------------------------------------------------------------------

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_width = area.width * percent_x / 100;
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, popup_width, height.min(area.height))
}

fn draw_org_popup(frame: &mut Frame, app: &App) {
    let item_count = app.orgs.len() as u16;
    // border(2) + header(1) + items
    let popup_height = item_count + 3;
    let area = centered_rect(40, popup_height, frame.area());

    frame.render_widget(Clear, area);

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
    frame.render_widget(Clear, area);

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

    // Title field
    let title_focused = *current_field == FormField::Title;
    let title_border = if title_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let title_widget = Paragraph::new(title.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(title_border))
            .title(" Title * "),
    );
    frame.render_widget(title_widget, field_layout[0]);

    // Description field
    let desc_focused = *current_field == FormField::Description;
    let desc_border = if desc_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let desc_widget = Paragraph::new(description.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(desc_border))
            .title(" Description "),
    );
    frame.render_widget(desc_widget, field_layout[1]);

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
    frame.render_widget(Clear, area);

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

    // Title field
    let title_focused = *current_field == FormField::Title;
    let title_border = if title_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let title_widget = Paragraph::new(title.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(title_border))
            .title(" Title * "),
    );
    frame.render_widget(title_widget, field_layout[0]);

    // Description field
    let desc_focused = *current_field == FormField::Description;
    let desc_border = if desc_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let desc_widget = Paragraph::new(description.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(desc_border))
            .title(" Description "),
    );
    frame.render_widget(desc_widget, field_layout[1]);

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
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(" ⚠️  削除の確認 ")
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
        Span::styled("⚠️  ", Style::default().fg(Color::Red)),
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

            let prefix = if is_current_user { "👤 " } else { "  " };
            let content = format!("{}{}", prefix, member.name);

            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(visible_members);
    frame.render_widget(list, inner_area);
}
