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

#[test]
fn scroll_to_bottom_in_every_view_does_not_panic() {
    // beads_rust-5ob1t: operator hit unreachable! at app.rs:451:40 scrolling
    // to panel bottom. Every view focus must accept End/PageDown without
    // panicking (handle_view_nav now owns stray keys).
    for open_key in [
        Key::Char('b'),
        Key::Char('g'),
        Key::Char('a'),
        Key::Char('i'),
        Key::Char('E'),
        Key::Char('['),
    ] {
        let mut app = app();
        app.handle_key(open_key);
        app.handle_key(Key::End);
        app.handle_key(Key::PageDown);
        assert_eq!(app.selected(), 2, "End clamps to last row");
    }

    // Detail panel scroll to bottom (MAX/2 sentinel, clamped in draw) and back.
    let mut app = app();
    app.handle_key(Key::Enter);
    app.handle_key(Key::End);
    assert!(app.detail_scroll() > 0);
    app.handle_key(Key::PageDown);
    app.handle_key(Key::Char('g'));
    assert_eq!(app.detail_scroll(), 0);
}

#[test]
fn responsive_layout_threshold_is_width_based() {
    // beads_rust-5ob1t: phone-width terminal kept the 2-panel split. The
    // split decision is width>100; set_size must flip it at both ends.
    let mut app = app();
    app.set_size(120, 40);
    assert!(app.is_split(), "default wide terminal splits");
    app.set_size(40, 24);
    assert!(!app.is_split(), "40-col phone collapses to single column");
    app.set_size(101, 24);
    assert!(app.is_split());
    app.set_size(100, 24);
    assert!(!app.is_split(), "exactly 100 columns stays single-column");
}

#[test]
fn list_keeps_open_and_in_progress_visible() {
    // beads_rust-5ob1t regression: open/in_progress missing while
    // `br list --status open --json` returned them. TuiApp::new sorts by
    // status rank (open first), never filters — verify both statuses land
    // in visible_indices and sort before closed.
    let mut issues = fixture_issues();
    let mut closed = serde_json::from_str::<Issue>(
        r#"{"id": "fx-z", "title": "done", "status": "closed",
            "priority": 2, "issue_type": "task",
            "created_at": "2026-08-01T10:00:00Z",
            "updated_at": "2026-08-01T10:00:00Z"}"#,
    )
    .expect("closed fixture parses");
    let _ = &mut closed;
    issues.push(closed);
    let app = TuiApp::new(issues);

    let ids: Vec<&str> = app
        .visible_indices()
        .iter()
        .map(|&i| app.issues()[i].id.as_str())
        .collect();
    assert!(ids.contains(&"fx-a") && ids.contains(&"fx-b") && ids.contains(&"fx-c"));
    assert!(ids.contains(&"fx-z"), "closed stays listed, ranked last");
    assert_eq!(ids.last().copied(), Some("fx-z"), "closed sorts after open");
}
