//! Headless TUI state machine (bead gu7ts.6 + gu7ts.8 extras). governed-by: ADR-0003.
//!
//! Pure logic: no ratatui, no terminal I/O. `tests/tui_harness.rs` drives
//! [`TuiApp::handle_key`] directly to pin the focus contract (ADR-0003 §5
//! proof 4) without a pseudo-TTY.
//!
//! gu7ts.8 adds: search/filter (`/`), shortcuts sidebar (`;`/F2),
//! help overlay (`?`/F1), and the keybinding registry surface via
//! `src/tui/keys.rs` (bv UX map §3/§5).

use crate::model::Issue;

/// Keyboard focus owner (bv UX map §2.1, skeleton + extras).
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
    CtrlC,
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
}

impl TuiApp {
    /// State over `issues` (expected id-sorted) at bv's default 120x40.
    #[must_use]
    pub fn new(issues: Vec<Issue>) -> Self {
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

    /// Apply one key event. This is the whole focus contract.
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
            // Any key closes help; ctrl+c already handled.
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

        // Shortcuts sidebar scroll when visible (ctrl+j/k) — available from any non-search/help/quit layer.
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
        // Note: filtering means search input active; applied filter does NOT suppress these.
        let is_filtering_input = false; // we already returned if Search; so false here
        if !is_filtering_input {
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
                _ => {}
            }
        }

        match self.focus {
            Focus::List => self.handle_list_key(key),
            Focus::Detail => self.handle_detail_key(key),
            Focus::Search | Focus::Help | Focus::QuitConfirm => unreachable!(),
        }
    }

    fn handle_search_key(&mut self, key: Key) {
        match key {
            Key::Esc => {
                // Cancel search: clear buffer and filter, return to list.
                self.search_buffer.clear();
                self.filter = None;
                self.focus = Focus::List;
                self.selected = 0;
                self.set_status("search cancelled");
            }
            Key::Enter => {
                // Commit filter: keep buffer as applied filter if non-empty, else clear.
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
                // Treat other printable as char if possible — handled via to_key as Char already.
                let _ = k;
            }
            _ => {}
        }
    }

    fn handle_list_key(&mut self, key: Key) {
        // Esc at top list: first clear applied filter if active, else quit-confirm (bv §3.0).
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
            Key::Enter => self.focus = Focus::Detail,
            Key::Tab if self.is_split() => self.focus = Focus::Detail,
            // Top of the layer stack: q quits outright, esc asks first.
            Key::Char('q') => self.should_quit = true,
            Key::Esc => self.focus = Focus::QuitConfirm,
            Key::Char('/') => {
                // Enter incremental search.
                self.focus = Focus::Search;
                self.search_buffer.clear();
                self.selected = 0;
            }
            // Sidebar toggle already handled globally, but keep here for List context redundancy.
            _ => {}
        }
    }

    fn handle_detail_key(&mut self, key: Key) {
        match key {
            // q/esc close the detail layer back to the list (split keeps the
            // pane visible but returns focus to the list).
            Key::Char('q') | Key::Esc => self.focus = Focus::List,
            Key::Tab if self.is_split() => self.focus = Focus::List,
            _ => {}
        }
    }
}
