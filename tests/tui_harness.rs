// governed-by: ADR-0003
//! Headless TUI focus-model harness (ADR-0003 §5 proof 4, bead gu7ts.6).
//!
//! Drives `TuiApp::handle_key` directly — no pseudo-TTY needed — and pins
//! the skeleton's layer contract: split threshold, detail open/close,
//! `q`/`esc` layering with quit-confirm at the top list, ctrl+c always quits.

use beads::model::{Dependency, Issue, Priority, Status};
use beads::tui::actions::{ReloadSelection, TuiCmd, TuiMsg};
use beads::tui::{Focus, Key, TuiApp};

fn fixture_issues() -> Vec<Issue> {
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

fn parent_edge(child: &str, parent: &str) -> Dependency {
    serde_json::from_value(serde_json::json!({
        "issue_id": child,
        "depends_on_id": parent,
        "type": "parent-child",
        "created_at": "2026-08-01T10:00:00Z"
    }))
    .expect("parent-child edge parses")
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
    app.handle_key(Key::Esc);
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
        let selection = match open_key {
            Key::Char('b') => app.board_selection(),
            Key::Char('a') => app.actionable_selection(),
            Key::Char('E') => app.tree_selection(),
            Key::Char('[') => app.label_selection(),
            _ => app.selected(),
        };
        assert!(selection <= 2, "End clamps {open_key:?} to a real row");
    }

    let mut issues = fixture_issues();
    let mut issue = issues.remove(0);
    issue.description = Some("word ".repeat(400));
    let mut app = TuiApp::new(vec![issue]);
    app.set_size(80, 8);
    app.handle_key(Key::Enter);
    app.handle_key(Key::End);
    let bottom = app.detail_scroll();
    assert!(bottom > 0, "long detail has a real bottom");
    app.handle_key(Key::PageDown);
    assert_eq!(app.detail_scroll(), bottom, "PageDown clamps at bottom");
    app.handle_key(Key::Char('g'));
    assert_eq!(app.detail_scroll(), 0);
}

#[test]
fn analysis_is_built_on_first_detail_open() {
    let mut app = app();
    assert!(app.analysis().is_none());
    app.handle_key(Key::Enter);
    assert!(app.analysis().is_some());
}

#[test]
fn responsive_layout_threshold_is_width_based() {
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
fn close_prompt_returns_a_command_and_leaves_the_row_open() {
    let mut app = app();
    assert!(app.handle_key(Key::Char('c')).is_empty());
    assert_eq!(app.focus(), Focus::Prompt);
    for character in "abc1234".chars() {
        app.handle_key(Key::Char(character));
    }
    app.handle_key(Key::Tab);
    for character in "done".chars() {
        app.handle_key(Key::Char(character));
    }
    let commands = app.handle_key(Key::Enter);
    assert_eq!(commands.len(), 1);
    let TuiCmd::Close {
        issue_id,
        commit_sha,
        reason,
        expected_revision,
    } = &commands[0]
    else {
        panic!("expected a close command");
    };
    assert_eq!(issue_id, "fx-a");
    assert_eq!(commit_sha, "abc1234");
    assert_eq!(reason, "done");
    assert!(expected_revision.is_some(), "the TUI pins the observed row");
    assert_eq!(
        app.selected_issue().expect("selection").status.as_str(),
        "open",
        "a keystroke must not mutate the ledger"
    );
    assert_eq!(app.focus(), Focus::List);
}

#[test]
fn delete_prompt_refuses_a_blank_reason() {
    let mut app = app();
    app.handle_key(Key::Char('d'));
    let commands = app.handle_key(Key::Enter);
    assert!(commands.is_empty());
    assert_eq!(app.focus(), Focus::Prompt, "blank reason keeps the prompt");
    assert!(app.status_is_error());

    for character in "duplicate".chars() {
        app.handle_key(Key::Char(character));
    }
    let commands = app.handle_key(Key::Enter);
    assert!(
        matches!(&commands[0], TuiCmd::Delete { reason, .. } if reason == "duplicate"),
        "got {commands:?}"
    );
}

#[test]
fn edit_prompt_prefills_the_title() {
    let mut app = app();
    app.handle_key(Key::Char('e'));
    let commands = app.handle_key(Key::Enter);
    assert!(
        matches!(&commands[0], TuiCmd::EditTitle { issue_id, title, .. } if issue_id == "fx-a" && title == "issue fx-a"),
        "got {commands:?}"
    );
}

#[test]
fn ctrl_r_returns_a_reload_command() {
    let mut app = app();
    assert_eq!(app.handle_key(Key::CtrlR), vec![TuiCmd::Reload]);
}

#[test]
fn reloaded_message_preserves_selection_by_id() {
    let mut app = app();
    app.handle_key(Key::Down);
    assert_eq!(app.selected_issue().expect("selection").id, "fx-b");

    let issues: Vec<Issue> = fixture_issues().into_iter().rev().collect();
    app.update(TuiMsg::Reloaded {
        issues,
        now: chrono::Utc::now(),
        status: "reloaded".to_string(),
        selection: ReloadSelection::KeepById,
    });

    assert_eq!(
        app.selected_issue().expect("selection").id,
        "fx-b",
        "a reload keeps the selected bead selected"
    );
    assert_eq!(app.status_message(), Some("reloaded"));
    assert!(!app.status_is_error());
}

#[test]
fn close_reload_advances_to_the_next_row() {
    let mut app = app();
    assert_eq!(app.selected_issue().expect("selection").id, "fx-a");

    let mut issues = fixture_issues();
    issues[0].status = Status::Closed;
    app.update(TuiMsg::Reloaded {
        issues,
        now: chrono::Utc::now(),
        status: "closed fx-a".to_string(),
        selection: ReloadSelection::KeepIndex,
    });

    assert_eq!(
        app.selected_issue().expect("selection").id,
        "fx-b",
        "the cursor moves off the row that just closed"
    );
}

#[test]
fn failed_message_is_an_error_status() {
    let mut app = app();
    app.update(TuiMsg::Failed {
        message: "policy refused".to_string(),
    });
    assert!(app.status_is_error());
    assert!(
        app.status_message()
            .is_some_and(|message| message.contains("policy refused"))
    );
}

#[test]
fn close_prompt_refuses_a_terminal_issue() {
    let mut issues = fixture_issues();
    issues[0].status = Status::Closed;
    let mut app = TuiApp::new(issues);
    app.handle_key(Key::End);
    assert_eq!(app.selected_issue().expect("selection").id, "fx-a");

    app.handle_key(Key::Char('c'));
    assert_eq!(app.focus(), Focus::List, "no modal opens for a closed row");
    assert!(app.status_is_error());
}

#[test]
fn board_keeps_every_status_column() {
    let mut issues = fixture_issues();
    issues[0].status = Status::Deferred;
    issues[1].status = Status::Draft;
    let app = TuiApp::new(issues);

    let columns = app.board_columns();
    let statuses: Vec<&str> = columns
        .iter()
        .map(|column| column.status.as_str())
        .collect();
    assert!(statuses.contains(&"deferred"));
    assert!(statuses.contains(&"draft"));
    assert_eq!(app.board_rows().len(), 3, "no bead falls off the board");
}

#[test]
fn actionable_view_opens_the_row_it_renders() {
    let mut issues = fixture_issues();
    issues[0].priority = Priority(2);
    issues[1].priority = Priority(0);
    let mut app = TuiApp::new(issues);

    app.handle_key(Key::Char('a'));
    app.handle_key(Key::Down);
    assert_eq!(app.actionable_selection(), 1);
    app.handle_key(Key::Enter);

    assert_eq!(app.focus(), Focus::Detail);
    assert_eq!(
        app.selected_issue().expect("selection").id,
        "fx-a",
        "enter opens the card the actionable view highlights"
    );
}

#[test]
fn tree_rows_include_grandchildren() {
    let mut issues = fixture_issues();
    issues[1].dependencies.push(parent_edge("fx-b", "fx-a"));
    issues[2].dependencies.push(parent_edge("fx-c", "fx-b"));
    let app = TuiApp::new(issues);

    let shape: Vec<(&str, usize)> = app
        .tree_rows()
        .iter()
        .map(|row| (app.issues()[row.issue_index].id.as_str(), row.depth))
        .collect();
    assert_eq!(shape, vec![("fx-a", 0), ("fx-b", 1), ("fx-c", 2)]);
}

#[test]
fn label_dashboard_filters_by_the_selected_label() {
    let mut issues = fixture_issues();
    issues[0].labels = vec!["core".to_string()];
    issues[1].labels = vec!["core".to_string(), "ui".to_string()];
    issues[2].labels = vec!["ui".to_string()];
    let mut app = TuiApp::new(issues);

    app.handle_key(Key::Char('['));
    app.handle_key(Key::Enter);

    assert_eq!(app.focus(), Focus::List);
    assert_eq!(app.active_filter_query(), Some("label:core"));
    assert_eq!(app.visible_count(), 2);
}

#[test]
fn label_filter_matches_the_whole_label() {
    let mut issues = fixture_issues();
    issues[0].labels = vec!["ui".to_string()];
    issues[1].labels = vec!["build".to_string()];
    issues[2].labels = vec!["ui".to_string(), "build".to_string()];
    let mut app = TuiApp::new(issues);

    app.handle_key(Key::Char('['));
    app.handle_key(Key::Down);
    app.handle_key(Key::Enter);

    assert_eq!(app.active_filter_query(), Some("label:ui"));
    assert_eq!(app.visible_count(), 2, "\"build\" is not the \"ui\" label");
}

#[test]
fn list_keeps_open_and_in_progress_visible() {
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
