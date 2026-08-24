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

use crate::analysis::{
    AnalysisConfig, AnalysisEngine, engine::AnalysisResult, triage::TriageResult,
};
use crate::model::Issue;

/// Keyboard focus owner (bv UX map §2.1, skeleton + extras + views).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// Main issue list; the root layer.
    List,
    /// Detail viewport for the selected issue.
    Detail,
    /// Centered quit confirmation over the top list.
    QuitConfirm,
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
    F1,
    F2,
    Char(char),
    Other(crossterm::event::KeyCode),
}

/// Application state: selection, focus layer, and quit intent.
#[derive(Debug)]
pub struct TuiApp {
    issues: Vec<Issue>,
    selected: usize,
    focus: Focus,
    focus_before_help: Option<Focus>,
    width: u16,
    height: u16,
    status_message: Option<String>,
    should_quit: bool,
    // gu7ts.8 extras
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
    /// Cached analysis for triage/graph sections (bv parity, computed once).
    analysis: Option<AnalysisResult>,
    triage: Option<TriageResult>,
}

impl TuiApp {
    /// State over `issues` (expected id-sorted) at bv's default 120x40.
    #[must_use]
    pub fn new(issues: Vec<Issue>) -> Self {
        // Precompute analysis once for triage insights + graph analysis sections.
        // Keep it cheap: full triage only for <2000 issues, else phase1.
        let (analysis, triage) = if issues.is_empty() {
            (None, None)
        } else if issues.len() < 2000 {
            let engine = AnalysisEngine::new(issues.clone());
            let a = engine.analyze(&AnalysisConfig::full());
            let t = crate::analysis::triage::compute_triage(
                &issues,
                chrono::Utc::now(),
                env!("CARGO_PKG_VERSION"),
            );
            (Some(a), Some(t))
        } else {
            let engine = AnalysisEngine::new(issues.clone());
            let a = engine.analyze_phase1();
            (Some(a), None)
        };
        Self {
            issues,
            selected: 0,
            focus: Focus::List,
            focus_before_help: None,
            width: 120,
            height: 40,
            status_message: None,
            should_quit: false,
            filter: None,
            search_buffer: String::new(),
            show_shortcuts: false,
            shortcuts_scroll: 0,
            detail_scroll: 0,
            analysis,
            triage,
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
        self.status_message.as_deref()
    }

    /// Raw selected issue ignoring any filter — kept for backward compat
    /// with existing callers that index directly. Prefer `selected_visible_issue`.
    #[must_use]
    pub fn selected_issue(&self) -> Option<&Issue> {
        let vis = self.visible_indices();
        vis.get(self.selected).and_then(|&idx| self.issues.get(idx))
    }

    /// Visible issue count (filtered or all).
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.visible_indices().len()
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

    #[must_use]
    pub fn analysis(&self) -> Option<&AnalysisResult> {
        self.analysis.as_ref()
    }

    #[must_use]
    pub fn triage_for(&self, id: &str) -> Option<crate::analysis::triage::Recommendation> {
        let t = self.triage.as_ref()?;
        t.recommendations.iter().find(|r| r.id == id).cloned()
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
        self.width = width;
        self.height = height;
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some(message.into());
    }

    fn clamp_selection(&mut self) {
        let count = self.visible_count();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }

    fn matches_filter(issue: &Issue, query: &str) -> bool {
        if query.is_empty() {
            return true;
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
    pub fn visible_indices(&self) -> Vec<usize> {
        let query_opt: Option<&str> = if self.focus == Focus::Search {
            Some(&self.search_buffer)
        } else {
            self.filter.as_deref()
        };
        let Some(q) = query_opt else {
            return (0..self.issues.len()).collect();
        };
        if q.is_empty() {
            return (0..self.issues.len()).collect();
        }
        self.issues
            .iter()
            .enumerate()
            .filter(|(_, issue)| Self::matches_filter(issue, q))
            .map(|(idx, _)| idx)
            .collect()
    }

    fn viewport_rows(&self) -> usize {
        // Body height = total - footer (1) - optional search bar (1).
        let mut h = usize::from(self.height.saturating_sub(1));
        if self.is_searching() {
            h = h.saturating_sub(1);
        }
        // Reserve header line.
        h.saturating_sub(1)
    }

    fn page_step(&self) -> usize {
        (self.viewport_rows() / 3).max(1)
    }

    /// Apply one key event. This is the whole focus contract.
    #[allow(clippy::too_many_lines)]
    pub fn handle_key(&mut self, key: Key) {
        // Status messages clear on any keypress (bv footer rule §1.6).
        self.status_message = None;

        if key == Key::CtrlC {
            self.should_quit = true;
            return;
        }

        // Search input consumes all keys (bv §3.2: filtering suppresses globals).
        if self.focus == Focus::Search {
            self.handle_search_key(key);
            return;
        }

        // Help overlay: any key closes and restores focus (bv §5.2).
        if self.focus == Focus::Help {
            let prev = self.focus_before_help.take().unwrap_or(Focus::List);
            self.focus = prev;
            if key != Key::Esc && key != Key::Char('?') && key != Key::F1 {
                self.set_status("help closed");
            }
            return;
        }

        // QuitConfirm handled before globals.
        if self.focus == Focus::QuitConfirm {
            if key == Key::Char('y') {
                self.should_quit = true;
            } else {
                self.focus = Focus::List;
                self.set_status("quit cancelled");
            }
            return;
        }

        // View overlays: q/esc/tab close back to list.
        if self.is_view_focus() {
            match key {
                Key::Char('q') | Key::Esc => {
                    self.focus = Focus::List;
                    self.detail_scroll = 0;
                    return;
                }
                Key::Enter => {
                    // Drill-down: keep selection, open detail (split keeps pane, single replaces).
                    self.focus = Focus::Detail;
                    self.detail_scroll = 0;
                    return;
                }
                Key::Tab if self.is_split() => {
                    self.focus = Focus::List;
                    return;
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
                    return;
                }
                _ => {}
            }
            // Fall through to global toggles for view switching (b/g/… switch views directly).
        }

        // Shortcuts sidebar scroll when visible (ctrl+j/k) — available from any non-search/help/quit/view layer.
        if self.show_shortcuts {
            match key {
                Key::CtrlJ => {
                    self.shortcuts_scroll = self.shortcuts_scroll.saturating_add(1);
                    return;
                }
                Key::CtrlK => {
                    self.shortcuts_scroll = self.shortcuts_scroll.saturating_sub(1);
                    return;
                }
                _ => {}
            }
        }

        // Global toggles when not filtering (bv §3.1, filtering gates)
        // Applied filter does NOT suppress these.
        match key {
            Key::Char('?') | Key::F1 => {
                self.focus_before_help = Some(self.focus);
                self.focus = Focus::Help;
                return;
            }
            Key::Char(';') | Key::F2 => {
                self.show_shortcuts = !self.show_shortcuts;
                if self.show_shortcuts {
                    self.shortcuts_scroll = 0;
                    self.set_status("Shortcuts sidebar: ; hide | ctrl+j/k scroll");
                } else {
                    self.set_status("shortcuts hidden");
                }
                return;
            }
            Key::Char('b') => {
                return self.toggle_view(Focus::Board);
            }
            Key::Char('g') => {
                return self.toggle_view(Focus::Graph);
            }
            Key::Char('a') => {
                return self.toggle_view(Focus::Actionable);
            }
            Key::Char('i') => {
                return self.toggle_view(Focus::Insights);
            }
            Key::Char('E') => {
                return self.toggle_view(Focus::Tree);
            }
            Key::Char('[') => {
                return self.toggle_view(Focus::LabelDashboard);
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
                // View overlays reuse list navigation for any unhandled key (don't panic on stray input)
                self.handle_view_nav(key);
            }
            Focus::Search | Focus::Help | Focus::QuitConfirm => {
                // Already returned above; reaching here means a stray key arrived after early return race — ignore
            }
        }
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
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        match key {
            Key::Down => {
                if self.selected + 1 < count {
                    self.selected += 1;
                }
            }
            Key::Up => self.selected = self.selected.saturating_sub(1),
            Key::Home => self.selected = 0,
            Key::End => self.selected = count.saturating_sub(1),
            Key::PageDown | Key::CtrlD => {
                self.selected = (self.selected + self.page_step()).min(count.saturating_sub(1));
            }
            Key::PageUp | Key::CtrlU => {
                self.selected = self.selected.saturating_sub(self.page_step());
            }
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: Key) {
        match key {
            Key::Esc => {
                self.search_buffer.clear();
                self.filter = None;
                self.focus = Focus::List;
                self.selected = 0;
                self.set_status("search cancelled");
            }
            Key::Enter => {
                let committed = self.search_buffer.trim().to_string();
                if committed.is_empty() {
                    self.filter = None;
                    self.set_status("filter cleared");
                } else {
                    self.filter = Some(committed);
                    self.set_status(format!("filter: {} matches", self.visible_indices().len()));
                }
                self.search_buffer.clear();
                self.focus = Focus::List;
                self.clamp_selection();
            }
            Key::Backspace => {
                self.search_buffer.pop();
                self.selected = 0;
            }
            Key::Down => {
                if self.selected + 1 < self.visible_count() {
                    self.selected += 1;
                }
            }
            Key::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            Key::Home => self.selected = 0,
            Key::End => self.selected = self.visible_count().saturating_sub(1),
            Key::Char(c) => {
                self.search_buffer.push(c);
                self.selected = 0;
            }
            Key::Other(k) => {
                let _ = k;
            }
            _ => {}
        }
    }

    fn handle_list_key(&mut self, key: Key) {
        if key == Key::Esc && self.filter.is_some() {
            self.filter = None;
            self.selected = 0;
            self.set_status("filter cleared");
            return;
        }
        match key {
            Key::Down => {
                let count = self.visible_count();
                if count == 0 {
                    return;
                }
                if self.selected + 1 < count {
                    self.selected += 1;
                }
            }
            Key::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            Key::Home => self.selected = 0,
            Key::End => {
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
                self.focus = Focus::Detail;
                self.detail_scroll = 0;
            }
            Key::Tab if self.is_split() => {
                self.focus = Focus::Detail;
                self.detail_scroll = 0;
            }
            Key::Char('q') => self.should_quit = true,
            Key::Esc => self.focus = Focus::QuitConfirm,
            Key::Char('/') => {
                self.focus = Focus::Search;
                self.search_buffer.clear();
                self.selected = 0;
            }
            _ => {}
        }
    }

    #[allow(clippy::semicolon_if_nothing_returned)]
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
            Key::Down | Key::Char('j') => self.detail_scroll = self.detail_scroll.saturating_add(1),
            Key::Up | Key::Char('k') => self.detail_scroll = self.detail_scroll.saturating_sub(1),
            Key::PageDown | Key::CtrlD => {
                self.detail_scroll = self.detail_scroll.saturating_add(self.page_step())
            }
            Key::PageUp | Key::CtrlU => {
                self.detail_scroll = self.detail_scroll.saturating_sub(self.page_step())
            }
            Key::Home => self.detail_scroll = 0,
            Key::End => self.detail_scroll = usize::MAX / 2, // clamped in draw
            _ => {}
        }
    }
}
