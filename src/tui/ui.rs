//! Frame rendering for the TUI skeleton + extras (bead gu7ts.6 + gu7ts.8 + gu7ts.7). governed-by: ADR-0003.
//!
#![allow(
    clippy::too_many_lines,
    clippy::needless_range_loop,
    clippy::match_same_arms
)]
//! Screen = body (height-1) + footer (1 line), per bv UX map §1.2. The body
//! is a split list+detail above width 100 and single-column below; the detail
//! layer replaces the whole body in non-split mode. gu7ts.8 adds: search bar,
//! shortcuts sidebar (34 cols, ;/F2), help overlay (?/F1). gu7ts.7 adds view
//! overlays (board/graph/actionable/insights/tree/label) and pagination.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::analysis::engine::AnalysisResult;
use crate::analysis::triage::Recommendation;
use crate::model::Issue;
use rich_rust::cells::get_character_cell_size;

use super::app::{Focus, PromptKind, TuiApp};
use super::{keys, theme};

/// Render one frame.
pub fn draw(frame: &mut Frame, app: &TuiApp) {
    let area = frame.area();
    let [body, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

    if app.focus() == Focus::Help {
        draw_help(frame, body, app);
        draw_footer(frame, footer, app);
        return;
    }

    if app.is_view_focus() {
        draw_view(frame, body, footer, app);
        return;
    }

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
    if app.focus() == Focus::Prompt {
        draw_prompt(frame, area, app);
    }

    draw_footer(frame, footer, app);
}

fn view_title(focus: Focus) -> (&'static str, &'static str) {
    match focus {
        Focus::Board => (" Board — Kanban ", "b close • j/k cards • enter detail"),
        Focus::Graph => (
            " Graph — Dependencies ",
            "g close • j/k nodes • enter detail",
        ),
        Focus::Actionable => (" Actionable — Ready Plan ", "a close • j/k • enter detail"),
        Focus::Insights => (" Insights — Metrics ", "i close"),
        Focus::Tree => (" Tree — Hierarchy ", "E close • j/k • enter detail"),
        Focus::LabelDashboard => (" Labels — Dashboard ", "[ close • j/k • enter filter"),
        _ => ("", ""),
    }
}

fn draw_view(
    frame: &mut Frame,
    body: ratatui::layout::Rect,
    footer: ratatui::layout::Rect,
    app: &TuiApp,
) {
    let (title, hint) = view_title(app.focus());
    let [view_area, hint_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(body);
    match app.focus() {
        Focus::Board => draw_board(frame, view_area, app),
        Focus::Graph => draw_graph_view(frame, view_area, app),
        Focus::Actionable => draw_actionable(frame, view_area, app),
        Focus::Insights => draw_insights(frame, view_area, app),
        Focus::Tree => draw_tree(frame, view_area, app),
        Focus::LabelDashboard => draw_label_dashboard(frame, view_area, app),
        _ => {}
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(hint.to_string(), theme::dim()),
            Span::styled(format!(" │ {}", title), theme::primary()),
        ])),
        hint_area,
    );
    draw_footer(frame, footer, app);
    if app.focus() == Focus::QuitConfirm {
        draw_quit_confirm(frame, frame.area());
    }
}

fn draw_board(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    if area.width < 20 || area.height < 5 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("(board: too small)", theme::dim())))
                .block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }
    let columns = app.board_columns();
    if columns.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("  (no cards)", theme::dim())))
                .block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }
    let column_count = u16::try_from(columns.len()).unwrap_or(1).max(1);
    let column_width = area.width / column_count;
    if area.width <= 80 || column_width < 16 {
        draw_board_compact(frame, area, app);
        return;
    }
    let constraints = vec![Constraint::Length(column_width); columns.len()];
    let col_areas = Layout::horizontal(constraints).split(area);
    let selection = app.board_selection();
    let mut flat = 0usize;
    for (index, column) in columns.iter().enumerate() {
        let Some(&column_area) = col_areas.get(index) else {
            break;
        };
        let column_start = flat;
        let column_end = flat + column.rows.len();
        let selected_in_column = selection >= column_start && selection < column_end;
        let content_height = usize::from(column_area.height.saturating_sub(3)).max(1);
        let row_in_column = selection.saturating_sub(column_start);
        let start = if selected_in_column {
            row_in_column.saturating_sub(content_height / 2)
        } else {
            0
        };
        let mut lines = vec![Line::from(Span::styled(
            format!(" {} ({}) ", column.status, column.rows.len()),
            theme::status_warning().add_modifier(Modifier::BOLD),
        ))];
        for (offset, &issue_index) in column
            .rows
            .iter()
            .skip(start)
            .take(content_height)
            .enumerate()
        {
            let is_selected = column_start + start + offset == selection;
            let issue = &app.issues()[issue_index];
            let style = if is_selected {
                theme::selected_row()
            } else {
                theme::row()
            };
            let marker = if is_selected { "› " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), style),
                Span::styled(clean(format!("{} ", issue.id)), style),
                Span::styled(truncate(&issue.title, 20), style),
            ]));
        }
        let border = if selected_in_column {
            theme::border_focused()
        } else {
            theme::border_unfocused()
        };
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).border_style(border)),
            column_area,
        );
        flat = column_end;
    }
}

fn draw_board_compact(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let rows = app.board_rows();
    let selection = app.board_selection().min(rows.len().saturating_sub(1));
    let mut lines = vec![Line::from(vec![
        Span::styled(" Board ", theme::primary()),
        Span::styled(format!("({} cards)", rows.len()), theme::dim()),
    ])];
    let window = viewport_window(
        selection,
        rows.len(),
        usize::from(area.height.saturating_sub(2)),
    );
    for position in window.0..window.1 {
        let issue = &app.issues()[rows[position]];
        let is_selected = position == selection;
        let style = if is_selected {
            theme::selected_row()
        } else {
            theme::row()
        };
        lines.push(Line::from(vec![
            Span::styled(if is_selected { "› " } else { "  " }.to_string(), style),
            Span::styled(format!("[{}] ", issue.status.as_str()), status_style(issue)),
            Span::styled(clean(format!("{:<10} ", issue.id)), style),
            Span::styled(truncate(&issue.title, 22), style),
        ]));
    }
    if rows.is_empty() {
        lines.push(Line::from(Span::styled("  (no cards)", theme::dim())));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_graph_view(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let visible = app.visible_indices();
    let [list, detail] = if area.width > 80 {
        Layout::horizontal([Constraint::Length(32), Constraint::Min(0)]).areas(area)
    } else {
        Layout::horizontal([Constraint::Min(0), Constraint::Length(0)]).areas(area)
    };
    let mut lines = vec![Line::from(Span::styled(" Nodes ", theme::primary()))];
    if visible.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no nodes — filter or empty)",
            theme::dim(),
        )));
    }
    let window = viewport_window(app.selected(), visible.len(), list.height as usize);
    for vis_idx in window.0..window.1 {
        let issue_idx = visible[vis_idx];
        let issue = &app.issues()[issue_idx];
        let sel = vis_idx == app.selected();
        let style = if sel {
            theme::selected_row()
        } else {
            theme::row()
        };
        let deps = issue.dependencies.len();
        lines.push(Line::from(vec![
            Span::styled(if sel { "› " } else { "  " }.to_string(), style),
            Span::styled(clean(format!("{:<14} ", issue.id)), style),
            Span::styled(
                if deps > 0 {
                    format!("→{deps}")
                } else {
                    "  ".to_string()
                },
                theme::dim(),
            ),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        list,
    );
    if detail.width > 0
        && let Some(issue) = app.selected_issue()
    {
        let mut r_lines = vec![Line::from(Span::styled(
            clean(format!(" {} ", issue.id)),
            theme::primary(),
        ))];
        if issue.dependencies.is_empty() {
            r_lines.push(Line::from(Span::styled(
                "  (no dependencies)",
                theme::dim(),
            )));
        } else {
            for dep in &issue.dependencies {
                r_lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", dep.dep_type.as_str()), theme::dim()),
                    Span::styled(clean(&dep.depends_on_id), theme::primary_fg()),
                ]));
            }
        }
        r_lines.push(Line::from(""));
        r_lines.push(Line::from(Span::styled(
            format!(
                "  status {}  prio P{}",
                issue.status.as_str(),
                issue.priority.0
            ),
            theme::dim(),
        )));
        frame.render_widget(
            Paragraph::new(r_lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Edges ")
                    .border_style(theme::border_unfocused()),
            ),
            detail,
        );
    }
}

fn draw_actionable(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let rows = app.actionable_rows();
    let selection = app.actionable_selection().min(rows.len().saturating_sub(1));
    let mut lines = vec![Line::from(vec![
        Span::styled(" Actionable ", theme::primary()),
        Span::styled(format!("({} ready)", rows.len()), theme::dim()),
    ])];
    let window = viewport_window(
        selection,
        rows.len(),
        usize::from(area.height.saturating_sub(2)),
    );
    for position in window.0..window.1 {
        let issue = &app.issues()[rows[position]];
        let is_selected = position == selection;
        let style = if is_selected {
            theme::selected_row()
        } else {
            theme::row()
        };
        lines.push(Line::from(vec![
            Span::styled(if is_selected { "› " } else { "  " }.to_string(), style),
            Span::styled(format!("P{} ", issue.priority.0), style),
            Span::styled(clean(format!("{:<14} ", issue.id)), style),
            Span::styled(clean(&issue.title), style),
        ]));
    }
    if rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no open actionable — triage via br triage)",
            theme::dim(),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_insights(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let total = app.issues().len();
    let open = app
        .issues()
        .iter()
        .filter(|i| i.status.as_str() == "open")
        .count();
    let closed = app
        .issues()
        .iter()
        .filter(|i| i.status.as_str() == "closed")
        .count();
    let blocked = app
        .issues()
        .iter()
        .filter(|i| i.status.as_str() == "blocked")
        .count();
    let with_deps = app
        .issues()
        .iter()
        .filter(|i| !i.dependencies.is_empty())
        .count();
    let mut lines = vec![
        Line::from(Span::styled(" Insights ", theme::primary())),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  total {total}  "), theme::row()),
            Span::styled(format!("open {open}  "), theme::status_open()),
            Span::styled(format!("blocked {blocked}  "), theme::status_blocked()),
            Span::styled(format!("closed {closed}"), theme::status_closed()),
        ]),
        Line::from(vec![
            Span::styled(format!("  with deps {with_deps}  "), theme::dim()),
            Span::styled(
                format!("isolated {}", total.saturating_sub(with_deps)),
                theme::dim(),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Panels: Bottlenecks • Keystones • Cycle (bv --robot-insights for full metrics)",
            theme::dim(),
        )),
    ];
    if let Some(q) = app.active_filter_query()
        && !q.is_empty()
    {
        lines.push(Line::from(Span::styled(
            clean(format!("  filter 🔍 {q}")),
            theme::status_warning(),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_tree(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let rows = app.tree_rows();
    let selection = app.tree_selection().min(rows.len().saturating_sub(1));
    let mut lines = vec![Line::from(Span::styled(
        " Tree — parent-child ",
        theme::primary(),
    ))];
    let window = viewport_window(
        selection,
        rows.len(),
        usize::from(area.height.saturating_sub(2)),
    );
    for (position, row) in rows
        .iter()
        .enumerate()
        .skip(window.0)
        .take(window.1.saturating_sub(window.0))
    {
        let issue = &app.issues()[row.issue_index];
        let is_selected = position == selection;
        let style = if is_selected {
            theme::selected_row()
        } else {
            theme::row()
        };
        let indent = "  ".repeat(row.depth);
        lines.push(Line::from(vec![
            Span::styled(if is_selected { "› " } else { "  " }.to_string(), style),
            Span::styled(clean(format!("{indent}{} ", issue.id)), style),
            Span::styled(truncate(&issue.title, 30), style),
        ]));
    }
    if rows.is_empty() {
        lines.push(Line::from(Span::styled("  (no issues)", theme::dim())));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_label_dashboard(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let rows = app.label_rows();
    let selection = app.label_selection().min(rows.len().saturating_sub(1));
    let mut lines = vec![Line::from(vec![
        Span::styled(" Labels ", theme::primary()),
        Span::styled(format!("({} labels)", rows.len()), theme::dim()),
    ])];
    let window = viewport_window(
        selection,
        rows.len(),
        usize::from(area.height.saturating_sub(2)),
    );
    for (position, row) in rows
        .iter()
        .enumerate()
        .skip(window.0)
        .take(window.1.saturating_sub(window.0))
    {
        let is_selected = position == selection;
        let style = if is_selected {
            theme::selected_row()
        } else {
            theme::row()
        };
        let bar = "█".repeat(row.count.min(12));
        lines.push(Line::from(vec![
            Span::styled(if is_selected { "› " } else { "  " }.to_string(), style),
            Span::styled(clean(format!("{:<18} ", row.label)), style),
            Span::styled(format!("{:>3} ", row.count), theme::dim()),
            Span::styled(bar, theme::primary_fg()),
        ]));
    }
    if rows.is_empty() {
        lines.push(Line::from(Span::styled("  (no labels)", theme::dim())));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn markdown_lines(md: &str, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let sanitized = crate::format::sanitize_terminal_text(md);
    let markdown = rich_rust::renderables::Markdown::new(sanitized.as_ref()).hyperlinks(false);
    let rendered = markdown.render(width);
    let mut lines = Vec::new();
    for segments in rich_rust::segment::split_lines(rendered.into_iter()) {
        let spans: Vec<Span<'static>> = segments
            .into_iter()
            .filter(|segment| !segment.is_control() && !segment.text.is_empty())
            .map(|segment| {
                Span::styled(
                    clean(segment.text.as_ref()),
                    rich_text_style(segment.style.as_ref()),
                )
            })
            .collect();
        lines.extend(wrap_line(Line::from(spans), width));
    }
    while lines
        .last()
        .is_some_and(|line| line.spans.iter().all(|span| span.content.trim().is_empty()))
    {
        lines.pop();
    }
    lines
}

fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 || line.spans.is_empty() {
        return vec![line];
    }
    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut chunk = String::new();
    let mut chunk_style = Style::new();
    let mut used = 0usize;
    for span in line.spans {
        let style = span.style;
        for ch in span.content.chars() {
            let char_width = get_character_cell_size(ch);
            if used > 0 && used + char_width > width {
                if !chunk.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut chunk), chunk_style));
                }
                lines.push(Line::from(std::mem::take(&mut spans)));
                used = 0;
            }
            if chunk.is_empty() {
                chunk_style = style;
            }
            chunk.push(ch);
            used += char_width;
        }
        if !chunk.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut chunk), chunk_style));
        }
    }
    if !spans.is_empty() || lines.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}

fn rich_color(color: &rich_rust::Color) -> Color {
    match color.color_type {
        rich_rust::ColorType::Default => Color::Reset,
        rich_rust::ColorType::Standard | rich_rust::ColorType::Windows => {
            match color.number.unwrap_or(0) {
                0 => Color::Black,
                1 => Color::Red,
                2 => Color::Green,
                3 => Color::Yellow,
                4 => Color::Blue,
                5 => Color::Magenta,
                6 => Color::Cyan,
                7 => Color::Gray,
                8 => Color::DarkGray,
                9 => Color::LightRed,
                10 => Color::LightGreen,
                11 => Color::LightYellow,
                12 => Color::LightBlue,
                13 => Color::LightMagenta,
                14 => Color::LightCyan,
                _ => Color::White,
            }
        }
        rich_rust::ColorType::EightBit => Color::Indexed(color.number.unwrap_or(0)),
        rich_rust::ColorType::TrueColor => {
            let triplet = color.triplet.unwrap_or_default();
            Color::Rgb(triplet.red, triplet.green, triplet.blue)
        }
    }
}

fn rich_text_style(style: Option<&rich_rust::Style>) -> Style {
    let Some(style) = style else {
        return Style::new();
    };
    let mut output = Style::new();
    if let Some(color) = &style.color {
        output = output.fg(rich_color(color));
    }
    if let Some(color) = &style.bgcolor {
        output = output.bg(rich_color(color));
    }
    let attributes = style.attributes;
    if attributes.contains(rich_rust::Attributes::BOLD) {
        output = output.add_modifier(Modifier::BOLD);
    }
    if attributes.contains(rich_rust::Attributes::DIM) {
        output = output.add_modifier(Modifier::DIM);
    }
    if attributes.contains(rich_rust::Attributes::ITALIC) {
        output = output.add_modifier(Modifier::ITALIC);
    }
    if attributes.contains(rich_rust::Attributes::UNDERLINE)
        || attributes.contains(rich_rust::Attributes::UNDERLINE2)
    {
        output = output.add_modifier(Modifier::UNDERLINED);
    }
    if attributes.contains(rich_rust::Attributes::STRIKE) {
        output = output.add_modifier(Modifier::CROSSED_OUT);
    }
    if attributes.contains(rich_rust::Attributes::REVERSE) {
        output = output.add_modifier(Modifier::REVERSED);
    }
    output
}

fn clean(value: impl AsRef<str>) -> String {
    crate::format::sanitize_terminal_inline(value.as_ref()).into_owned()
}

fn truncate(s: &str, max: usize) -> String {
    let s = clean(s);
    let count = s.chars().count();
    if count <= max {
        s
    } else if max == 0 {
        String::new()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn viewport_window(selected: usize, total: usize, viewport: usize) -> (usize, usize) {
    if total <= viewport || viewport == 0 {
        return (0, total);
    }
    let half = viewport / 2;
    let mut start = selected.saturating_sub(half);
    if start + viewport > total {
        start = total - viewport;
    }
    (start, start + viewport)
}

fn draw_list(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let header_text = if area.width <= 40 {
        "  ID       TITLE"
    } else if area.width <= 60 {
        "  STATUS ID       TITLE"
    } else if area.width <= 100 {
        "  PRI STATUS ID       TITLE"
    } else {
        "  TYPE PRI STATUS  ID             TITLE"
    };
    let mut header_spans = vec![Span::styled(header_text, theme::primary())];
    if let Some(q) = app.active_filter_query() {
        if !q.is_empty() {
            header_spans.push(Span::styled(
                clean(format!("  [filter: {q}]")),
                theme::dim(),
            ));
        }
    } else if app.show_shortcuts() {
        header_spans.push(Span::styled("  [; shortcuts]", theme::dim()));
    }
    let header = Line::from(header_spans);
    let visible = app.visible_indices();
    let total = visible.len();
    let viewport = usize::from(area.height.saturating_sub(1));
    let (start, end) = viewport_window(app.selected(), total, viewport.max(1));
    let page = if viewport == 0 {
        1
    } else {
        start / viewport.max(1) + 1
    };
    let total_pages = if total == 0 || viewport == 0 {
        1
    } else {
        total.div_ceil(viewport)
    };
    let page_line = Line::from(Span::styled(
        format!(
            " Page {page}/{total_pages} ({}-{} of {total}) ",
            start + 1,
            end.min(total)
        ),
        theme::dim(),
    ));
    let mut rows = Vec::with_capacity(viewport + 2);
    rows.push(header);
    for vis_idx in start..end {
        let issue_idx = visible[vis_idx];
        let issue = &app.issues()[issue_idx];
        let is_selected = vis_idx == app.selected();
        let style = if is_selected {
            theme::selected_row()
        } else {
            theme::row()
        };
        let selector = if is_selected { "› " } else { "  " };
        let label_tag = if !issue.labels.is_empty() && area.width > 80 {
            clean(format!(" {}", issue.labels.join(",")))
        } else {
            String::new()
        };
        let row_spans = if area.width <= 40 {
            vec![
                Span::styled(selector.to_string(), style),
                Span::styled(clean(format!("{:<10} ", issue.id)), style),
                Span::styled(
                    truncate(&issue.title, usize::from(area.width.saturating_sub(14))),
                    style,
                ),
            ]
        } else if area.width <= 60 {
            vec![
                Span::styled(selector.to_string(), style),
                Span::styled(
                    format!("{:<7} ", issue.status.as_str()),
                    status_style(issue),
                ),
                Span::styled(clean(format!("{:<10} ", issue.id)), style),
                Span::styled(truncate(&issue.title, 18), style),
            ]
        } else {
            vec![
                Span::styled(selector.to_string(), style),
                Span::styled(format!("{:<4}", issue.issue_type.as_str()), style),
                Span::styled(format!("P{:<2} ", issue.priority.0), style),
                Span::styled(
                    format!("{:<7} ", issue.status.as_str()),
                    status_style(issue),
                ),
                Span::styled(clean(format!("{:<14} ", issue.id)), style),
                Span::styled(truncate(&issue.title, 28), style),
                Span::styled(label_tag, theme::dim()),
            ]
        };
        rows.push(Line::from(row_spans));
    }
    if total == 0 {
        let empty_msg = if app.active_filter_query().is_some() {
            "  (no matches)"
        } else {
            "  (no issues)"
        };
        rows.push(Line::from(Span::styled(
            empty_msg.to_string(),
            theme::dim(),
        )));
    }
    if rows.len() < usize::from(area.height) {
        rows.push(Line::from(""));
        rows.push(page_line);
    }
    frame.render_widget(Paragraph::new(rows), area);
}

fn detail_lines(
    issue: &Issue,
    analysis: Option<&AnalysisResult>,
    triage: Option<&Recommendation>,
    width: u16,
) -> Vec<Line<'static>> {
    let status_style_val = status_style(issue);
    let type_icon = match issue.issue_type.as_str() {
        "bug" => "🐛",
        "feature" => "✨",
        "task" => "📋",
        "epic" => "🚀",
        "chore" => "🧹",
        _ => "•",
    };
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(
                format!("{type_icon} {} ", issue.issue_type.as_str()),
                theme::primary(),
            ),
            Span::styled(
                clean(&issue.title),
                theme::row().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            clean(format!("{} │ {}", issue.id, issue.status.as_str())),
            status_style_val,
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Status:   ", theme::dim()),
            Span::styled(issue.status.as_str().to_string(), status_style_val),
            Span::styled(format!("  Priority: P{}  ", issue.priority.0), theme::dim()),
            Span::styled(
                format!("Blast: {}", format!("{:?}", issue.blast).to_lowercase()),
                theme::dim(),
            ),
            Span::styled(
                format!("  Type: {}", issue.issue_type.as_str()),
                theme::dim(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Assignee: ", theme::dim()),
            Span::styled(
                clean(issue.assignee.as_deref().unwrap_or("-")),
                theme::primary_fg(),
            ),
            Span::styled("  Pin: ", theme::dim()),
            Span::styled(
                clean(issue.pin.as_deref().unwrap_or("-")),
                theme::primary_fg(),
            ),
            Span::styled("  Wave: ", theme::dim()),
            Span::styled(
                issue
                    .wave
                    .map(|w| w.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                theme::dim(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Created: ", theme::dim()),
            Span::styled(
                issue.created_at.format("%Y-%m-%d %H:%M").to_string(),
                theme::dim(),
            ),
            Span::styled("  Updated: ", theme::dim()),
            Span::styled(
                issue.updated_at.format("%Y-%m-%d %H:%M").to_string(),
                theme::dim(),
            ),
        ]),
    ];
    if !issue.labels.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Labels: ", theme::dim()),
            Span::styled(clean(issue.labels.join(" • ")), theme::status_warning()),
        ]));
    }
    if let Some(verify) = &issue.verify {
        lines.push(Line::from(vec![
            Span::styled("VERIFY: ", theme::dim()),
            Span::styled(clean(verify), theme::status_open()),
        ]));
    }
    if !issue.principles.is_empty() {
        lines.push(Line::from(Span::styled("Principles:", theme::dim())));
        for p in &issue.principles {
            lines.push(Line::from(vec![
                Span::styled(clean(format!("  • {} — ", p.name)), theme::accent_lav()),
                Span::styled(clean(&p.decision), theme::row()),
            ]));
        }
    }
    if let Some(ac) = issue
        .acceptance_criteria
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        lines.push(Line::from(Span::styled("Acceptance:", theme::dim())));
        lines.push(Line::from(Span::styled(
            clean(format!("  {ac}")),
            theme::row(),
        )));
    }
    if !issue.dependencies.is_empty() {
        lines.push(Line::from(Span::styled("Depends on:", theme::dim())));
        for dep in issue.dependencies.iter().take(8) {
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", dep.dep_type.as_str()), theme::dim()),
                Span::styled(clean(&dep.depends_on_id), theme::primary_fg()),
            ]));
        }
        if issue.dependencies.len() > 8 {
            lines.push(Line::from(Span::styled(
                format!("  … and {} more", issue.dependencies.len() - 8),
                theme::dim(),
            )));
        }
    }
    if !issue.comments.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Comments", Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(" ({})  j/k scroll to see more", issue.comments.len()),
                theme::dim(),
            ),
        ]));
        for c in issue.comments.iter().take(5) {
            lines.push(Line::from(vec![
                Span::styled(clean(format!("  {}: ", c.author)), theme::status_warning()),
                Span::styled(c.created_at.format("%m/%d").to_string(), theme::dim()),
            ]));
            for l in markdown_lines(&c.body, usize::from(width.saturating_sub(6))) {
                let indented: Vec<Span> =
                    std::iter::once(Span::styled("    ".to_string(), theme::dim()))
                        .chain(l.spans)
                        .collect();
                lines.push(Line::from(indented));
            }
        }
        if issue.comments.len() > 5 {
            lines.push(Line::from(Span::styled(
                format!("  … {} more", issue.comments.len() - 5),
                theme::dim(),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled("Comments: (none)", theme::dim())));
    }

    if let Some(rec) = triage {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "🎯 Triage Insight:",
            theme::accent_lav().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::styled(
                format!("  score {:.3} ", rec.score),
                theme::status_warning(),
            ),
            Span::styled(clean(&rec.action), theme::row()),
        ]));
        for reason in rec.reasons.iter().take(3) {
            lines.push(Line::from(Span::styled(
                clean(format!("  • {reason}")),
                theme::dim(),
            )));
        }
        if rec.score > 0.5 {
            lines.push(Line::from(Span::styled(
                "  ⚡ Quick win candidate",
                theme::status_open(),
            )));
        }
    }
    if let Some(analysis) = analysis {
        let mut graph_bits: Vec<String> = Vec::new();
        if let Some(pr) = analysis.pagerank.as_ref().and_then(|m| m.get(&issue.id)) {
            graph_bits.push(format!("PR {:.3}", pr));
        }
        if let Some(bw) = analysis.betweenness.as_ref().and_then(|m| m.get(&issue.id)) {
            graph_bits.push(format!("BW {:.1}", bw));
        }
        if let Some(ev) = analysis.eigenvector.as_ref().and_then(|m| m.get(&issue.id)) {
            graph_bits.push(format!("EV {:.3}", ev));
        }
        if !graph_bits.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "🔎 Graph Analysis:",
                Style::new().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                format!(
                    "  {}  depth {:.0}",
                    graph_bits.join(" │ "),
                    analysis
                        .critical_path_score
                        .as_ref()
                        .and_then(|m| m.get(&issue.id))
                        .copied()
                        .unwrap_or(0.0)
                ),
                theme::dim(),
            )));
        }
        if analysis.has_cycles && analysis.cycle_count > 0 {
            lines.push(Line::from(Span::styled(
                format!("  ⚠ cycles: {} in workspace", analysis.cycle_count),
                theme::status_blocked(),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Description:",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    if let Some(description) = &issue.description {
        if crate::format::markdown::contains_markdown(description) {
            for l in markdown_lines(description, usize::from(width.saturating_sub(4))) {
                lines.push(l);
            }
        } else {
            for chunk in split_for_wrap(description, usize::from(width.saturating_sub(4))) {
                lines.push(Line::from(Span::styled(chunk, theme::row())));
            }
        }
    } else {
        lines.push(Line::from(Span::styled("  (no description)", theme::dim())));
    }
    if let Some(design) = issue.design.as_deref().filter(|s| !s.is_empty()) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Design:",
            Style::new().add_modifier(Modifier::BOLD),
        )));
        if crate::format::markdown::contains_markdown(design) {
            for l in markdown_lines(design, usize::from(width.saturating_sub(4))) {
                lines.push(l);
            }
        } else {
            for chunk in split_for_wrap(design, usize::from(width.saturating_sub(4))) {
                lines.push(Line::from(Span::styled(chunk, theme::row())));
            }
        }
    }

    lines
}

pub(crate) fn detail_line_count(app: &TuiApp, width: u16) -> usize {
    let Some(issue) = app.selected_issue() else {
        return 0;
    };
    detail_lines(issue, app.analysis(), app.triage_for(&issue.id), width).len()
}

fn draw_detail(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let Some(issue) = app.selected_issue() else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("(no selection)", theme::dim())))
                .block(Block::default().borders(Borders::LEFT).title(" Detail ")),
            area,
        );
        return;
    };

    let lines = detail_lines(issue, app.analysis(), app.triage_for(&issue.id), area.width);
    let viewport = usize::from(area.height.saturating_sub(2));
    let total = lines.len();
    let max_scroll = total.saturating_sub(viewport);
    let start = app.detail_scroll().min(max_scroll);
    let end = (start + viewport).min(total);
    let visible: Vec<Line> = lines.into_iter().skip(start).take(end - start).collect();
    let scroll_hint = if total > viewport {
        format!(
            " {} {}/{} ",
            if start > 0 { "↑" } else { " " },
            start + 1,
            total
        )
    } else {
        String::new()
    };
    frame.render_widget(
        Paragraph::new(visible)
            .block(
                Block::default()
                    .borders(Borders::LEFT)
                    .title(format!(" Detail{scroll_hint} ")),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn split_for_wrap(text: &str, width: usize) -> Vec<String> {
    let text = crate::format::sanitize_terminal_text(text);
    let text = text.as_ref();
    if width == 0 || text.chars().count() <= width {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if cur.chars().count() + word.chars().count() + 1 > width && !cur.is_empty() {
            out.push(cur);
            cur = String::new();
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        out.push(text.to_string());
    }
    out
}

fn draw_search_bar(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let q = app.active_filter_query().unwrap_or("");
    let line = Line::from(vec![
        Span::styled(" / ", theme::primary()),
        Span::styled(clean(q), theme::row()),
        Span::styled("  (enter keep • esc cancel)", theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_shortcuts_sidebar(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let bindings = keys::all_bindings_for_focus(match app.focus() {
        Focus::Detail => "detail",
        Focus::Help => "help",
        _ => "list",
    });
    let scroll = app.shortcuts_scroll();
    let visible_rows = area.height.saturating_sub(2) as usize;
    let total = bindings.len();
    let start = scroll.min(total.saturating_sub(visible_rows));
    let end = (start + visible_rows).min(total);
    let mut lines = vec![Line::from(Span::styled(
        " Shortcuts  (;/F2 hide • ctrl+j/k scroll) ",
        theme::primary(),
    ))];
    for b in &bindings[start..end] {
        lines.push(Line::from(vec![
            Span::styled(format!(" {:>8} ", b.key), theme::status_warning()),
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
                " j/k move  G end  home start  ctrl+d/u page  enter detail  esc back  q quit",
                theme::dim(),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Views ", theme::primary()),
            Span::styled(
                " b board  g graph  i insights  a actionable  E tree  [ labels  ; sidebar  ? help",
                theme::dim(),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Filters ", theme::primary()),
            Span::styled(" / search  esc clear filter", theme::dim()),
        ]),
        Line::from(vec![
            Span::styled(" Actions ", theme::primary()),
            Span::styled(
                " c close  d delete  e edit title  ctrl+r reload",
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

fn draw_prompt(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let Some(prompt) = app.prompt() else {
        return;
    };
    let field_rows = u16::try_from(prompt.fields().count()).unwrap_or(1);
    let height = (field_rows * 2 + 4).min(area.height);
    let popup = centered_rect(area, area.width.saturating_sub(8).min(72), height);
    frame.render_widget(Clear, popup);
    let border = if prompt.kind() == PromptKind::Delete {
        theme::danger_border()
    } else {
        theme::primary()
    };
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border)
            .title(clean(format!(
                " {} — {} ",
                prompt.title(),
                prompt.issue_id()
            ))),
        popup,
    );
    let inner = ratatui::layout::Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: popup.width.saturating_sub(4),
        height: popup.height.saturating_sub(2),
    };
    let mut lines = Vec::new();
    for (index, (label, value)) in prompt.fields().enumerate() {
        let active = index == prompt.active();
        lines.push(Line::from(Span::styled(
            label.to_string(),
            if active {
                theme::primary_fg()
            } else {
                theme::dim()
            },
        )));
        lines.push(Line::from(Span::styled(
            clean(format!("  {value}{}", if active { "▌" } else { "" })),
            if active {
                theme::selected_row()
            } else {
                theme::row()
            },
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " tab/↑↓ field • enter save • esc cancel",
        theme::dim(),
    )));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_footer(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    if let Some(message) = app.status_message() {
        let (marker, style) = if app.status_is_error() {
            ("!", theme::status_blocked())
        } else {
            ("✓", theme::status_open())
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                clean(format!("{marker} {message}")),
                style,
            ))),
            area,
        );
        return;
    }

    let hint = match app.focus() {
        Focus::List => {
            if app.active_filter_query().is_some() {
                "esc clear filter • / search • j/k move • enter detail • q quit"
            } else if app.show_shortcuts() {
                "tab focus • ; hide shortcuts • ctrl+j/k scroll • ? help"
            } else {
                "j/k move • / search • c/d/e act • b board • g graph • ; shortcuts • ? help • q quit"
            }
        }
        Focus::Search => "enter keep filter • esc cancel • backspace del • j/k nav",
        Focus::Detail => "j/k scroll • q/esc back • tab pane • c/d/e act • ? help • ; shortcuts",
        Focus::Board | Focus::Graph | Focus::Actionable | Focus::Tree => {
            "q/esc back • j/k nav • enter detail • ? help"
        }
        Focus::Insights => "q/esc back • ? help",
        Focus::LabelDashboard => "q/esc back • j/k nav • enter filter • ? help",
        Focus::Help => "any key to close help",
        Focus::QuitConfirm => "y quit • other cancel",
        Focus::Prompt => "tab/↑↓ field • enter save • esc cancel",
    };
    let filtered_note = if let Some(q) = app.active_filter_query() {
        if !q.is_empty() {
            clean(format!(" 🔍{} ", q))
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
            Span::styled(filtered_note, theme::status_warning()),
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

#[cfg(test)]
mod tests {
    use super::{markdown_lines, truncate};

    #[test]
    fn rendered_text_escapes_terminal_controls() {
        let title = truncate("safe\u{1b}[31mred", 40);
        assert!(!title.chars().any(char::is_control));
    }

    #[test]
    fn markdown_escapes_terminal_controls() {
        let lines = markdown_lines("hello \u{1b}[31mred", 80);
        let text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect();
        assert!(!text.chars().any(char::is_control));
    }

    #[test]
    fn markdown_wraps_to_width() {
        let md = "word ".repeat(40);
        let lines = markdown_lines(&md, 20);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| {
            line.spans
                .iter()
                .map(|span| rich_rust::cells::cell_len(span.content.as_ref()))
                .sum::<usize>()
                <= 20
        }));
    }
}
