//! Headless TUI state machine (bead gu7ts.6 + gu7ts.8 extras + gu7ts.7 views + pagination fix). governed-by: ADR-0003.
//!
//! Pure logic: no ratatui, no terminal I/O. `tests/tui_harness.rs` drives
//! [`TuiApp::handle_key`] directly to pin the focus contract (ADR-0003 §5
//! proof 4) without a pseudo-TTY.
//!
//! gu7ts.8 adds: search/filter (`/`), shortcuts sidebar (`;`/F2),
//! help overlay (`?`/F1), keybinding registry via `src/tui/keys.rs`.
//! gu7ts.7 + pagination fix adds: scroll-aware list (window keeps `selected`
//! visible), detail scroll, board/graph/actionable/insights/tree/label views.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use chrono::{DateTime, Utc};

use crate::analysis::{
    AnalysisConfig, AnalysisEngine, engine::AnalysisResult, triage::TriageResult,
};
use crate::model::Issue;

use super::actions::{ReloadSelection, TuiCmd, TuiMsg};

/// Keyboard focus owner (bv UX map §2.1, skeleton + extras + views).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// Main issue list; the root layer.
    List,
    /// Detail viewport for the selected issue.
    Detail,
    /// Centered quit confirmation over the top list.
    QuitConfirm,
    /// Modal input for a mutating action (close/delete/edit).
    Prompt,
    /// Incremental search input (filtering).
    Search,
    /// Help overlay (focusHelp).
    Help,
    /// Kanban board (grouped by status).
    Board,
    /// Dependency graph view.
    Graph,
    /// Actionable execution plan.
    Actionable,
    /// Insights metrics panel.
    Insights,
    /// Hierarchical tree.
    Tree,
    /// Label dashboard.
    LabelDashboard,
}

/// Terminal-independent key events the state machine understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Down,
    Up,
    Home,
    End,
    Enter,
    Esc,
    Tab,
    Backspace,
    PageDown,
    PageUp,
    CtrlC,
    CtrlD,
    CtrlU,
    CtrlJ,
    CtrlK,
    CtrlR,
    F1,
    F2,
    Char(char),
    Unhandled,
}

/// One board column: a status and the issue indices in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardColumn {
    pub status: String,
    pub rows: Vec<usize>,
}

/// One rendered tree row: the issue and its indent depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeRow {
    pub issue_index: usize,
    pub depth: usize,
}

/// One rendered label row: the label and how many issues carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelRow {
    pub label: String,
    pub count: usize,
}

/// Selection per derived view. `selected` stays the list/graph index; each
/// view that renders a different list owns an index into that list.
#[derive(Debug, Default, Clone, Copy)]
struct ViewSelections {
    board: usize,
    actionable: usize,
    tree: usize,
    label: usize,
}

const BOARD_STATUS_ORDER: [&str; 8] = [
    "open",
    "in_progress",
    "blocked",
    "deferred",
    "draft",
    "pinned",
    "closed",
    "tombstone",
];

/// Application state: selection, focus layer, and quit intent.
#[derive(Debug)]
pub struct TuiApp {
    issues: Vec<Issue>,
    selected: usize,
    focus: Focus,
    focus_before_help: Option<Focus>,
    width: u16,
    height: u16,
    status: Option<StatusLine>,
    /// Open modal input, when the user pressed an action key.
    prompt: Option<Prompt>,
    /// Layer to restore when the prompt closes.
    focus_before_prompt: Focus,
    should_quit: bool,
    /// Applied filter query (persisted after Search Enter). None = no filter.
    filter: Option<String>,
    /// Live search input buffer while in Search focus. Only meaningful when focus==Search.
    search_buffer: String,
    /// Shortcuts sidebar visibility (bv §5.1, `;`/F2).
    show_shortcuts: bool,
    /// Scroll offset inside the sidebar (ctrl+j/k).
    shortcuts_scroll: usize,
    /// Scroll offset inside the detail viewport (j/k, ctrl+d/u, g/G).
    detail_scroll: usize,
    /// Highest useful detail scroll for the current issue and viewport.
    detail_max_scroll: usize,
    /// Visible issue indices after the active filter, rebuilt when it changes.
    visible: Vec<usize>,
    /// Selection per derived view (board/actionable/tree/labels).
    view_selections: ViewSelections,
    /// Cached analysis for triage/graph sections, built on first detail open.
    analysis: Option<AnalysisResult>,
    triage: Option<TriageResult>,
    /// Clock captured at load/reload for deterministic triage.
    now: DateTime<Utc>,
}

/// The footer's one line: a success report or a refusal.
#[derive(Debug, Clone)]
enum StatusLine {
    Info(String),
    Error(String),
}

impl StatusLine {
    fn text(&self) -> &str {
        match self {
            Self::Info(message) | Self::Error(message) => message,
        }
    }
}

/// Modal action kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Close the selected issue with a commit citation.
    Close,
    /// Tombstone the selected issue with a reason.
    Delete,
    /// Rename the selected issue.
    EditTitle,
}

#[derive(Debug, Clone)]
struct PromptField {
    label: &'static str,
    value: String,
}

/// One modal input; fields are edited locally and submitted as one command.
#[derive(Debug, Clone)]
pub struct Prompt {
    kind: PromptKind,
    issue_id: String,
    fields: Vec<PromptField>,
    active: usize,
}

impl Prompt {
    #[must_use]
    pub const fn kind(&self) -> PromptKind {
        self.kind
    }

    #[must_use]
    pub fn issue_id(&self) -> &str {
        &self.issue_id
    }

    pub fn fields(&self) -> impl Iterator<Item = (&str, &str)> {
        self.fields
            .iter()
            .map(|field| (field.label, field.value.as_str()))
    }

    #[must_use]
    pub const fn active(&self) -> usize {
        self.active
    }

    #[must_use]
    pub const fn title(&self) -> &'static str {
        match self.kind {
            PromptKind::Close => "Close issue",
            PromptKind::Delete => "Delete issue",
            PromptKind::EditTitle => "Edit title",
        }
    }
}

fn prompt_for(kind: PromptKind, issue_id: &str, title: &str) -> Prompt {
    let fields = match kind {
        PromptKind::Close => vec![
            PromptField {
                label: "commit sha",
                value: String::new(),
            },
            PromptField {
                label: "reason (optional)",
                value: String::new(),
            },
        ],
        PromptKind::Delete => vec![PromptField {
            label: "reason (required)",
            value: String::new(),
        }],
        PromptKind::EditTitle => vec![PromptField {
            label: "title",
            value: title.to_string(),
        }],
    };
    Prompt {
        kind,
        issue_id: issue_id.to_string(),
        fields,
        active: 0,
    }
}

fn nav_view(selected: &mut usize, count: usize, key: Key, page: usize) {
    if count == 0 {
        *selected = 0;
        return;
    }
    let last = count - 1;
    match key {
        Key::Down | Key::Char('j') => *selected = (*selected + 1).min(last),
        Key::Up | Key::Char('k') => *selected = (*selected).saturating_sub(1),
        Key::Home => *selected = 0,
        Key::End => *selected = last,
        Key::PageDown | Key::CtrlD => *selected = (*selected + page).min(last),
        Key::PageUp | Key::CtrlU => *selected = (*selected).saturating_sub(page),
        _ => {}
    }
}

fn push_tree_rows<'a>(
    id: &'a str,
    depth: usize,
    children: &BTreeMap<&'a str, Vec<&'a str>>,
    by_id: &BTreeMap<&'a str, usize>,
    visited: &mut HashSet<&'a str>,
    rows: &mut Vec<TreeRow>,
) {
    const MAX_DEPTH: usize = 64;
    if depth > MAX_DEPTH || !visited.insert(id) {
        return;
    }
    if let Some(&issue_index) = by_id.get(id) {
        rows.push(TreeRow { issue_index, depth });
    }
    if let Some(kids) = children.get(id) {
        for &kid in kids {
            push_tree_rows(kid, depth + 1, children, by_id, visited, rows);
        }
    }
}

fn sort_issues(issues: &mut [Issue]) {
    issues.sort_by(|a, b| {
        status_sort_key(&a.status)
            .cmp(&status_sort_key(&b.status))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn status_sort_key(s: &crate::model::Status) -> u8 {
    match s.as_str() {
        "open" => 0,
        "in_progress" => 1,
        "blocked" => 2,
        "deferred" => 3,
        "draft" => 4,
        "closed" => 5,
        "tombstone" => 6,
        _ => 7,
    }
}

impl TuiApp {
    /// State over `issues` at bv's default 120x40.
    /// Display order: open → in_progress → blocked/deferred → closed last.
    #[must_use]
    pub fn new(issues: Vec<Issue>) -> Self {
        Self::new_at(issues, Utc::now())
    }

    /// [`TuiApp::new`] with the clock supplied by the edge, so a replay can
    /// rebuild the same triage.
    #[must_use]
    pub fn new_at(issues: Vec<Issue>, now: DateTime<Utc>) -> Self {
        let mut app = Self {
            issues: Vec::new(),
            selected: 0,
            focus: Focus::List,
            focus_before_help: None,
            focus_before_prompt: Focus::List,
            width: 120,
            height: 40,
            status: None,
            should_quit: false,
            filter: None,
            search_buffer: String::new(),
            show_shortcuts: false,
            shortcuts_scroll: 0,
            detail_scroll: 0,
            detail_max_scroll: 0,
            visible: Vec::new(),
            analysis: None,
            triage: None,
            now,
            prompt: None,
            view_selections: ViewSelections::default(),
        };
        app.adopt_issues(issues, now, ReloadSelection::KeepById);
        app
    }

    fn adopt_issues(
        &mut self,
        mut issues: Vec<Issue>,
        now: DateTime<Utc>,
        selection: ReloadSelection,
    ) {
        let selected_id = self.selected_issue().map(|issue| issue.id.clone());
        let selected_index = self.selected;
        sort_issues(&mut issues);
        self.issues = issues;
        self.now = now;
        self.analysis = None;
        self.triage = None;
        self.refresh_visible();
        self.selected = match selection {
            ReloadSelection::KeepById => selected_id
                .and_then(|id| {
                    self.visible
                        .iter()
                        .position(|&index| self.issues[index].id == id)
                })
                .unwrap_or(0),
            ReloadSelection::KeepIndex => selected_index,
        };
        self.clamp_selection();
        self.detail_scroll = 0;
        self.detail_max_scroll = 0;
        if self.focus == Focus::Detail {
            self.ensure_analysis();
            self.refresh_detail_max_scroll();
        }
    }

    fn ensure_analysis(&mut self) {
        if self.analysis.is_some() || self.issues.is_empty() {
            return;
        }
        let engine = AnalysisEngine::new(self.issues.clone());
        if self.issues.len() < 2000 {
            self.analysis = Some(engine.analyze(&AnalysisConfig::full()));
            self.triage = Some(crate::analysis::triage::compute_triage(
                &self.issues,
                self.now,
                env!("CARGO_PKG_VERSION"),
            ));
        } else {
            self.analysis = Some(engine.analyze_phase1());
            self.triage = None;
        }
    }

    #[must_use]
    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }

    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    #[must_use]
    pub const fn focus(&self) -> Focus {
        self.focus
    }

    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.should_quit
    }

    #[must_use]
    pub fn status_message(&self) -> Option<&str> {
        self.status.as_ref().map(StatusLine::text)
    }

    #[must_use]
    pub fn status_is_error(&self) -> bool {
        matches!(self.status.as_ref(), Some(StatusLine::Error(_)))
    }

    #[must_use]
    pub fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    /// Raw selected issue ignoring any filter — kept for backward compat
    /// with existing callers that index directly. Prefer `selected_visible_issue`.
    #[must_use]
    pub fn selected_issue(&self) -> Option<&Issue> {
        self.visible
            .get(self.selected)
            .and_then(|&idx| self.issues.get(idx))
    }

    /// Visible issue count (filtered or all).
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.visible.len()
    }

    #[must_use]
    pub const fn board_selection(&self) -> usize {
        self.view_selections.board
    }

    #[must_use]
    pub const fn actionable_selection(&self) -> usize {
        self.view_selections.actionable
    }

    #[must_use]
    pub const fn tree_selection(&self) -> usize {
        self.view_selections.tree
    }

    #[must_use]
    pub const fn label_selection(&self) -> usize {
        self.view_selections.label
    }

    /// Board columns in display order, one per status present.
    #[must_use]
    pub fn board_columns(&self) -> Vec<BoardColumn> {
        let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for &index in &self.visible {
            groups
                .entry(self.issues[index].status.as_str())
                .or_default()
                .push(index);
        }
        let mut columns = Vec::new();
        for status in BOARD_STATUS_ORDER {
            if let Some(rows) = groups.remove(status) {
                columns.push(BoardColumn {
                    status: status.to_string(),
                    rows,
                });
            }
        }
        for (status, rows) in groups {
            columns.push(BoardColumn {
                status: status.to_string(),
                rows,
            });
        }
        columns
    }

    /// Board cards in the order the board draws them.
    #[must_use]
    pub fn board_rows(&self) -> Vec<usize> {
        self.board_columns()
            .into_iter()
            .flat_map(|column| column.rows)
            .collect()
    }

    /// Ready work: open beads, priority first, then id.
    #[must_use]
    pub fn actionable_rows(&self) -> Vec<usize> {
        let mut rows: Vec<usize> = self
            .visible
            .iter()
            .copied()
            .filter(|&index| self.issues[index].status.as_str() == "open")
            .collect();
        rows.sort_by(|&left, &right| {
            self.issues[left]
                .priority
                .0
                .cmp(&self.issues[right].priority.0)
                .then_with(|| self.issues[left].id.cmp(&self.issues[right].id))
        });
        rows
    }

    /// Parent-child hierarchy, flattened depth-first. Issues outside a tree
    /// (including cycle leftovers) appear once at depth zero.
    #[must_use]
    pub fn tree_rows(&self) -> Vec<TreeRow> {
        let visible = &self.visible;
        let visible_ids: BTreeSet<&str> = visible
            .iter()
            .map(|&index| self.issues[index].id.as_str())
            .collect();
        let by_id: BTreeMap<&str, usize> = visible
            .iter()
            .map(|&index| (self.issues[index].id.as_str(), index))
            .collect();
        let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut has_parent: BTreeSet<&str> = BTreeSet::new();
        for &index in visible {
            let issue = &self.issues[index];
            for dependency in &issue.dependencies {
                if dependency.dep_type == crate::model::DependencyType::ParentChild
                    && visible_ids.contains(dependency.depends_on_id.as_str())
                {
                    children
                        .entry(dependency.depends_on_id.as_str())
                        .or_default()
                        .push(issue.id.as_str());
                    has_parent.insert(issue.id.as_str());
                }
            }
        }
        for kids in children.values_mut() {
            kids.sort_unstable();
            kids.dedup();
        }
        let roots: Vec<&str> = visible_ids
            .iter()
            .copied()
            .filter(|id| !has_parent.contains(*id))
            .collect();
        let mut rows = Vec::with_capacity(visible.len());
        let mut visited: HashSet<&str> = HashSet::new();
        for id in roots {
            push_tree_rows(id, 0, &children, &by_id, &mut visited, &mut rows);
        }
        for &id in &visible_ids {
            if !visited.contains(id) {
                push_tree_rows(id, 0, &children, &by_id, &mut visited, &mut rows);
            }
        }
        rows
    }

    /// Label counts, most used first, then label.
    #[must_use]
    pub fn label_rows(&self) -> Vec<LabelRow> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for issue in &self.issues {
            if issue.labels.is_empty() {
                *counts.entry("(unlabeled)".to_string()).or_default() += 1;
            } else {
                for label in &issue.labels {
                    *counts.entry(label.clone()).or_default() += 1;
                }
            }
        }
        let mut rows: Vec<LabelRow> = counts
            .into_iter()
            .map(|(label, count)| LabelRow { label, count })
            .collect();
        rows.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.label.cmp(&right.label))
        });
        rows
    }

    /// Currently active filter query (applied or live search).
    #[must_use]
    pub fn active_filter_query(&self) -> Option<&str> {
        if self.focus == Focus::Search {
            Some(&self.search_buffer)
        } else {
            self.filter.as_deref()
        }
    }

    /// Whether the shortcuts sidebar is visible.
    #[must_use]
    pub const fn show_shortcuts(&self) -> bool {
        self.show_shortcuts
    }

    #[must_use]
    pub const fn shortcuts_scroll(&self) -> usize {
        self.shortcuts_scroll
    }

    #[must_use]
    pub const fn detail_scroll(&self) -> usize {
        self.detail_scroll
    }

    /// Recompute the detail bottom from the current terminal geometry.
    pub fn refresh_detail_max_scroll(&mut self) {
        let (width, height) = self.detail_layout();
        let viewport = usize::from(height.saturating_sub(2));
        let total = super::ui::detail_line_count(self, width);
        self.detail_max_scroll = total.saturating_sub(viewport);
    }

    fn detail_layout(&self) -> (u16, u16) {
        let body_height = self.height.saturating_sub(1);
        let sidebar_width = if self.show_shortcuts && self.width > 34 {
            34
        } else {
            0
        };
        let main_width = self.width.saturating_sub(sidebar_width);
        let search_height = u16::from(self.is_searching() && body_height > 2);
        let body_height = body_height.saturating_sub(search_height);
        let detail_width = if self.is_split() {
            main_width.saturating_sub(main_width / 2)
        } else {
            main_width
        };
        (detail_width, body_height)
    }

    #[must_use]
    pub fn analysis(&self) -> Option<&AnalysisResult> {
        self.analysis.as_ref()
    }

    #[must_use]
    pub fn triage_for(&self, id: &str) -> Option<&crate::analysis::triage::Recommendation> {
        self.triage
            .as_ref()?
            .recommendations
            .iter()
            .find(|recommendation| recommendation.id == id)
    }

    #[must_use]
    pub fn triage_result(&self) -> Option<&TriageResult> {
        self.triage.as_ref()
    }

    /// Whether we are in incremental search input mode.
    #[must_use]
    pub fn is_searching(&self) -> bool {
        self.focus == Focus::Search
    }

    /// Split list+detail above width 100 (bv UX map §1.3); automatic and
    /// independent of the view layer.
    #[must_use]
    pub const fn is_split(&self) -> bool {
        self.width > 100
    }

    #[must_use]
    pub fn is_view_focus(&self) -> bool {
        matches!(
            self.focus,
            Focus::Board
                | Focus::Graph
                | Focus::Actionable
                | Focus::Insights
                | Focus::Tree
                | Focus::LabelDashboard
        )
    }

    /// Resize hook from the render loop.
    pub fn set_size(&mut self, width: u16, height: u16) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        if self.focus == Focus::Detail {
            self.refresh_detail_max_scroll();
        }
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some(StatusLine::Info(message.into()));
    }

    fn set_status_error(&mut self, message: impl Into<String>) {
        self.status = Some(StatusLine::Error(message.into()));
    }

    fn clamp_selection(&mut self) {
        let count = self.visible_count();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
        self.clamp_view_selections();
    }

    fn clamp_view_selections(&mut self) {
        self.view_selections.board = self
            .view_selections
            .board
            .min(self.board_rows().len().saturating_sub(1));
        self.view_selections.actionable = self
            .view_selections
            .actionable
            .min(self.actionable_rows().len().saturating_sub(1));
        self.view_selections.tree = self
            .view_selections
            .tree
            .min(self.tree_rows().len().saturating_sub(1));
        self.view_selections.label = self
            .view_selections
            .label
            .min(self.label_rows().len().saturating_sub(1));
    }

    fn matches_filter(issue: &Issue, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        if let Some(label) = query.strip_prefix("label:") {
            let label = label.trim();
            return !label.is_empty()
                && issue
                    .labels
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(label));
        }
        let q = query.to_lowercase();
        issue.id.to_lowercase().contains(&q)
            || issue.title.to_lowercase().contains(&q)
            || issue.status.as_str().to_lowercase().contains(&q)
            || issue
                .description
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&q)
            || issue.labels.iter().any(|l| l.to_lowercase().contains(&q))
    }

    #[must_use]
    pub fn visible_indices(&self) -> &[usize] {
        &self.visible
    }

    fn refresh_visible(&mut self) {
        let query = if self.focus == Focus::Search {
            Some(self.search_buffer.as_str())
        } else {
            self.filter.as_deref()
        };
        self.visible = match query {
            Some(q) if !q.is_empty() => self
                .issues
                .iter()
                .enumerate()
                .filter(|(_, issue)| Self::matches_filter(issue, q))
                .map(|(index, _)| index)
                .collect(),
            _ => (0..self.issues.len()).collect(),
        };
    }

    fn viewport_rows(&self) -> usize {
        let mut h = usize::from(self.height.saturating_sub(1));
        if self.is_searching() {
            h = h.saturating_sub(1);
        }
        h.saturating_sub(1)
    }

    fn page_step(&self) -> usize {
        (self.viewport_rows() / 3).max(1)
    }

    /// Apply one key event and return the effects the fold asks for.
    ///
    /// Pure: no storage, clock, or terminal access. The caller runs each
    /// command and feeds the reply back through [`TuiApp::update`].
    #[allow(clippy::too_many_lines)]
    pub fn handle_key(&mut self, key: Key) -> Vec<TuiCmd> {
        self.status = None;

        if key == Key::CtrlC {
            self.should_quit = true;
            return Vec::new();
        }

        if self.focus == Focus::Prompt {
            return self.handle_prompt_key(key);
        }

        if self.focus == Focus::Search {
            self.handle_search_key(key);
            return Vec::new();
        }

        if self.focus == Focus::Help {
            let prev = self.focus_before_help.take().unwrap_or(Focus::List);
            self.focus = prev;
            if key != Key::Esc && key != Key::Char('?') && key != Key::F1 {
                self.set_status("help closed");
            }
            return Vec::new();
        }

        if self.focus == Focus::QuitConfirm {
            if key == Key::Char('y') {
                self.should_quit = true;
            } else {
                self.focus = Focus::List;
                self.set_status("quit cancelled");
            }
            return Vec::new();
        }

        if key == Key::CtrlR {
            return vec![TuiCmd::Reload];
        }

        if self.is_view_focus() {
            match key {
                Key::Char('q') | Key::Esc => {
                    self.focus = Focus::List;
                    self.detail_scroll = 0;
                    return Vec::new();
                }
                Key::Enter => {
                    self.enter_selected_view_row();
                    return Vec::new();
                }
                Key::Tab if self.is_split() => {
                    self.focus = Focus::List;
                    return Vec::new();
                }
                Key::Down
                | Key::Up
                | Key::Home
                | Key::End
                | Key::PageDown
                | Key::PageUp
                | Key::CtrlD
                | Key::CtrlU => {
                    self.handle_view_nav(key);
                    return Vec::new();
                }
                _ => {}
            }
        }

        if self.show_shortcuts {
            match key {
                Key::CtrlJ => {
                    self.shortcuts_scroll = self.shortcuts_scroll.saturating_add(1);
                    return Vec::new();
                }
                Key::CtrlK => {
                    self.shortcuts_scroll = self.shortcuts_scroll.saturating_sub(1);
                    return Vec::new();
                }
                _ => {}
            }
        }

        if matches!(self.focus, Focus::List | Focus::Detail) {
            match key {
                Key::Char('c') => {
                    self.begin_prompt(PromptKind::Close);
                    return Vec::new();
                }
                Key::Char('d') => {
                    self.begin_prompt(PromptKind::Delete);
                    return Vec::new();
                }
                Key::Char('e') => {
                    self.begin_prompt(PromptKind::EditTitle);
                    return Vec::new();
                }
                _ => {}
            }
        }

        match key {
            Key::Char('?') | Key::F1 => {
                self.focus_before_help = Some(self.focus);
                self.focus = Focus::Help;
                return Vec::new();
            }
            Key::Char(';') | Key::F2 => {
                self.show_shortcuts = !self.show_shortcuts;
                if self.show_shortcuts {
                    self.shortcuts_scroll = 0;
                    self.set_status("Shortcuts sidebar: ; hide | ctrl+j/k scroll");
                } else {
                    self.set_status("shortcuts hidden");
                }
                if self.focus == Focus::Detail {
                    self.refresh_detail_max_scroll();
                }
                return Vec::new();
            }
            Key::Char('b') => {
                self.toggle_view(Focus::Board);
                return Vec::new();
            }
            Key::Char('g') => {
                self.toggle_view(Focus::Graph);
                return Vec::new();
            }
            Key::Char('a') => {
                self.toggle_view(Focus::Actionable);
                return Vec::new();
            }
            Key::Char('i') => {
                self.toggle_view(Focus::Insights);
                return Vec::new();
            }
            Key::Char('E') => {
                self.toggle_view(Focus::Tree);
                return Vec::new();
            }
            Key::Char('[') => {
                self.toggle_view(Focus::LabelDashboard);
                return Vec::new();
            }
            _ => {}
        }

        match self.focus {
            Focus::List => self.handle_list_key(key),
            Focus::Detail => self.handle_detail_key(key),
            Focus::Board
            | Focus::Graph
            | Focus::Actionable
            | Focus::Insights
            | Focus::Tree
            | Focus::LabelDashboard => {
                self.handle_view_nav(key);
            }
            Focus::Search | Focus::Help | Focus::QuitConfirm | Focus::Prompt => {}
        }
        Vec::new()
    }

    /// Adopt one interpreter reply. The only place issue rows change after boot.
    pub fn update(&mut self, msg: TuiMsg) {
        match msg {
            TuiMsg::Reloaded {
                issues,
                now,
                status,
                selection,
            } => {
                self.adopt_issues(issues, now, selection);
                self.set_status(status);
            }
            TuiMsg::Failed { message } => {
                self.set_status_error(format!("action failed: {message}"));
            }
        }
    }

    fn begin_prompt(&mut self, kind: PromptKind) {
        let Some(issue) = self.selected_issue() else {
            self.set_status_error("no issue selected");
            return;
        };
        let id = issue.id.clone();
        let title = issue.title.clone();
        let status = issue.status.as_str().to_string();
        let terminal = issue.status.is_terminal();
        if kind == PromptKind::Close && terminal {
            self.set_status_error(format!("{id} is already {status}"));
            return;
        }
        if kind == PromptKind::Delete && status == "tombstone" {
            self.set_status_error(format!("{id} is already deleted"));
            return;
        }
        self.focus_before_prompt = self.focus;
        self.prompt = Some(prompt_for(kind, &id, &title));
        self.focus = Focus::Prompt;
    }

    fn handle_prompt_key(&mut self, key: Key) -> Vec<TuiCmd> {
        match key {
            Key::Esc => {
                self.prompt = None;
                self.focus = self.focus_before_prompt;
                self.set_status("action cancelled");
                Vec::new()
            }
            Key::Tab | Key::Down => {
                if let Some(prompt) = &mut self.prompt {
                    let count = prompt.fields.len();
                    if count > 0 {
                        prompt.active = (prompt.active + 1) % count;
                    }
                }
                Vec::new()
            }
            Key::Up => {
                if let Some(prompt) = &mut self.prompt {
                    let count = prompt.fields.len();
                    if count > 0 {
                        prompt.active = (prompt.active + count - 1) % count;
                    }
                }
                Vec::new()
            }
            Key::Backspace => {
                if let Some(field) = self.active_prompt_field_mut() {
                    field.value.pop();
                }
                Vec::new()
            }
            Key::CtrlU => {
                if let Some(field) = self.active_prompt_field_mut() {
                    field.value.clear();
                }
                Vec::new()
            }
            Key::Enter => self.submit_prompt(),
            Key::Char(character) => {
                if let Some(field) = self.active_prompt_field_mut() {
                    field.value.push(character);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn active_prompt_field_mut(&mut self) -> Option<&mut PromptField> {
        let prompt = self.prompt.as_mut()?;
        let active = prompt.active;
        prompt.fields.get_mut(active)
    }

    fn submit_prompt(&mut self) -> Vec<TuiCmd> {
        let Some(prompt) = self.prompt.take() else {
            return Vec::new();
        };
        let expected_revision = self
            .issues
            .iter()
            .find(|issue| issue.id == prompt.issue_id)
            .map(|issue| issue.revision);
        let values: Vec<String> = prompt
            .fields
            .iter()
            .map(|field| field.value.trim().to_string())
            .collect();

        let command = match prompt.kind {
            PromptKind::Close => {
                let Some(commit_sha) = values.first().filter(|value| !value.is_empty()).cloned()
                else {
                    self.prompt = Some(prompt);
                    self.set_status_error("commit sha is required");
                    return Vec::new();
                };
                TuiCmd::Close {
                    issue_id: prompt.issue_id.clone(),
                    commit_sha,
                    reason: values.get(1).cloned().unwrap_or_default(),
                    expected_revision,
                }
            }
            PromptKind::Delete => {
                let Some(reason) = values.first().filter(|value| !value.is_empty()).cloned() else {
                    self.prompt = Some(prompt);
                    self.set_status_error("delete reason is required");
                    return Vec::new();
                };
                TuiCmd::Delete {
                    issue_id: prompt.issue_id.clone(),
                    reason,
                    expected_revision,
                }
            }
            PromptKind::EditTitle => {
                let Some(title) = values.first().filter(|value| !value.is_empty()).cloned() else {
                    self.prompt = Some(prompt);
                    self.set_status_error("title cannot be empty");
                    return Vec::new();
                };
                TuiCmd::EditTitle {
                    issue_id: prompt.issue_id.clone(),
                    title,
                    expected_revision,
                }
            }
        };

        self.focus = self.focus_before_prompt;
        self.set_status(match prompt.kind {
            PromptKind::Close => "closing",
            PromptKind::Delete => "deleting",
            PromptKind::EditTitle => "saving",
        });
        vec![command]
    }

    fn toggle_view(&mut self, view: Focus) {
        if self.focus == view {
            self.focus = Focus::List;
        } else {
            self.focus = view;
        }
        self.detail_scroll = 0;
    }

    fn handle_view_nav(&mut self, key: Key) {
        let page = self.page_step();
        match self.focus {
            Focus::Board => {
                let count = self.board_rows().len();
                nav_view(&mut self.view_selections.board, count, key, page);
            }
            Focus::Actionable => {
                let count = self.actionable_rows().len();
                nav_view(&mut self.view_selections.actionable, count, key, page);
            }
            Focus::Tree => {
                let count = self.tree_rows().len();
                nav_view(&mut self.view_selections.tree, count, key, page);
            }
            Focus::LabelDashboard => {
                let count = self.label_rows().len();
                nav_view(&mut self.view_selections.label, count, key, page);
            }
            Focus::Graph => {
                let count = self.visible_count();
                nav_view(&mut self.selected, count, key, page);
            }
            Focus::Insights
            | Focus::List
            | Focus::Detail
            | Focus::Search
            | Focus::Help
            | Focus::QuitConfirm
            | Focus::Prompt => {}
        }
    }

    fn enter_selected_view_row(&mut self) {
        if self.focus == Focus::LabelDashboard {
            self.apply_label_filter();
            return;
        }
        let Some(issue_index) = self.selected_view_issue_index() else {
            return;
        };
        if let Some(position) = self.visible.iter().position(|&index| index == issue_index) {
            self.selected = position;
        }
        self.ensure_analysis();
        self.focus = Focus::Detail;
        self.detail_scroll = 0;
        self.refresh_detail_max_scroll();
    }

    fn selected_view_issue_index(&self) -> Option<usize> {
        match self.focus {
            Focus::Board => {
                let rows = self.board_rows();
                rows.get(self.view_selections.board).copied()
            }
            Focus::Actionable => {
                let rows = self.actionable_rows();
                rows.get(self.view_selections.actionable).copied()
            }
            Focus::Tree => {
                let rows = self.tree_rows();
                rows.get(self.view_selections.tree)
                    .map(|row| row.issue_index)
            }
            Focus::Graph => self.visible.get(self.selected).copied(),
            Focus::Insights
            | Focus::LabelDashboard
            | Focus::List
            | Focus::Detail
            | Focus::Search
            | Focus::Help
            | Focus::QuitConfirm
            | Focus::Prompt => None,
        }
    }

    fn apply_label_filter(&mut self) {
        let rows = self.label_rows();
        let Some(row) = rows.get(self.view_selections.label) else {
            return;
        };
        if row.label == "(unlabeled)" {
            self.set_status_error("unlabeled filter is not supported; use `br list --no-label`");
            return;
        }
        self.filter = Some(format!("label:{}", row.label));
        self.focus = Focus::List;
        self.refresh_visible();
        self.selected = 0;
        self.clamp_selection();
        self.set_status(format!("filter: label:{}", row.label));
    }

    fn handle_search_key(&mut self, key: Key) {
        match key {
            Key::Esc => {
                // Cancel only the in-progress search; a filter committed by a
                // previous Enter stays applied (Esc on the list clears it).
                self.search_buffer.clear();
                self.focus = Focus::List;
                self.refresh_visible();
                self.selected = 0;
                self.clamp_selection();
                self.set_status("search cancelled");
            }
            Key::Enter => {
                let committed = self.search_buffer.trim().to_string();
                self.search_buffer.clear();
                self.filter = if committed.is_empty() {
                    None
                } else {
                    Some(committed)
                };
                self.focus = Focus::List;
                self.refresh_visible();
                self.clamp_selection();
                if self.filter.is_some() {
                    self.set_status(format!("filter: {} matches", self.visible.len()));
                } else {
                    self.set_status("filter cleared");
                }
            }
            Key::Backspace => {
                self.search_buffer.pop();
                self.refresh_visible();
                self.selected = 0;
            }
            Key::Down if self.selected + 1 < self.visible_count() => {
                self.selected += 1;
            }
            Key::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            Key::Home => self.selected = 0,
            Key::End => self.selected = self.visible_count().saturating_sub(1),
            Key::Char(c) => {
                self.search_buffer.push(c);
                self.refresh_visible();
                self.selected = 0;
            }
            _ => {}
        }
    }

    fn handle_list_key(&mut self, key: Key) {
        if key == Key::Esc && self.filter.is_some() {
            self.filter = None;
            self.refresh_visible();
            self.selected = 0;
            self.clamp_selection();
            self.set_status("filter cleared");
            return;
        }
        match key {
            Key::Down | Key::Char('j') => {
                let count = self.visible_count();
                if count == 0 {
                    return;
                }
                if self.selected + 1 < count {
                    self.selected += 1;
                }
            }
            Key::Up | Key::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            Key::Home | Key::Char('g') => self.selected = 0,
            Key::End | Key::Char('G') => {
                let count = self.visible_count();
                self.selected = count.saturating_sub(1);
            }
            Key::PageDown | Key::CtrlD => {
                let count = self.visible_count();
                if count == 0 {
                    return;
                }
                self.selected = (self.selected + self.page_step()).min(count.saturating_sub(1));
            }
            Key::PageUp | Key::CtrlU => {
                self.selected = self.selected.saturating_sub(self.page_step());
            }
            Key::Enter => {
                self.ensure_analysis();
                self.focus = Focus::Detail;
                self.detail_scroll = 0;
                self.refresh_detail_max_scroll();
            }
            Key::Tab if self.is_split() => {
                self.ensure_analysis();
                self.focus = Focus::Detail;
                self.detail_scroll = 0;
                self.refresh_detail_max_scroll();
            }
            Key::Char('q') => self.should_quit = true,
            Key::Esc => self.focus = Focus::QuitConfirm,
            Key::Char('/') => {
                self.focus = Focus::Search;
                self.search_buffer.clear();
                self.refresh_visible();
                self.selected = 0;
            }
            _ => {}
        }
    }

    fn handle_detail_key(&mut self, key: Key) {
        match key {
            Key::Char('q') | Key::Esc => {
                self.focus = Focus::List;
                self.detail_scroll = 0;
            }
            Key::Tab if self.is_split() => {
                self.focus = Focus::List;
                self.detail_scroll = 0;
            }
            Key::Down | Key::Char('j') => {
                self.detail_scroll = self
                    .detail_scroll
                    .saturating_add(1)
                    .min(self.detail_max_scroll);
            }
            Key::Up | Key::Char('k') => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
            Key::PageDown | Key::CtrlD => {
                self.detail_scroll = self
                    .detail_scroll
                    .saturating_add(self.page_step())
                    .min(self.detail_max_scroll);
            }
            Key::PageUp | Key::CtrlU => {
                self.detail_scroll = self.detail_scroll.saturating_sub(self.page_step());
            }
            Key::Home | Key::Char('g') => self.detail_scroll = 0,
            Key::End | Key::Char('G') => self.detail_scroll = self.detail_max_scroll,
            _ => {}
        }
    }
}
