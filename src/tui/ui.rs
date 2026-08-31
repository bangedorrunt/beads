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

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

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

    // View overlays replace main body with their own layout but keep sidebar+search chrome.
    if app.is_view_focus() {
        draw_view(frame, body, footer, app);
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

fn view_title(focus: Focus) -> (&'static str, &'static str) {
    match focus {
        Focus::Board => (
            " Board — Kanban ",
            "b close • h/l columns • j/k cards • enter detail",
        ),
        Focus::Graph => (
            " Graph — Dependencies ",
            "g close • j/k nodes • enter detail • h/l scroll",
        ),
        Focus::Actionable => (" Actionable — Ready Plan ", "a close • j/k • enter detail"),
        Focus::Insights => (
            " Insights — Metrics ",
            "i close • h/l panels • j/k items • enter jump",
        ),
        Focus::Tree => (
            " Tree — Hierarchy ",
            "E close • j/k • enter toggle • h/l collapse",
        ),
        Focus::LabelDashboard => (
            " Labels — Dashboard ",
            "[ close • j/k • enter filter • h detail",
        ),
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
    // Split body: view area (Min) + pagination hint line (1) when list-like views.
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
    // Hint line under view
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(hint.to_string(), theme::dim()),
            Span::styled(format!(" │ {}", title), theme::primary()),
        ])),
        hint_area,
    );
    // Footer still shows counts
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
    let visible = app.visible_indices();
    // Mobile stacked mode when narrow (iphone): single column
    if area.width <= 80 {
        let mut lines = vec![Line::from(vec![
            Span::styled(" Board ", theme::primary()),
            Span::styled(format!("({} cards)", visible.len()), theme::dim()),
        ])];
        let window = viewport_window(
            app.selected(),
            visible.len(),
            area.height.saturating_sub(2) as usize,
        );
        for vis_idx in window.0..window.1 {
            let issue_idx = visible[vis_idx];
            let issue = &app.issues()[issue_idx];
            let sel = vis_idx == app.selected();
            let style = if sel {
                theme::selected_row()
            } else {
                theme::row()
            };
            lines.push(Line::from(vec![
                Span::styled(if sel { "› " } else { "  " }.to_string(), style),
                Span::styled(format!("[{}] ", issue.status.as_str()), status_style(issue)),
                Span::styled(format!("{:<10} ", issue.id), style),
                Span::styled(truncate(&issue.title, 22), style),
            ]));
        }
        if visible.is_empty() {
            lines.push(Line::from(Span::styled("  (no cards)", theme::dim())));
        }
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }
    // Group by status
    let mut groups: std::collections::BTreeMap<&str, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (vis_idx, &issue_idx) in visible.iter().enumerate() {
        let s = app.issues()[issue_idx].status.as_str();
        groups.entry(s).or_default().push(vis_idx);
    }
    let order = ["open", "in_progress", "blocked", "closed"];
    let cols: Vec<String> = order
        .iter()
        .map(|s| {
            let count = groups.get(s).map(Vec::len).unwrap_or(0);
            format!("{s} ({count})")
        })
        .collect();
    let header = Line::from(vec![
        Span::styled(" Board ", theme::primary()),
        Span::styled(cols.join(" │ "), theme::dim()),
    ]);
    // Build columns side-by-side with simple vertical lists.
    let col_count = 4;
    let col_width = area.width / u16::try_from(col_count).unwrap_or(4);
    let constraints = vec![Constraint::Length(col_width); col_count];
    let col_areas = Layout::horizontal(constraints).split(area);
    for (ci, status) in order.iter().enumerate() {
        if ci >= col_areas.len() {
            break;
        }
        let vis_indices = groups.get(status).cloned().unwrap_or_default();
        let mut lines = vec![Line::from(Span::styled(
            format!(" {status} "),
            theme::status_warning().add_modifier(Modifier::BOLD),
        ))];
        for &vis_idx in vis_indices
            .iter()
            .take(col_areas[ci].height.saturating_sub(1) as usize)
        {
            let issue_idx = visible[vis_idx];
            let issue = &app.issues()[issue_idx];
            let sel = vis_idx == app.selected();
            let style = if sel {
                theme::selected_row()
            } else {
                theme::row()
            };
            let marker = if sel { "› " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), style),
                Span::styled(format!("{} ", issue.id), style),
                Span::styled(truncate(&issue.title, 20), style),
            ]));
        }
        let is_first = ci == 0;
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).border_style(
                if is_first {
                    theme::border_focused()
                } else {
                    theme::border_unfocused()
                },
            )),
            col_areas[ci],
        );
    }
    // Overlay header at top? Instead render header in first col area top is already, we also render global header at very top via hint.
    let _ = header;
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
            Span::styled(format!("{:<14} ", issue.id), style),
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
    // Right pane: ASCII edge list for selected
    if detail.width > 0
        && let Some(issue) = app.selected_issue()
    {
        let mut r_lines = vec![Line::from(Span::styled(
            format!(" {} ", issue.id),
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
                    Span::styled(dep.depends_on_id.clone(), theme::primary_fg()),
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
            Paragraph::new(r_lines).block(Block::default().borders(Borders::ALL).title(" Edges ").border_style(theme::border_unfocused())),
            detail,
        );
    }
}

fn draw_actionable(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let visible = app.visible_indices();
    // Actionable = open, unblocked, sorted by priority then id (simplified vs bv triage).
    let mut actionable: Vec<usize> = visible
        .iter()
        .copied()
        .filter(|&issue_idx| {
            let s = app.issues()[issue_idx].status.as_str();
            s == "open"
        })
        .collect();
    actionable.sort_by(|&a, &b| {
        let pa = app.issues()[a].priority.0;
        let pb = app.issues()[b].priority.0;
        pa.cmp(&pb)
            .then_with(|| app.issues()[a].id.cmp(&app.issues()[b].id))
    });
    let mut lines = vec![Line::from(vec![
        Span::styled(" Actionable ", theme::primary()),
        Span::styled(format!("({} ready)", actionable.len()), theme::dim()),
    ])];
    let window = viewport_window(
        app.selected().min(actionable.len().saturating_sub(1)),
        actionable.len(),
        area.height.saturating_sub(2) as usize,
    );
    for idx in window.0..window.1 {
        let issue_idx = actionable[idx];
        let issue = &app.issues()[issue_idx];
        let sel = idx == app.selected().min(actionable.len().saturating_sub(1));
        let style = if sel {
            theme::selected_row()
        } else {
            theme::row()
        };
        lines.push(Line::from(vec![
            Span::styled(if sel { "› " } else { "  " }.to_string(), style),
            Span::styled(format!("P{} ", issue.priority.0), style),
            Span::styled(format!("{:<14} ", issue.id), style),
            Span::styled(issue.title.clone(), style),
        ]));
    }
    if actionable.is_empty() {
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
            format!("  filter 🔍 {q}"),
            theme::status_warning(),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_tree(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    // Simple parent-child tree via parent-child deps.
    let visible = app.visible_indices();
    let mut parent_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut roots: Vec<String> = Vec::new();
    let id_set: std::collections::HashSet<String> = visible
        .iter()
        .map(|&idx| app.issues()[idx].id.clone())
        .collect();
    for &issue_idx in &visible {
        let issue = &app.issues()[issue_idx];
        let has_parent = issue
            .dependencies
            .iter()
            .any(|d| d.dep_type.as_str() == "parent-child" && id_set.contains(&d.depends_on_id));
        if !has_parent {
            roots.push(issue.id.clone());
        }
        for dep in &issue.dependencies {
            if dep.dep_type.as_str() == "parent-child" {
                parent_map
                    .entry(dep.depends_on_id.clone())
                    .or_default()
                    .push(issue.id.clone());
            }
        }
    }
    let mut lines = vec![Line::from(Span::styled(
        " Tree — parent-child ",
        theme::primary(),
    ))];
    let window = viewport_window(
        app.selected(),
        roots.len().max(1),
        area.height.saturating_sub(2) as usize,
    );
    // Render roots with one level of children inline.
    let mut flat: Vec<(String, usize)> = Vec::new();
    for rid in &roots {
        flat.push((rid.clone(), 0));
        if let Some(children) = parent_map.get(rid) {
            for cid in children {
                flat.push((cid.clone(), 1));
            }
        }
    }
    if flat.is_empty() && !visible.is_empty() {
        // Fallback: show flat list when no parent-child edges.
        for &idx in &visible {
            flat.push((app.issues()[idx].id.clone(), 0));
        }
    }
    let sel = app.selected().min(flat.len().saturating_sub(1));
    let w = viewport_window(sel, flat.len(), area.height.saturating_sub(2) as usize);
    // Use computed window, not earlier roots window, for consistency.
    let _ = window;
    for (i, (id, depth)) in flat.iter().enumerate().skip(w.0).take(w.1 - w.0) {
        let is_sel = i == sel;
        let style = if is_sel {
            theme::selected_row()
        } else {
            theme::row()
        };
        let indent = "  ".repeat(*depth);
        let issue = app.issues().iter().find(|it| &it.id == id);
        let title = issue
            .map(|it| truncate(&it.title, 30))
            .unwrap_or_else(|| "-".to_string());
        lines.push(Line::from(vec![
            Span::styled(if is_sel { "› " } else { "  " }.to_string(), style),
            Span::styled(format!("{indent}{id} "), style),
            Span::styled(title, style),
        ]));
    }
    if flat.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no parent-child edges; flat list)",
            theme::dim(),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_label_dashboard(frame: &mut Frame, area: ratatui::layout::Rect, app: &TuiApp) {
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for issue in app.issues() {
        for l in &issue.labels {
            *counts.entry(l.clone()).or_default() += 1;
        }
        if issue.labels.is_empty() {
            *counts.entry("(unlabeled)".to_string()).or_default() += 1;
        }
    }
    let mut lines = vec![Line::from(vec![
        Span::styled(" Labels ", theme::primary()),
        Span::styled(format!("({} labels)", counts.len()), theme::dim()),
    ])];
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let sel = app.selected().min(sorted.len().saturating_sub(1));
    let window = viewport_window(sel, sorted.len(), area.height.saturating_sub(2) as usize);
    for idx in window.0..window.1 {
        let (label, count) = &sorted[idx];
        let is_sel = idx == sel;
        let style = if is_sel {
            theme::selected_row()
        } else {
            theme::row()
        };
        let bar = "█".repeat((*count).min(12));
        lines.push(Line::from(vec![
            Span::styled(if is_sel { "› " } else { "  " }.to_string(), style),
            Span::styled(format!("{label:<18} "), style),
            Span::styled(format!("{count:>3} "), theme::dim()),
            Span::styled(bar, theme::primary_fg()),
        ]));
    }
    if sorted.is_empty() {
        lines.push(Line::from(Span::styled("  (no labels)", theme::dim())));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

#[allow(
    clippy::too_many_lines,
    clippy::wildcard_enum_match_arm,
    clippy::needless_pass_by_value,
    clippy::drain_collect
)]
#[allow(clippy::pedantic, clippy::nursery)]
fn markdown_lines(md: &str, width: usize) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(md, opts);
    let mut lines: Vec<Line> = Vec::new();
    let mut cur_spans: Vec<Span<'static>> = Vec::new();
    let mut in_code_block = false;
    let mut code_lang: String = String::new();
    let mut strong = false;
    let mut em = false;
    let mut _code_inline = false;
    let mut heading_level: u8 = 0;
    for ev in parser {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                if !cur_spans.is_empty() {
                    lines.push(Line::from(cur_spans.clone()));
                    cur_spans.clear();
                }
                heading_level = level as u8;
            }
            Event::End(TagEnd::Heading(_)) => {
                if !cur_spans.is_empty() {
                    // style heading bold cyan
                    let styled: Vec<Span> = cur_spans
                        .drain(..)
                        .map(|s| {
                            Span::styled(
                                s.content.to_string(),
                                theme::primary_fg(),
                            )
                        })
                        .collect();
                    lines.push(Line::from(styled));
                }
                heading_level = 0;
            }
            Event::Start(Tag::Paragraph) => {
                if !cur_spans.is_empty() {
                    lines.push(Line::from(cur_spans.clone()));
                    cur_spans.clear();
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if !cur_spans.is_empty() {
                    lines.push(Line::from(cur_spans.clone()));
                    cur_spans.clear();
                }
                lines.push(Line::from(""));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                if !cur_spans.is_empty() {
                    lines.push(Line::from(cur_spans.clone()));
                    cur_spans.clear();
                }
                in_code_block = true;
                code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(l) => l.to_string(),
                    _ => String::new(),
                };
                if !code_lang.is_empty() {
                    lines.push(Line::from(                    Span::styled(
                        format!("  {} ─", code_lang),
                        theme::dim(),
                    )));
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                code_lang.clear();
            }
            Event::Start(Tag::Strong) => strong = true,
            Event::End(TagEnd::Strong) => strong = false,
            Event::Start(Tag::Emphasis) => em = true,
            Event::End(TagEnd::Emphasis) => em = false,
            Event::Start(Tag::Strikethrough) => {}
            Event::End(TagEnd::Strikethrough) => {}
            Event::Start(Tag::List(_)) | Event::End(TagEnd::List(_)) => {}
            Event::Start(Tag::Item) => cur_spans.push(Span::styled("• ".to_string(), theme::dim())),
            Event::End(TagEnd::Item) => {
                if !cur_spans.is_empty() {
                    lines.push(Line::from(cur_spans.clone()));
                    cur_spans.clear();
                }
            }
            Event::Text(text) => {
                let mut style = Style::new();
                if in_code_block {
                    style = theme::status_open().bg(Color::Rgb(0x12, 0x12, 0x2a));
                    // code block lines are kept as separate lines
                    for l in text.lines() {
                        lines.push(Line::from(Span::styled(format!("    {l}"), style)));
                    }
                    continue;
                }
                if strong {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if em {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if heading_level > 0 {
                    style = theme::primary_fg();
                }
                // inline split by soft breaks already handled; just push
                cur_spans.push(Span::styled(text.to_string(), style));
            }
            Event::Code(text) => {
                _code_inline = true;
                cur_spans.push(                    Span::styled(
                    format!("`{text}`"),
                    theme::status_warning(),
                ));
                _code_inline = false;
            }
            Event::SoftBreak | Event::HardBreak => {
                if !cur_spans.is_empty() {
                    lines.push(Line::from(cur_spans.clone()));
                    cur_spans.clear();
                }
            }
            Event::Html(_) | Event::FootnoteReference(_) => {}
            _ => {}
        }
    }
    if !cur_spans.is_empty() {
        lines.push(Line::from(cur_spans));
    }
    if width > 0 {
        // crude wrap: split long lines
        let mut wrapped: Vec<Line> = Vec::new();
        for line in lines {
            let s: String = line.spans.iter().map(|sp| sp.content.as_ref()).collect();
            if s.len() <= width {
                wrapped.push(line);
            } else {
                for chunk in s.chars().collect::<Vec<_>>().chunks(width) {
                    let ch: String = chunk.iter().collect();
                    wrapped.push(Line::from(Span::styled(ch, theme::row())));
                }
            }
        }
        wrapped
    } else {
        lines
    }
}

fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
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
    // Responsive header per bv thresholds (ultrawide>180, wide>140, split>100, mobile<=60)
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
            header_spans.push(Span::styled(format!("  [filter: {q}]"), theme::dim()));
        }
    } else if app.show_shortcuts() {
        header_spans.push(Span::styled("  [; shortcuts]", theme::dim()));
    }
    let header = Line::from(header_spans);
    let visible = app.visible_indices();
    let total = visible.len();
    let viewport = usize::from(area.height.saturating_sub(1)); // minus header
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
    // pagination line appended after rows (as footer of list pane)
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
            format!(" {}", issue.labels.join(","))
        } else {
            String::new()
        };
        // Mobile: collapse to ID + title only for iphone readability
        let row_spans = if area.width <= 40 {
            vec![
                Span::styled(selector.to_string(), style),
                Span::styled(format!("{:<10} ", issue.id), style),
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
                Span::styled(format!("{:<10} ", issue.id), style),
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
                Span::styled(format!("{:<14} ", issue.id), style),
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
    // If we have room, show pagination; otherwise it will be clipped but counts are in footer too.
    if rows.len() < usize::from(area.height) {
        rows.push(Line::from(""));
        rows.push(page_line);
    }
    frame.render_widget(Paragraph::new(rows), area);
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
                issue.title.clone(),
                theme::row().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            format!("{} │ {}", issue.id, issue.status.as_str()),
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
                issue.assignee.as_deref().unwrap_or("-").to_string(),
                theme::primary_fg(),
            ),
            Span::styled("  Pin: ", theme::dim()),
            Span::styled(
                issue.pin.as_deref().unwrap_or("-").to_string(),
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
            Span::styled(issue.labels.join(" • "), theme::status_warning()),
        ]));
    }
    if let Some(verify) = &issue.verify {
        lines.push(Line::from(vec![
            Span::styled("VERIFY: ", theme::dim()),
            Span::styled(verify.clone(), theme::status_open()),
        ]));
    }
    if !issue.principles.is_empty() {
        lines.push(Line::from(Span::styled("Principles:", theme::dim())));
        for p in &issue.principles {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  • {} — ", p.name),
                    theme::accent_lav(),
                ),
                Span::styled(p.decision.clone(), theme::row()),
            ]));
        }
    }
    if let Some(ac) = issue
        .acceptance_criteria
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        lines.push(Line::from(Span::styled("Acceptance:", theme::dim())));
        lines.push(Line::from(Span::styled(format!("  {ac}"), theme::row())));
    }
    // Dependencies
    if !issue.dependencies.is_empty() {
        lines.push(Line::from(Span::styled("Depends on:", theme::dim())));
        for dep in issue.dependencies.iter().take(8) {
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", dep.dep_type.as_str()), theme::dim()),
                Span::styled(dep.depends_on_id.clone(), theme::primary_fg()),
            ]));
        }
        if issue.dependencies.len() > 8 {
            lines.push(Line::from(Span::styled(
                format!("  … and {} more", issue.dependencies.len() - 8),
                theme::dim(),
            )));
        }
    }
    // Comments first (visible without scrolling past long description)
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
                Span::styled(format!("  {}: ", c.author), theme::status_warning()),
                Span::styled(c.created_at.format("%m/%d").to_string(), theme::dim()),
            ]));
            // render comment body with markdown
            for l in markdown_lines(&c.body, usize::from(area.width.saturating_sub(6))) {
                // indent comment body
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

    // Triage insight (from analysis cache)
    if let Some(rec) = app.triage_for(&issue.id) {
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
            Span::styled(rec.action.clone(), theme::row()),
        ]));
        for reason in rec.reasons.iter().take(3) {
            lines.push(Line::from(Span::styled(
                format!("  • {reason}"),
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
    // Graph analysis for this bead
    if let Some(analysis) = app.analysis() {
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

    // Description with markdown + syntax highlighting
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Description:",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    if let Some(description) = &issue.description {
        if crate::format::markdown::contains_markdown(description) {
            for l in markdown_lines(description, usize::from(area.width.saturating_sub(4))) {
                lines.push(l);
            }
        } else {
            for chunk in split_for_wrap(description, usize::from(area.width.saturating_sub(4))) {
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
            for l in markdown_lines(design, usize::from(area.width.saturating_sub(4))) {
                lines.push(l);
            }
        } else {
            for chunk in split_for_wrap(design, usize::from(area.width.saturating_sub(4))) {
                lines.push(Line::from(Span::styled(chunk, theme::row())));
            }
        }
    }

    // Apply scroll windowing: slice lines by detail_scroll and area height.
    let viewport = usize::from(area.height.saturating_sub(2)); // minus border
    let total = lines.len();
    let start = app.detail_scroll().min(total.saturating_sub(viewport));
    // If End was pressed (MAX/2), clamp to bottom.
    let start = start.min(total.saturating_sub(viewport));
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
    if width == 0 || text.len() <= width {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if cur.len() + word.len() + 1 > width && !cur.is_empty() {
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
        Span::styled(q.to_string(), theme::row()),
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
                " j/k move  G end  gg start  ctrl+d/u page  pgUp/Dn  enter detail  esc back  q quit",
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
                theme::status_open(),
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
                "j/k move • / search • b board • g graph • i insights • ; shortcuts • ? help • q quit"
            }
        }
        Focus::Search => "enter keep filter • esc cancel • backspace del • j/k nav",
        Focus::Detail => "j/k scroll • q/esc back • tab pane • ? help • ; shortcuts",
        Focus::Board
        | Focus::Graph
        | Focus::Actionable
        | Focus::Insights
        | Focus::Tree
        | Focus::LabelDashboard => "q/esc back • j/k nav • enter detail • ? help",
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
