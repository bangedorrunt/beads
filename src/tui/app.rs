//! Headless TUI state machine (bead gu7ts.6). governed-by: ADR-0003.
//!
//! Pure logic: no ratatui, no terminal I/O. `tests/tui_harness.rs` drives
//! [`TuiApp::handle_key`] directly to pin the focus contract (ADR-0003 §5
//! proof 4) without a pseudo-TTY.

use crate::model::Issue;

/// Keyboard focus owner (bv UX map §2.1, skeleton subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// Main issue list; the root layer.
    List,
    /// Detail viewport for the selected issue.
    Detail,
    /// Centered quit confirmation over the top list.
    QuitConfirm,
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
    CtrlC,
    Char(char),
    Other(crossterm::event::KeyCode),
}

/// Application state: selection, focus layer, and quit intent.
#[derive(Debug)]
pub struct TuiApp {
    issues: Vec<Issue>,
    selected: usize,
    focus: Focus,
    width: u16,
    height: u16,
    status_message: Option<String>,
    should_quit: bool,
}

impl TuiApp {
    /// State over `issues` (expected id-sorted) at bv's default 120x40.
    #[must_use]
    pub fn new(issues: Vec<Issue>) -> Self {
        Self {
            issues,
            selected: 0,
            focus: Focus::List,
            width: 120,
            height: 40,
            status_message: None,
            should_quit: false,
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

    #[must_use]
    pub fn selected_issue(&self) -> Option<&Issue> {
        self.issues.get(self.selected)
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

    /// Apply one key event. This is the whole focus contract.
    pub fn handle_key(&mut self, key: Key) {
        // Status messages clear on any keypress (bv footer rule §1.6).
        self.status_message = None;

        if key == Key::CtrlC {
            self.should_quit = true;
            return;
        }

        match self.focus {
            Focus::QuitConfirm => {
                if key == Key::Char('y') {
                    self.should_quit = true;
                } else {
                    self.focus = Focus::List;
                    self.set_status("quit cancelled");
                }
            }
            Focus::List => self.handle_list_key(key),
            Focus::Detail => self.handle_detail_key(key),
        }
    }

    fn handle_list_key(&mut self, key: Key) {
        match key {
            Key::Down => {
                if self.selected + 1 < self.issues.len() {
                    self.selected += 1;
                }
            }
            Key::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            Key::Home => self.selected = 0,
            Key::End => self.selected = self.issues.len().saturating_sub(1),
            Key::Enter => self.focus = Focus::Detail,
            Key::Tab if self.is_split() => self.focus = Focus::Detail,
            // Top of the layer stack: q quits outright, esc asks first.
            Key::Char('q') => self.should_quit = true,
            Key::Esc => self.focus = Focus::QuitConfirm,
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
