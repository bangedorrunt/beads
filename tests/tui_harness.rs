// governed-by: ADR-0003
//! Headless TUI focus-model harness (ADR-0003 §5 proof 4, bead gu7ts.6).
//!
//! Drives `TuiApp::handle_key` directly — no pseudo-TTY needed — and pins
//! the skeleton's layer contract: split threshold, detail open/close,
//! `q`/`esc` layering with quit-confirm at the top list, ctrl+c always quits.

use beads::model::Issue;
use beads::tui::{Focus, Key, TuiApp};

fn fixture_issues() -> Vec<Issue> {
    // Minimal id-sorted set; the state machine only reads ids/status for
    // rendering, so serde defaults suffice via JSON like the other suites.
    ["fx-a", "fx-b", "fx-c"]
        .iter()
        .map(|id| {
            serde_json::from_str(&format!(
                r#"{{"id": "{id}", "title": "issue {id}", "status": "open",
                    "priority": 2, "issue_type": "task",
                    "created_at": "2026-08-01T10:00:00Z",
                    "updated_at": "2026-08-01T10:00:00Z"}}"#
            ))
            .expect("test issue parses")
        })
        .collect()
}

fn app() -> TuiApp {
    TuiApp::new(fixture_issues())
}

#[test]
fn selection_navigation_respects_bounds() {
    let mut app = app();
    assert_eq!(app.selected(), 0);
    app.handle_key(Key::Down);
    assert_eq!(app.selected(), 1);
    app.handle_key(Key::Down);
    app.handle_key(Key::Down);
    assert_eq!(app.selected(), 2, "clamped at last issue");
    app.handle_key(Key::Home);
    assert_eq!(app.selected(), 0);
    app.handle_key(Key::End);
    assert_eq!(app.selected(), 2);
    app.handle_key(Key::Up);
    assert_eq!(app.selected(), 1);
}

#[test]
fn enter_opens_detail_and_q_esc_returns_to_list() {
    let mut app = app();
    assert_eq!(app.focus(), Focus::List);

    app.handle_key(Key::Enter);
    assert_eq!(app.focus(), Focus::Detail);
    app.handle_key(Key::Char('q'));
    assert_eq!(app.focus(), Focus::List);

    app.handle_key(Key::Enter);
    assert_eq!(app.focus(), Focus::Detail);
    app.handle_key(Key::Esc);
    assert_eq!(app.focus(), Focus::List);
}

#[test]
fn tab_moves_between_panes_only_in_split_view() {
    let mut narrow = app();
    narrow.set_size(80, 24);
    assert!(!narrow.is_split(), "<=100 columns is mobile/single-column");
    narrow.handle_key(Key::Enter);
    assert_eq!(narrow.focus(), Focus::Detail);
    narrow.handle_key(Key::Tab);
    assert_eq!(
        narrow.focus(),
        Focus::Detail,
        "tab must not move focus in single-column mode"
    );

    let mut wide = app();
    wide.set_size(120, 40);
    assert!(wide.is_split(), ">100 columns auto-splits");
    wide.handle_key(Key::Tab);
    assert_eq!(
        wide.focus(),
        Focus::Detail,
        "tab toggles to the detail pane"
    );
    wide.handle_key(Key::Tab);
    assert_eq!(
        wide.focus(),
        Focus::List,
        "tab toggles back to the list pane"
    );
}

#[test]
fn q_at_top_list_quits_without_confirm() {
    let mut app = app();
    app.handle_key(Key::Char('q'));
    assert!(app.should_quit(), "q at the top list quits outright");
}

#[test]
fn esc_at_top_list_asks_quit_confirm_first() {
    let mut app = app();
    app.handle_key(Key::Esc);
    assert_eq!(app.focus(), Focus::QuitConfirm);
    assert!(!app.should_quit(), "esc alone must not quit");

    app.handle_key(Key::Char('y'));
    assert!(app.should_quit(), "y confirms quit");
}

#[test]
fn any_other_key_cancels_quit_confirm() {
    let mut app = app();
    app.handle_key(Key::Esc);
    assert_eq!(app.focus(), Focus::QuitConfirm);
    app.handle_key(Key::Esc);
    assert_eq!(app.focus(), Focus::List, "second esc cancels the confirm");
    assert!(!app.should_quit());
}

#[test]
fn status_message_clears_on_next_keypress() {
    let mut app = app();
    app.handle_key(Key::Esc);
    app.handle_key(Key::Esc); // cancel -> sets "quit cancelled"
    assert!(app.status_message().is_some());
    app.handle_key(Key::Up);
    assert!(
        app.status_message().is_none(),
        "any keypress clears the footer status message"
    );
}

#[test]
fn ctrl_c_quits_from_any_layer() {
    for start in [Focus::List, Focus::Detail] {
        let mut app = app();
        if start == Focus::Detail {
            app.handle_key(Key::Enter);
        }
        app.handle_key(Key::CtrlC);
        assert!(app.should_quit(), "ctrl+c quits from {start:?}");
    }

    let mut app = app();
    app.handle_key(Key::Esc);
    app.handle_key(Key::CtrlC);
    assert!(app.should_quit(), "ctrl+c quits from the quit confirm");
}
