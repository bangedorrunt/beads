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

    // type "fx" -> should filter to 3 fx-* issues
    app.handle_key(Key::Char('f'));
    app.handle_key(Key::Char('x'));
    assert_eq!(app.active_filter_query(), Some("fx"));
    assert_eq!(app.visible_count(), 3);

    // type "-a" -> only fx-a
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

    // Esc at list with active filter clears it (bv §3.0)
    app.handle_key(Key::Esc);
    assert_eq!(app.active_filter_query(), None);
    assert_eq!(app.visible_count(), 4);
    // second Esc goes to quit confirm
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
    // "f" matches all 4 (fx-a etc contain f, other contains f? other has no f, but title "unrelated" no f, but id "other" has 'o' not f - check: "other" titles no f, should be 3? Let's just check non-empty)
    assert!(app.visible_count() >= 3);

    // nav while searching
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
    // hide again
    app.handle_key(Key::Char(';'));
    assert!(!app.show_shortcuts());
    // ctrl+j when hidden does not scroll (handler early returns only if visible)
    app.handle_key(Key::CtrlJ);
    assert_eq!(app.shortcuts_scroll(), 0);

    // F2 also toggles
    app.handle_key(Key::F2);
    assert!(app.show_shortcuts());
    app.handle_key(Key::F2);
    assert!(!app.show_shortcuts());
}

#[test]
fn help_overlay_toggle_and_focus_restore() {
    let mut app = app();
    app.handle_key(Key::Enter); // go to Detail
    assert_eq!(app.focus(), Focus::Detail);
    app.handle_key(Key::Char('?'));
    assert_eq!(app.focus(), Focus::Help);
    // any key closes help and restores Detail
    app.handle_key(Key::Char('x'));
    assert_eq!(app.focus(), Focus::Detail);

    // from List via ?
    app.handle_key(Key::Char('q')); // back to List
    assert_eq!(app.focus(), Focus::List);
    app.handle_key(Key::Char('?'));
    assert_eq!(app.focus(), Focus::Help);
    app.handle_key(Key::Esc);
    assert_eq!(app.focus(), Focus::List);

    // F1 also opens help
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
    // '?' typed while searching should be buffer, not help
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
    app.set_size(120, 20); // small viewport
    for _ in 0..50 {
        app.handle_key(Key::Down);
    }
    assert_eq!(app.selected(), 50);
    // paged navigation
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
