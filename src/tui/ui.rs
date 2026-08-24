//! Frame rendering for the TUI skeleton + extras (bead gu7ts.6 + gu7ts.8). governed-by: ADR-0003.
//!
//! Screen = body (height-1) + footer (1 line), per bv UX map §1.2. The body
//! is a split list+detail above width 100 and single-column below; the detail
//! layer replaces the whole body in non-split mode. gu7ts.8 adds: search bar,
//! shortcuts sidebar (34 cols, ;/F2), help overlay (?/F1).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::app::{Focus, TuiApp};
use super::{keys, theme};

/// Render one frame.
pub fn draw(frame: &mut Frame, app: &TuiApp) {
    let area = frame.area();
    let [body, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

    // Help overlay replaces body (priority per UX map §1.7).
    if app.focus() == Focus::Help {
        draw_help(frame, body, app);
        draw_footer(frame, footer, app);
        return;
    }

    // Sidebar takes 34 cols from body when visible (UX map §5.1).
    let (main_body, sidebar_area_opt) = if app.show_shortcuts() {
        if body.width > 34 {
            let [main, sidebar] =
                Layout::horizontal([Constraint::Min(0), Constraint::Length(34)]).areas(body);
            (main, Some(sidebar))
        } else {
            (body, None)
        }
    } else {
        (body, None)
    };

    // Search input bar is an extra line at top of main_body when in Search.
    let (list_detail_area, search_area_opt) = if app.is_searching() {
        if main_body.height > 2 {
            let [search, rest] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(main_body);
            (rest, Some(search))
        } else {
            (main_body, None)
        }
    } else {
        (main_body, None)
    };

    if let Some(search_area) = search_area_opt {
        draw_search_bar(frame, search_area, app);
    }

    match app.focus() {
        Focus::Detail if !app.is_split() => draw_detail(frame, list_detail_area, app),
        Focus::Search => {
            // While searching, show filtered list in main area (detail not shown in single-column search).
            if app.is_split() {
                let [list, detail] =
                    Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .areas(list_detail_area);
                draw_list(frame, list, app);
                draw_detail(frame, detail, app);
            } else {
                draw_list(frame, list_detail_area, app);
            }
        }
        _ => {
            if app.is_split() {
                let [list, detail] =
                    Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .areas(list_detail_area);
                draw_list(frame, list, app);
                draw_detail(frame, detail, app);
            } else {
                draw_list(frame, list_detail_area, app);
            }
        }
    }

    if let Some(sidebar_area) = sidebar_area_opt {
        draw_shortcuts_sidebar(frame, sidebar_area, app);
    }

    if app.focus() == Focus::QuitConfirm {
        draw_quit_confirm(frame, area);
    }

    draw_footer(frame, footer, app);
}

fn draw_list(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let mut header_spans = vec![Span::styled(
        "  TYPE PRI STATUS  ID             TITLE",
        theme::primary(),
    )];
    if let Some(q) = app.active_filter_query() {
        if !q.is_empty() {
            header_spans.push(Span::styled(format!("  [filter: {q}]"), theme::dim()));
        }
    } else if app.show_shortcuts() {
        header_spans.push(Span::styled("  [; shortcuts]", theme::dim()));
    }
    let header = Line::from(header_spans);
    let visible = app.visible_indices();
    let rows = std::iter::once(header)
        .chain(visible.iter().enumerate().map(|(visible_idx, &issue_idx)| {
            let issue = &app.issues()[issue_idx];
            let is_selected = visible_idx == app.selected();
            let selector = if is_selected { "> " } else { "  " };
            let style = if is_selected {
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

    // If filter yields no matches, show sentinel line.
    let rows = if visible.is_empty() && app.active_filter_query().is_some() {
        let mut with_empty = rows;
        with_empty.push(Line::from(Span::styled("  (no matches)", theme::dim())));
        with_empty
    } else {
        rows
    };

    frame.render_widget(Paragraph::new(rows), area);
}

fn draw_detail(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let Some(issue) = app.selected_issue() else {
        // Show placeholder when no selection / no filtered matches.
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("(no selection)", theme::dim())))
                .block(Block::default().borders(Borders::LEFT)),
            area,
        );
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

fn draw_search_bar(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let q = app.active_filter_query().unwrap_or("");
    let line = Line::from(vec![
        Span::styled(" / ", theme::primary()),
        Span::styled(q.to_string(), Style::new().fg(Color::White)),
        Span::styled("  (enter keep • esc cancel)", theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_shortcuts_sidebar(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let bindings = keys::all_bindings_for_focus(match app.focus() {
        Focus::Detail => "detail",
        Focus::Help => "help",
        Focus::List | Focus::Search | Focus::QuitConfirm => "list",
    });
    let scroll = app.shortcuts_scroll();
    let visible_rows = area.height.saturating_sub(2) as usize; // minus border
    let total = bindings.len();
    let start = scroll.min(total.saturating_sub(visible_rows));
    let end = (start + visible_rows).min(total);
    let mut lines = vec![Line::from(Span::styled(
        " Shortcuts  (;/F2 hide • ctrl+j/k scroll) ",
        theme::primary(),
    ))];
    for b in &bindings[start..end] {
        lines.push(Line::from(vec![
            Span::styled(format!(" {:>8} ", b.key), Style::new().fg(Color::Yellow)),
            Span::styled(b.desc.to_string(), theme::row()),
            Span::styled(format!(" [{}]", b.category), theme::dim()),
        ]));
    }
    if total > visible_rows {
        lines.push(Line::from(Span::styled(
            format!("  ... {}/{} ", end, total),
            theme::dim(),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::primary()),
        ),
        area,
    );
}

fn draw_help(frame: &mut Frame, area: ratatui::layout::Rect, _app: &TuiApp) {
    // Multi-panel help per UX map §5.2, simplified to two-column docs.
    let popup = centered_rect(
        area,
        area.width.saturating_sub(4).min(80),
        area.height.saturating_sub(2).min(20),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme::primary())
            .title(" Help  (?/F1 any key to close) "),
        popup,
    );
    let inner = ratatui::layout::Rect {
        x: popup.x + 1,
        y: popup.y + 1,
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };
    let help_lines = vec![
        Line::from(vec![
            Span::styled(" Navigation ", theme::primary()),
            Span::styled(
                " j/k move  G end  gg start  ctrl+d/u page  enter detail  esc back  q quit",
                theme::dim(),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Views ", theme::primary()),
            Span::styled(
                " b board  g graph  i insights  a actionable  E tree  [/] labels  ; sidebar  ? help",
                theme::dim(),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Filters ", theme::primary()),
            Span::styled(
                " / search  o open  c closed  r ready  l labels  esc clear filter",
                theme::dim(),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Actions ", theme::primary()),
            Span::styled(
                " x export  y copy id  C copy full  O editor  ctrl+r refresh",
                theme::dim(),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " Any key closes this overlay (focus restores). ",
            theme::dim(),
        )),
        Line::from(Span::styled(
            " Shortcuts sidebar: ;/F2 toggle, ctrl+j/k scroll when visible. ",
            theme::dim(),
        )),
        Line::from(Span::styled(
            " Search: / start, type to filter, enter keep, esc cancel, backspace delete. ",
            theme::dim(),
        )),
    ];
    frame.render_widget(Paragraph::new(help_lines), inner);
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

    // Context hints vary by focus (UX map §3.9, simplified).
    let hint = match app.focus() {
        Focus::List => {
            if app.active_filter_query().is_some() {
                "esc clear filter • / search • j/k move • enter detail • q quit"
            } else if app.show_shortcuts() {
                "tab focus • ; hide shortcuts • ctrl+j/k scroll • ? help"
            } else {
                "j/k move • / search • ; shortcuts • ? help • enter detail • q quit"
            }
        }
        Focus::Search => "enter keep filter • esc cancel • backspace del • j/k nav",
        Focus::Detail => "q/esc back • tab pane • ? help • ; shortcuts",
        Focus::Help => "any key to close help",
        Focus::QuitConfirm => "y quit • other cancel",
    };
    let filtered_note = if let Some(q) = app.active_filter_query() {
        if !q.is_empty() {
            format!(" 🔍{} ", q)
        } else {
            String::new()
        }
    } else {
        String::new()
    };
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
    let count_label = if app.active_filter_query().is_some() {
        format!("{}/{} issues", app.visible_count(), app.issues().len())
    } else {
        format!("{} issues", app.issues().len())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(hint.to_string(), theme::dim()),
            Span::styled(filtered_note, Style::new().fg(Color::Yellow)),
            Span::styled(" │ ", theme::dim()),
            Span::styled(format!("○{open} "), theme::status_open()),
            Span::styled(format!("●{closed} "), theme::status_closed()),
            Span::styled(count_label, theme::dim()),
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

fn centered_rect(area: ratatui::layout::Rect, width: u16, height: u16) -> ratatui::layout::Rect {
    let popup_y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_x = area.x + (area.width.saturating_sub(width)) / 2;
    ratatui::layout::Rect {
        x: popup_x,
        y: popup_y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}
