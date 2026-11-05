// governed-by: ADR-0003
//! Headless extras harness for gu7ts.8 (search/filter, sidebar, help, registry).
//! Driven via TuiApp::handle_key headless, per ADR-0003 §5 proof 4.

use beads::model::Issue;
use beads::tui::{Focus, Key, TuiApp, keys};

fn fixture_issues() -> Vec<Issue> {
    ["fx-a", "fx-b", "fx-c", "other"]
        .iter()
        .map(|id| {
            let title = if id.starts_with("fx-") {
                format!("issue {id} dashboard")
            } else {
                format!("issue {id} unrelated")
            };
            serde_json::from_str(&format!(
                r#"{{"id": "{id}", "title": "{title}", "status": "open",
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
fn slash_enters_search_and_typing_filters() {
    let mut app = app();
    assert_eq!(app.focus(), Focus::List);
    assert_eq!(app.visible_count(), 4);

    app.handle_key(Key::Char('/'));
    assert_eq!(app.focus(), Focus::Search);
    assert!(app.is_searching());

    app.handle_key(Key::Char('f'));
    app.handle_key(Key::Char('x'));
    assert_eq!(app.active_filter_query(), Some("fx"));
    assert_eq!(app.visible_count(), 3);

    app.handle_key(Key::Char('-'));
    app.handle_key(Key::Char('a'));
    assert_eq!(app.visible_count(), 1);
    assert_eq!(app.selected_issue().unwrap().id, "fx-a");
}

#[test]
fn search_enter_commits_and_esc_clears() {
    let mut app = app();
    app.handle_key(Key::Char('/'));
    app.handle_key(Key::Char('f'));
    app.handle_key(Key::Char('x'));
    app.handle_key(Key::Enter);
    assert_eq!(app.focus(), Focus::List);
    assert_eq!(app.active_filter_query(), Some("fx"));
    assert_eq!(app.visible_count(), 3);

    app.handle_key(Key::Esc);
    assert_eq!(app.active_filter_query(), None);
    assert_eq!(app.visible_count(), 4);
    app.handle_key(Key::Esc);
    assert_eq!(app.focus(), Focus::QuitConfirm);
}

#[test]
fn search_esc_cancels_without_applying() {
    let mut app = app();
    app.handle_key(Key::Char('/'));
    app.handle_key(Key::Char('f'));
    app.handle_key(Key::Esc);
    assert_eq!(app.focus(), Focus::List);
    assert_eq!(app.active_filter_query(), None);
    assert_eq!(app.visible_count(), 4);
}

#[test]
fn search_backspace_and_navigation() {
    let mut app = app();
    app.handle_key(Key::Char('/'));
    app.handle_key(Key::Char('f'));
    app.handle_key(Key::Char('x'));
    assert_eq!(app.visible_count(), 3);
    app.handle_key(Key::Backspace);
    assert_eq!(app.active_filter_query(), Some("f"));
    assert!(app.visible_count() >= 3);

    app.handle_key(Key::Down);
    assert_eq!(app.selected(), 1);
    app.handle_key(Key::Up);
    assert_eq!(app.selected(), 0);
}

#[test]
fn shortcuts_sidebar_toggle_and_scroll() {
    let mut app = app();
    assert!(!app.show_shortcuts());
    app.handle_key(Key::Char(';'));
    assert!(app.show_shortcuts());
    assert_eq!(app.shortcuts_scroll(), 0);
    app.handle_key(Key::CtrlJ);
    assert_eq!(app.shortcuts_scroll(), 1);
    app.handle_key(Key::CtrlK);
    assert_eq!(app.shortcuts_scroll(), 0);
    app.handle_key(Key::Char(';'));
    assert!(!app.show_shortcuts());
    app.handle_key(Key::CtrlJ);
    assert_eq!(app.shortcuts_scroll(), 0);

    app.handle_key(Key::F2);
    assert!(app.show_shortcuts());
    app.handle_key(Key::F2);
    assert!(!app.show_shortcuts());
}

#[test]
fn help_overlay_toggle_and_focus_restore() {
    let mut app = app();
    app.handle_key(Key::Enter);
    assert_eq!(app.focus(), Focus::Detail);
    app.handle_key(Key::Char('?'));
    assert_eq!(app.focus(), Focus::Help);
    app.handle_key(Key::Char('x'));
    assert_eq!(app.focus(), Focus::Detail);

    app.handle_key(Key::Char('q'));
    assert_eq!(app.focus(), Focus::List);
    app.handle_key(Key::Char('?'));
    assert_eq!(app.focus(), Focus::Help);
    app.handle_key(Key::Esc);
    assert_eq!(app.focus(), Focus::List);

    app.handle_key(Key::F1);
    assert_eq!(app.focus(), Focus::Help);
    app.handle_key(Key::F1);
    assert_eq!(app.focus(), Focus::List);
}

#[test]
fn help_suppressed_while_searching() {
    let mut app = app();
    app.handle_key(Key::Char('/'));
    assert_eq!(app.focus(), Focus::Search);
    app.handle_key(Key::Char('?'));
    assert_eq!(app.focus(), Focus::Search);
    assert_eq!(app.active_filter_query(), Some("?"));
}

#[test]
fn key_registry_has_expected_entries() {
    let reg = keys::registry();
    assert!(!reg.is_empty());
    assert!(
        reg.iter()
            .any(|b| b.key == "j" && b.contexts.contains(&"all"))
    );
    assert!(reg.iter().any(|b| b.key == "/"));
    assert!(reg.iter().any(|b| b.key == "?"));
    assert!(reg.iter().any(|b| b.key == ";"));
    let list_bindings = keys::all_bindings_for_focus("list");
    assert!(list_bindings.iter().any(|b| b.key == "/"));
    let help_bindings = keys::all_bindings_for_focus("help");
    assert!(help_bindings.iter().any(|b| b.key == "q"));
}

#[test]
fn pagination_window_keeps_selection_visible() {
    let mut app = TuiApp::new(
        (0..60)
            .map(|n| {
                serde_json::from_str(&format!(
                    r#"{{"id": "fx-{n:03}", "title": "issue {n}", "status": "open", "priority": 2, "issue_type": "task", "created_at": "2026-08-01T10:00:00Z", "updated_at": "2026-08-01T10:00:00Z"}}"#
                ))
                .unwrap()
            })
            .collect(),
    );
    app.set_size(120, 20);
    for _ in 0..50 {
        app.handle_key(Key::Down);
    }
    assert_eq!(app.selected(), 50);
    app.handle_key(Key::PageDown);
    assert!(app.selected() > 50);
    app.handle_key(Key::PageUp);
    assert!(app.selected() < 60);
    app.handle_key(Key::Home);
    assert_eq!(app.selected(), 0);
    app.handle_key(Key::End);
    assert_eq!(app.selected(), 59);
}

#[test]
fn detail_scroll_and_view_toggles() {
    let mut app = app();
    app.handle_key(Key::Enter);
    assert_eq!(app.focus(), Focus::Detail);
    app.handle_key(Key::Down);
    assert_eq!(app.detail_scroll(), 1);
    app.handle_key(Key::Up);
    assert_eq!(app.detail_scroll(), 0);
    app.handle_key(Key::PageDown);
    assert!(app.detail_scroll() > 0);
    app.handle_key(Key::Char('q'));
    assert_eq!(app.focus(), Focus::List);

    for (key, view) in [
        (Key::Char('b'), Focus::Board),
        (Key::Char('g'), Focus::Graph),
        (Key::Char('a'), Focus::Actionable),
        (Key::Char('i'), Focus::Insights),
        (Key::Char('E'), Focus::Tree),
        (Key::Char('['), Focus::LabelDashboard),
    ] {
        app.handle_key(key);
        assert_eq!(app.focus(), view, "toggle {view:?}");
        app.handle_key(Key::Esc);
        assert_eq!(app.focus(), Focus::List);
        app.handle_key(key);
        assert_eq!(app.focus(), view);
        app.handle_key(Key::Char('q'));
        assert_eq!(app.focus(), Focus::List);
    }
}
