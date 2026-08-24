//! Bare-`br` TUI skeleton (ADR-0003 §3.3, bead gu7ts.6). governed-by: ADR-0003.
//!
//! Layout, focus model, and key contract port the subset of the bv UX map
//! (docs/research/bv-tui-ux-map.md) this bead owns: split list+detail above
//! width 100, single column below, one-line footer status bar, layer-closing
//! `q`/`esc` with quit-confirm at the top list. The state machine lives in
//! [`app`] with no terminal dependency so `tests/tui_harness.rs` can drive
//! it headless (ADR-0003 §5 proof 4).

pub mod app;
pub mod keys;
pub mod theme;
pub mod ui;

use std::io::{self, Stdout};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub use app::{Focus, Key, TuiApp};

/// Run the TUI against the workspace storage. Blocking until quit.
///
/// # Errors
///
/// Returns an error when the terminal cannot be driven or storage fails.
pub fn run() -> crate::error::Result<()> {
    let issues = load_issues()?;
    let mut app = TuiApp::new(issues);
    let backend = CrosstermBackend::new(setup_terminal()?);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| crate::error::BeadsError::Config(format!("TUI backend: {e}")))?;

    loop {
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .map_err(|e| crate::error::BeadsError::Config(format!("TUI draw: {e}")))?;
        if !event::poll(std::time::Duration::from_millis(250))
            .map_err(|e| crate::error::BeadsError::Config(format!("TUI poll: {e}")))?
        {
            continue;
        }
        if let Event::Key(key) =
            event::read().map_err(|e| crate::error::BeadsError::Config(format!("TUI read: {e}")))?
        {
            // Only fire on press; some terminals emit release events too.
            if key.kind == KeyEventKind::Press {
                app.handle_key(to_key(&key));
            }
        }
        if app.should_quit() {
            break;
        }
    }

    restore_terminal()?;
    Ok(())
}

fn to_key(key: &crossterm::event::KeyEvent) -> Key {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => Key::CtrlC,
            KeyCode::Char('d') => Key::CtrlD,
            KeyCode::Char('u') => Key::CtrlU,
            KeyCode::Char('j') => Key::CtrlJ,
            KeyCode::Char('k') => Key::CtrlK,
            KeyCode::Char('r') => Key::Other(KeyCode::Char('r')),
            other => Key::Other(other),
        };
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Key::Down,
        KeyCode::Char('k') | KeyCode::Up => Key::Up,
        KeyCode::Char('g') | KeyCode::Home => Key::Home,
        KeyCode::Char('G') | KeyCode::End => Key::End,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::F(1) => Key::F1,
        KeyCode::F(2) => Key::F2,
        KeyCode::Char('q') => Key::Char('q'),
        KeyCode::Char('y') => Key::Char('y'),
        KeyCode::Char(other) => Key::Char(other),
        other => Key::Other(other),
    }
}

fn setup_terminal() -> io::Result<Stdout> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(stdout)
}

fn restore_terminal() -> crate::error::Result<()> {
    disable_raw_mode()
        .map_err(|e| crate::error::BeadsError::Config(format!("TUI raw mode: {e}")))?;
    execute!(io::stdout(), LeaveAlternateScreen)
        .map_err(|e| crate::error::BeadsError::Config(format!("TUI restore: {e}")))?;
    Ok(())
}

/// Every issue in the workspace sorted by id — the same hydration the robot
/// commands use, so list and detail agree with `br list`/`br show`.
fn load_issues() -> crate::error::Result<Vec<crate::model::Issue>> {
    let Some(beads_dir) = crate::config::discover_optional_beads_dir_with_cli(
        &crate::config::CliOverrides::default(),
    )?
    else {
        return Ok(Vec::new());
    };
    let ctx =
        crate::config::open_storage_with_cli(&beads_dir, &crate::config::CliOverrides::default())?;
    let filters = crate::storage::ListFilters {
        include_closed: true,
        include_deferred: true,
        ..crate::storage::ListFilters::default()
    };
    let listed = ctx.storage.list_issues(&filters)?;
    let ids: Vec<String> = listed.iter().map(|issue| issue.id.clone()).collect();
    let mut issues = if ids.is_empty() {
        Vec::new()
    } else {
        ctx.storage.get_issues_for_export(&ids)?
    };
    issues.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(issues)
}
