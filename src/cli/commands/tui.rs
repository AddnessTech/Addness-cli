use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::api::{ApiClient, Comment, GoalStatus, GoalTreeItem};
use crate::cli::commands::org::resolve_org_id;

/// Entry point: `addness tui [--org <id>]`
pub async fn handle_tui(org: Option<&str>, client: &ApiClient) -> Result<()> {
    let org_id = resolve_org_id(org)?;
    let resp = client.get_goal_tree(&org_id, 5).await?;
    let items = resp.data.items;

    if items.is_empty() {
        println!("No goals in organization {org_id}");
        return Ok(());
    }

    let mut terminal = setup_terminal()?;
    let result = run_app(&mut terminal, items, client, &org_id).await;
    restore_terminal(&mut terminal)?;
    result
}

type Backend = CrosstermBackend<Stdout>;

fn setup_terminal() -> Result<Terminal<Backend>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<Backend>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

#[derive(Default)]
struct DetailState {
    comments: Option<Vec<Comment>>,
    /// User-toggled "show comments" panel
    show_comments: bool,
}

async fn run_app(
    terminal: &mut Terminal<Backend>,
    items: Vec<GoalTreeItem>,
    client: &ApiClient,
    org_id: &str,
) -> Result<()> {
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    let mut detail = DetailState::default();
    let mut status_msg = format!("org {org_id} — {} goals — ?:help q:quit", items.len());

    loop {
        terminal.draw(|f| ui(f, &items, &mut list_state, &detail, &status_msg))?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != event::KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                break;
            }
            KeyCode::Down | KeyCode::Char('j') => move_selection(&mut list_state, &items, 1),
            KeyCode::Up | KeyCode::Char('k') => move_selection(&mut list_state, &items, -1),
            KeyCode::Char('g') => list_state.select(Some(0)),
            KeyCode::Char('G') => list_state.select(Some(items.len().saturating_sub(1))),
            KeyCode::Char('c') => {
                // Toggle comment panel: load comments on first show.
                detail.show_comments = !detail.show_comments;
                if detail.show_comments {
                    if let Some(idx) = list_state.selected()
                        && let Some(item) = items.get(idx)
                    {
                        match client.list_comments(&item.id).await {
                            Ok(resp) => {
                                detail.comments = Some(resp.comments);
                                status_msg = "comments loaded — c:close".into();
                            }
                            Err(e) => {
                                detail.comments = Some(vec![]);
                                status_msg = format!("failed to load comments: {e}");
                            }
                        }
                    }
                } else {
                    status_msg = "comment panel closed".into();
                }
            }
            KeyCode::Char('?') | KeyCode::Char('h') => {
                status_msg =
                    "j/k:move  g/G:top/bottom  c:comments  q:quit  Esc:quit".to_string();
            }
            _ => {}
        }

        // Reset comments when selection changes.
        if let Some(idx) = list_state.selected()
            && let Some(_item) = items.get(idx)
        {
            // Currently we only clear when comment panel was closed.
            if !detail.show_comments {
                detail.comments = None;
            }
        }
    }

    Ok(())
}

fn move_selection(state: &mut ListState, items: &[GoalTreeItem], delta: isize) {
    if items.is_empty() {
        return;
    }
    let current = state.selected().unwrap_or(0) as isize;
    let len = items.len() as isize;
    let new = (current + delta).clamp(0, len - 1);
    state.select(Some(new as usize));
}

fn ui(
    f: &mut ratatui::Frame,
    items: &[GoalTreeItem],
    list_state: &mut ListState,
    detail: &DetailState,
    status_msg: &str,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[0]);

    draw_list(f, body_chunks[0], items, list_state);
    draw_detail(f, body_chunks[1], items, list_state, detail);
    draw_status(f, chunks[1], status_msg);
}

fn draw_list(
    f: &mut ratatui::Frame,
    area: Rect,
    items: &[GoalTreeItem],
    list_state: &mut ListState,
) {
    // Reconstruct ancestry by parent_id to compute depth visually.
    let list_items: Vec<ListItem> = items
        .iter()
        .map(|g| {
            let depth = compute_depth(g, items);
            let indent = " ".repeat(depth * 2);
            let status_icon = if g.is_completed {
                "✓"
            } else {
                match &g.status {
                    Some(GoalStatus::InProgress) => "▶"
                    , Some(GoalStatus::Cancelled) => "✗"
                    , _ => "·"
                }
            };
            let line = Line::from(vec![
                Span::raw(indent),
                Span::styled(format!("{status_icon} "), status_color(g)),
                Span::raw(g.title.clone()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(list_items)
        .block(Block::default().borders(Borders::ALL).title("Goals (j/k, ?:help)"))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, area, list_state);
}

fn compute_depth(target: &GoalTreeItem, all: &[GoalTreeItem]) -> usize {
    let mut depth = 0usize;
    let mut current_parent = target.parent_id.clone();
    let mut guard = 0usize;
    while let Some(pid) = current_parent {
        guard += 1;
        if guard > 32 {
            break;
        }
        if let Some(parent) = all.iter().find(|g| g.id == pid) {
            depth += 1;
            current_parent = parent.parent_id.clone();
        } else {
            break;
        }
    }
    depth
}

fn status_color(g: &GoalTreeItem) -> Style {
    if g.is_completed {
        return Style::default().fg(Color::Green);
    }
    match &g.status {
        Some(GoalStatus::InProgress) => Style::default().fg(Color::Yellow),
        Some(GoalStatus::Cancelled) => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::Gray),
    }
}

fn draw_detail(
    f: &mut ratatui::Frame,
    area: Rect,
    items: &[GoalTreeItem],
    list_state: &ListState,
    detail: &DetailState,
) {
    let Some(idx) = list_state.selected() else {
        let p = Paragraph::new("No selection").block(
            Block::default().borders(Borders::ALL).title("Detail"),
        );
        f.render_widget(p, area);
        return;
    };
    let Some(item) = items.get(idx) else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Title: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(item.title.clone()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("ID:    ", Style::default().fg(Color::DarkGray)),
        Span::raw(item.id.clone()),
    ]));
    let owner_label = item
        .owner
        .as_ref()
        .map(|o| o.name.clone())
        .unwrap_or_else(|| "(none)".to_string());
    lines.push(Line::from(vec![
        Span::styled("Owner: ", Style::default().fg(Color::DarkGray)),
        Span::raw(owner_label),
    ]));
    let status_text = if item.is_completed {
        "COMPLETED".to_string()
    } else {
        match &item.status {
            Some(GoalStatus::InProgress) => "IN_PROGRESS".to_string(),
            Some(GoalStatus::Cancelled) => "CANCELLED".to_string(),
            Some(GoalStatus::None) => "NONE".to_string(),
            Some(GoalStatus::Other(s)) => s.clone(),
            None => "(unknown)".to_string(),
        }
    };
    lines.push(Line::from(vec![
        Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
        Span::styled(status_text, status_color(item)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Children: ", Style::default().fg(Color::DarkGray)),
        Span::raw(if item.has_children { "yes" } else { "no" }),
    ]));

    if detail.show_comments {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Comments",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        if let Some(cs) = &detail.comments {
            if cs.is_empty() {
                lines.push(Line::from(Span::styled(
                    "(none)",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for c in cs.iter().take(20) {
                    let resolved = if c.resolved_at.is_some() {
                        " [resolved]"
                    } else {
                        ""
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{}{resolved}", c.author.name),
                        Style::default().fg(Color::Cyan),
                    )));
                    for raw_line in c.content.lines().take(3) {
                        lines.push(Line::from(format!("  {raw_line}")));
                    }
                }
            }
        } else {
            lines.push(Line::from("loading..."));
        }
    }

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Detail"))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn draw_status(f: &mut ratatui::Frame, area: Rect, msg: &str) {
    let p = Paragraph::new(Line::from(Span::styled(
        msg.to_string(),
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(p, area);
}
