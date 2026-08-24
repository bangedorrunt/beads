//! Frame rendering for the TUI skeleton (bead gu7ts.6). governed-by: ADR-0003.
//!
//! Screen = body (height-1) + footer (1 line), per bv UX map §1.2. The body
//! is a split list+detail above width 100 and single-column below; the detail
//! layer replaces the whole body in non-split mode.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::app::{Focus, TuiApp};
use super::theme;

/// Render one frame.
pub fn draw(frame: &mut Frame, app: &TuiApp) {
    let area = frame.area();
    let [body, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

    match app.focus() {
        Focus::Detail if !app.is_split() => draw_detail(frame, body, app),
        _ => {
            if app.is_split() {
                let [list, detail] =
                    Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .areas(body);
                draw_list(frame, list, app);
                draw_detail(frame, detail, app);
            } else {
                draw_list(frame, body, app);
            }
        }
    }

    if app.focus() == Focus::QuitConfirm {
        draw_quit_confirm(frame, area);
    }

    draw_footer(frame, footer, app);
}

fn draw_list(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let header = Line::from(Span::styled(
        "  TYPE PRI STATUS  ID             TITLE",
        theme::primary(),
    ));
    let rows = std::iter::once(header)
        .chain(app.issues().iter().enumerate().map(|(index, issue)| {
            let selector = if index == app.selected() { "> " } else { "  " };
            let style = if index == app.selected() {
                theme::selected_row()
            } else {
                theme::row()
            };
            Line::from(vec![
                Span::styled(selector.to_string(), style),
                Span::styled(format!("{:<4}", issue.issue_type.as_str()), style),
                Span::styled(format!("P{:<3}", issue.priority.0), style),
                Span::styled(format!("{:<7}", issue.status.as_str()), status_style(issue)),
                Span::styled(format!("{:<14}", issue.id), style),
                Span::styled(issue.title.clone(), style),
            ])
        }))
        .collect::<Vec<_>>();

    frame.render_widget(Paragraph::new(rows), area);
}

fn draw_detail(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let Some(issue) = app.selected_issue() else {
        return;
    };

    let mut lines = vec![
        Line::from(Span::styled(
            format!("# {} {}", issue.issue_type.as_str(), issue.title),
            theme::primary(),
        )),
        Line::from(""),
        Line::from(format!("ID:       {}", issue.id)),
        Line::from(format!("Status:   {}", issue.status.as_str())),
        Line::from(format!("Priority: P{}", issue.priority.0)),
        Line::from(format!(
            "Assignee: {}",
            issue.assignee.as_deref().unwrap_or("-")
        )),
        Line::from(""),
    ];
    if let Some(description) = &issue.description {
        lines.push(Line::from(description.clone()));
    }

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::LEFT)),
        area,
    );
}

fn draw_quit_confirm(frame: &mut Frame, area: ratatui::layout::Rect) {
    // Centered rounded box over everything (bv modal rule §1.7).
    let popup = centered_rect(area, 40, 5);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme::danger_border()),
        popup,
    );
    let inner_area = ratatui::layout::Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: popup.width.saturating_sub(4),
        height: popup.height.saturating_sub(2),
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Quit br?"),
            Line::from("Y to quit / any other key to cancel"),
        ]),
        inner_area,
    );
}

fn draw_footer(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    // Status message replaces the whole bar when set (bv §1.6).
    if let Some(message) = app.status_message() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("✓ {message}"),
                Style::new().fg(Color::Green),
            ))),
            area,
        );
        return;
    }

    let open = app
        .issues()
        .iter()
        .filter(|issue| issue.status.as_str() == "open")
        .count();
    let closed = app
        .issues()
        .iter()
        .filter(|issue| issue.status.as_str() == "closed")
        .count();
    let hint = match app.focus() {
        Focus::List => "j/k move • enter detail • q quit",
        Focus::Detail => "q/esc back • tab pane",
        Focus::QuitConfirm => "y quit • other cancel",
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("{hint} │ "), theme::dim()),
            Span::styled(format!("○{open} "), theme::status_open()),
            Span::styled(format!("●{closed} "), theme::status_closed()),
            Span::styled(format!("{} issues", app.issues().len()), theme::dim()),
        ])),
        area,
    );
}

fn status_style(issue: &crate::model::Issue) -> Style {
    match issue.status.as_str() {
        "open" => theme::status_open(),
        "in_progress" => theme::status_in_progress(),
        "closed" | "tombstone" => theme::status_closed(),
        _ => theme::status_blocked(),
    }
}

fn centered_rect(
    area: ratatui::layout::Rect,
    percent_x: u16,
    height: u16,
) -> ratatui::layout::Rect {
    let popup_y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_x = area.x + (area.width.saturating_sub(percent_x)) / 2;
    ratatui::layout::Rect {
        x: popup_x,
        y: popup_y,
        width: percent_x.min(area.width),
        height: height.min(area.height),
    }
}
