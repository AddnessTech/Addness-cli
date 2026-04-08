use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table},
};

use super::app::{ActivePane, App};

pub fn draw(frame: &mut Frame, app: &App) {
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

    draw_sidebar(frame, content_layout[0], app);
    draw_content(frame, content_layout[1], app);
    draw_status_bar(frame, main_layout[2], app);
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

fn draw_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let is_active = app.active_pane == ActivePane::Sidebar;
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

    let sidebar = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if is_active {
                Color::Cyan
            } else {
                Color::DarkGray
            }))
            .title(" Navigation "),
    );
    frame.render_widget(sidebar, area);
}

fn draw_content(frame: &mut Frame, area: Rect, app: &App) {
    let border_color = if app.active_pane == ActivePane::Content {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    match app.sidebar_index {
        0 => draw_organizations(frame, area, border_color),
        1 => draw_goals(frame, area, border_color),
        2 => draw_comments(frame, area, border_color),
        _ => {}
    }
}

fn draw_organizations(frame: &mut Frame, area: Rect, border_color: Color) {
    let rows = vec![
        Row::new(vec![
            Cell::from("org-001"),
            Cell::from("Addness Inc."),
            Cell::from("5 members"),
            Cell::from("Active"),
        ]),
        Row::new(vec![
            Cell::from("org-002"),
            Cell::from("Side Project"),
            Cell::from("2 members"),
            Cell::from("Active"),
        ]),
    ];

    let widths = [
        Constraint::Length(10),
        Constraint::Min(20),
        Constraint::Length(12),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["ID", "Name", "Members", "Status"])
                .style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .bottom_margin(1),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(" Organizations (mock) "),
        );
    frame.render_widget(table, area);
}

fn draw_goals(frame: &mut Frame, area: Rect, border_color: Color) {
    let rows = vec![
        Row::new(vec![
            Cell::from("G-001"),
            Cell::from("Launch MVP"),
            Cell::from("In Progress").style(Style::default().fg(Color::Yellow)),
            Cell::from("2025-06-01"),
        ]),
        Row::new(vec![
            Cell::from("G-002"),
            Cell::from("Setup CI/CD Pipeline"),
            Cell::from("Done").style(Style::default().fg(Color::Green)),
            Cell::from("2025-04-15"),
        ]),
        Row::new(vec![
            Cell::from("G-003"),
            Cell::from("Write API Documentation"),
            Cell::from("Not Started").style(Style::default().fg(Color::Red)),
            Cell::from("2025-07-01"),
        ]),
        Row::new(vec![
            Cell::from("G-004"),
            Cell::from("User Authentication"),
            Cell::from("In Progress").style(Style::default().fg(Color::Yellow)),
            Cell::from("2025-05-20"),
        ]),
        Row::new(vec![
            Cell::from("G-005"),
            Cell::from("Performance Optimization"),
            Cell::from("Not Started").style(Style::default().fg(Color::Red)),
            Cell::from("2025-08-01"),
        ]),
    ];

    let widths = [
        Constraint::Length(8),
        Constraint::Min(25),
        Constraint::Length(14),
        Constraint::Length(12),
    ];

    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["ID", "Title", "Status", "Due Date"])
                .style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .bottom_margin(1),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(" Goals (mock) "),
        );
    frame.render_widget(table, area);
}

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

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let current_section = app.sidebar_items[app.sidebar_index];
    let status = Paragraph::new(Line::from(vec![
        Span::styled(
            " q",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Quit  "),
        Span::styled(
            "↑↓/jk",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Navigate  "),
        Span::styled(
            "Tab",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Switch Pane  "),
        Span::styled("|", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" Section: {current_section} "),
            Style::default().fg(Color::Yellow),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Help "),
    );
    frame.render_widget(status, area);
}
