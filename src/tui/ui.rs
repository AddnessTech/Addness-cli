use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use super::app::{ActivePane, App};
use super::goal_tree::TreeRow;
use crate::api::{DeliverableType, GoalStatus};

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
            .title(" addness v0.1.0 "),
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
    let border_color = if is_active { Color::Cyan } else { Color::DarkGray };

    let org_name = app.current_org_name();
    let content = if is_active {
        Line::from(vec![
            Span::styled(
                format!(" {org_name} "),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " <Enter>",
                Style::default().fg(Color::DarkGray),
            ),
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
    let highlight_color = if is_active { Color::Cyan } else { Color::DarkGray };

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
        0 => draw_goals(frame, area, app, border_color),
        1 => draw_comments(frame, area, border_color),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Goal tree rendering
// ---------------------------------------------------------------------------

fn draw_goals(frame: &mut Frame, area: Rect, app: &mut App, border_color: Color) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" Goals ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let viewport_h = inner.height as usize;
    app.content_height = viewport_h;
    app.goal_tree.adjust_scroll(viewport_h);

    let rows = app.goal_tree.flatten();
    let scroll = app.goal_tree.scroll_offset;

    let visible = rows
        .iter()
        .enumerate()
        .skip(scroll)
        .take(viewport_h);

    for (i, row) in visible {
        let y = inner.y + (i - scroll) as u16;
        if y >= inner.y + inner.height {
            break;
        }
        let line_area = Rect::new(inner.x, y, inner.width, 1);
        let is_cursor = i == app.goal_tree.cursor;

        let line = render_tree_row(row, is_cursor, inner.width as usize);
        frame.render_widget(Paragraph::new(line), line_area);
    }
}

fn render_tree_row(row: &TreeRow, is_cursor: bool, width: usize) -> Line<'static> {
    let bg = if is_cursor {
        Color::DarkGray
    } else {
        Color::Reset
    };

    match row {
        TreeRow::Goal { node, depth } => {
            let indent = "  ".repeat(*depth);
            let icon = if node.summary.has_children() || node.children.is_some() {
                if node.expanded { "▼ " } else { "▶ " }
            } else {
                "  "
            };

            let title = node.summary.title();
            let status_str = format_status(node.summary.status());
            let owner_str = node
                .summary
                .owner()
                .map(|o| o.name.as_str())
                .unwrap_or("");

            let completed = node.summary.is_completed();
            let title_style = if completed {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::CROSSED_OUT)
                    .bg(bg)
            } else {
                Style::default().fg(Color::White).bg(bg)
            };

            let mut spans = vec![
                Span::styled(format!("{indent}{icon}"), Style::default().fg(Color::Cyan).bg(bg)),
                Span::styled(title.to_string(), title_style),
            ];

            // Append status + owner inline if there's room
            let meta = format_goal_meta(&status_str, owner_str);
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
        TreeRow::Detail { goal, depth } => {
            let indent = "  ".repeat(*depth);
            let status_str = format_status(goal.status.as_ref());
            let owner_str = goal.owner.as_ref().map(|o| o.name.as_str()).unwrap_or("-");
            let due = goal.due_date.as_deref().unwrap_or("-");

            let text = format!("{indent}  {status_str} | {owner_str} | Due: {due}");

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
            let text = format!("{indent}  \u{1F4DD} {count} comment{}", if *count != 1 { "s" } else { "" });

            let mut spans = vec![Span::styled(
                text.clone(),
                Style::default().fg(Color::Yellow).bg(bg),
            )];
            if is_cursor {
                pad_line(&mut spans, text.len(), width, bg);
            }
            Line::from(spans)
        }
        TreeRow::CommentItem { comment, depth } => {
            let indent = "  ".repeat(*depth);
            let author = &comment.author.name;
            let content = truncate_str(&comment.content, width.saturating_sub(indent.len() + author.len() + 4));
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

fn format_status(status: Option<&GoalStatus>) -> &'static str {
    match status {
        Some(GoalStatus::Active) => "Active",
        Some(GoalStatus::InProgress) => "InProgress",
        Some(GoalStatus::Completed) => "Completed",
        Some(GoalStatus::Cancelled) => "Cancelled",
        Some(GoalStatus::None) | None => "-",
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
        format!("{}...", &s[..max - 3])
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
// Comments (unchanged mock)
// ---------------------------------------------------------------------------

fn draw_comments(frame: &mut Frame, area: Rect, border_color: Color) {
    let text = vec![
        Line::from(vec![
            Span::styled(
                "user1",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" - 2025-04-01 10:30", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from("  Initial project setup looks great. Let's proceed."),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "user2",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" - 2025-04-02 14:15", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from("  Agreed. I'll start working on the API endpoints."),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "user1",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" - 2025-04-03 09:00", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from("  Don't forget to add error handling for edge cases."),
    ];

    let paragraph = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" Comments (mock) "),
    );
    frame.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
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

    if app.active_pane == ActivePane::Content && app.sidebar_index == 0 {
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
            let marker = if i == app.current_org_index {
                " *"
            } else {
                ""
            };
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
            .title_bottom(Line::from(" Enter: select | Esc: cancel ").style(Style::default().fg(Color::DarkGray))),
    );
    frame.render_widget(popup, area);
}
