//! Comprehensive tests for the blocked_issues_cache correctness.
//!
//! Covers: cache rebuild triggers, ready/blocked consistency, reopen semantics,
//! chain dependencies, mixed dependency types, delete/tombstone cleanup,
//! remove-all-deps, parent-child transitive blocking, and cross-check of
//! `get_ready_issues` vs `get_blocked_ids`.
//!
//! Related bead: beads-1kaf

#![allow(clippy::similar_names)]

mod common;

use beads::model::{DependencyType, Status};
use beads::storage::{IssueUpdate, ReadyFilters, ReadySortPolicy, SqliteStorage};
use common::{fixtures, test_db};

fn blocked_ids(storage: &SqliteStorage) -> Vec<String> {
    storage
        .get_blocked_issues()
        .unwrap()
        .into_iter()
        .map(|(issue, _)| issue.id)
        .collect()
}

fn ready_ids(storage: &SqliteStorage) -> Vec<String> {
    storage
        .get_ready_issues(&ReadyFilters::default(), ReadySortPolicy::Priority)
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect()
}

fn status_update(status: Status) -> IssueUpdate {
    IssueUpdate {
        status: Some(status),
        ..Default::default()
    }
}

#[test]
fn reopen_blocker_re_blocks_dependent() {
    let mut storage = test_db();

    let blocker = fixtures::issue("reopen-blocker");
    let dependent = fixtures::issue("reopen-dependent");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&dependent, "tester").unwrap();

    storage
        .add_dependency(
            &dependent.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    assert!(
        blocked_ids(&storage).contains(&dependent.id),
        "dependent should be blocked initially"
    );

    storage
        .update_issue(&blocker.id, &status_update(Status::Closed), "tester")
        .unwrap();
    assert!(
        !blocked_ids(&storage).contains(&dependent.id),
        "dependent should be unblocked after closing blocker"
    );

    storage
        .update_issue(&blocker.id, &status_update(Status::Open), "tester")
        .unwrap();
    assert!(
        blocked_ids(&storage).contains(&dependent.id),
        "dependent should be re-blocked after reopening blocker"
    );
}

#[test]
fn chain_dependency_blocks_downstream() {
    let mut storage = test_db();

    let a = fixtures::issue("chain-A");
    let b = fixtures::issue("chain-B");
    let c = fixtures::issue("chain-C");

    storage.create_issue(&a, "tester").unwrap();
    storage.create_issue(&b, "tester").unwrap();
    storage.create_issue(&c, "tester").unwrap();

    storage
        .add_dependency(&b.id, &a.id, DependencyType::Blocks.as_str(), "tester")
        .unwrap();

    storage
        .add_dependency(&c.id, &b.id, DependencyType::Blocks.as_str(), "tester")
        .unwrap();

    let ids = blocked_ids(&storage);
    assert!(ids.contains(&b.id), "B should be blocked by A");
    assert!(ids.contains(&c.id), "C should be blocked by B");

    storage
        .update_issue(&a.id, &status_update(Status::Closed), "tester")
        .unwrap();

    let ids = blocked_ids(&storage);
    assert!(!ids.contains(&b.id), "B should be unblocked after A closed");
    assert!(ids.contains(&c.id), "C should still be blocked by open B");

    storage
        .update_issue(&b.id, &status_update(Status::Closed), "tester")
        .unwrap();

    let ids = blocked_ids(&storage);
    assert!(!ids.contains(&c.id), "C should be unblocked after B closed");
}

#[test]
fn multiple_blockers_partial_close() {
    let mut storage = test_db();

    let blocker1 = fixtures::issue("multi-blocker-1");
    let blocker2 = fixtures::issue("multi-blocker-2");
    let dependent = fixtures::issue("multi-dependent");

    storage.create_issue(&blocker1, "tester").unwrap();
    storage.create_issue(&blocker2, "tester").unwrap();
    storage.create_issue(&dependent, "tester").unwrap();

    storage
        .add_dependency(
            &dependent.id,
            &blocker1.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();
    storage
        .add_dependency(
            &dependent.id,
            &blocker2.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    assert!(blocked_ids(&storage).contains(&dependent.id));

    storage
        .update_issue(&blocker1.id, &status_update(Status::Closed), "tester")
        .unwrap();
    assert!(
        blocked_ids(&storage).contains(&dependent.id),
        "still blocked by blocker2"
    );

    storage
        .update_issue(&blocker2.id, &status_update(Status::Closed), "tester")
        .unwrap();
    assert!(
        !blocked_ids(&storage).contains(&dependent.id),
        "unblocked after all blockers closed"
    );
}

#[test]
fn mixed_dependency_types_all_block() {
    let mut storage = test_db();

    let b1 = fixtures::issue("mix-blocks-src");
    let b2 = fixtures::issue("mix-cond-src");
    let b3 = fixtures::issue("mix-waits-src");
    let target = fixtures::issue("mix-target");

    storage.create_issue(&b1, "tester").unwrap();
    storage.create_issue(&b2, "tester").unwrap();
    storage.create_issue(&b3, "tester").unwrap();
    storage.create_issue(&target, "tester").unwrap();

    storage
        .add_dependency(&target.id, &b1.id, "blocks", "tester")
        .unwrap();
    storage
        .add_dependency(&target.id, &b2.id, "conditional-blocks", "tester")
        .unwrap();
    storage
        .add_dependency(&target.id, &b3.id, "waits-for", "tester")
        .unwrap();

    let ids = blocked_ids(&storage);
    assert!(
        ids.contains(&target.id),
        "target should be blocked by all three dependency types"
    );

    for blocker_id in [&b1.id, &b2.id, &b3.id] {
        storage
            .update_issue(blocker_id, &status_update(Status::Closed), "tester")
            .unwrap();
    }

    assert!(
        !blocked_ids(&storage).contains(&target.id),
        "target unblocked after all mixed deps closed"
    );
}

#[test]
fn closing_blocked_issue_removes_from_cache() {
    let mut storage = test_db();

    let blocker = fixtures::issue("close-self-blocker");
    let blocked_issue = fixtures::issue("close-self-blocked");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&blocked_issue, "tester").unwrap();

    storage
        .add_dependency(
            &blocked_issue.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    assert!(blocked_ids(&storage).contains(&blocked_issue.id));

    storage
        .update_issue(&blocked_issue.id, &status_update(Status::Closed), "tester")
        .unwrap();

    let ids = blocked_ids(&storage);
    assert!(
        !ids.contains(&blocked_issue.id),
        "closed issue should not appear in blocked cache"
    );
}

#[test]
fn delete_issue_cleans_cache() {
    let mut storage = test_db();

    let blocker = fixtures::issue("del-blocker");
    let blocked_issue = fixtures::issue("del-blocked");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&blocked_issue, "tester").unwrap();

    storage
        .add_dependency(
            &blocked_issue.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    assert!(blocked_ids(&storage).contains(&blocked_issue.id));

    storage
        .delete_issue(&blocker.id, "tester", "test cleanup", None)
        .unwrap();

    let ids = blocked_ids(&storage);
    assert!(
        !ids.contains(&blocked_issue.id),
        "deleting blocker should unblock dependent"
    );
}

#[test]
fn remove_all_dependencies_clears_cache() {
    let mut storage = test_db();

    let b1 = fixtures::issue("rm-all-b1");
    let b2 = fixtures::issue("rm-all-b2");
    let target = fixtures::issue("rm-all-target");

    storage.create_issue(&b1, "tester").unwrap();
    storage.create_issue(&b2, "tester").unwrap();
    storage.create_issue(&target, "tester").unwrap();

    storage
        .add_dependency(
            &target.id,
            &b1.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();
    storage
        .add_dependency(
            &target.id,
            &b2.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    assert!(blocked_ids(&storage).contains(&target.id));

    storage
        .remove_all_dependencies(&target.id, "tester")
        .unwrap();

    assert!(
        !blocked_ids(&storage).contains(&target.id),
        "removing all deps should unblock issue"
    );
}

#[test]
fn parent_child_transitive_blocking() {
    let mut storage = test_db();

    let blocker = fixtures::issue("pc-blocker");
    let parent = fixtures::issue("pc-parent");
    let child = fixtures::issue("pc-child");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&parent, "tester").unwrap();
    storage.create_issue(&child, "tester").unwrap();

    storage
        .add_dependency(
            &parent.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    storage
        .add_dependency(&child.id, &parent.id, "parent-child", "tester")
        .unwrap();

    let ids = blocked_ids(&storage);
    assert!(ids.contains(&parent.id), "parent blocked by blocker");
    assert!(
        ids.contains(&child.id),
        "child transitively blocked via parent"
    );

    storage
        .update_issue(&blocker.id, &status_update(Status::Closed), "tester")
        .unwrap();

    let ids = blocked_ids(&storage);
    assert!(!ids.contains(&parent.id), "parent unblocked");
    assert!(!ids.contains(&child.id), "child transitively unblocked");
}

#[test]
fn remove_parent_unblocks_child() {
    let mut storage = test_db();

    let blocker = fixtures::issue("rp-blocker");
    let parent = fixtures::issue("rp-parent");
    let child = fixtures::issue("rp-child");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&parent, "tester").unwrap();
    storage.create_issue(&child, "tester").unwrap();

    storage
        .add_dependency(
            &parent.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();
    storage
        .add_dependency(&child.id, &parent.id, "parent-child", "tester")
        .unwrap();

    assert!(
        blocked_ids(&storage).contains(&child.id),
        "child blocked via parent"
    );

    storage
        .remove_dependency(&child.id, &parent.id, "tester")
        .unwrap();

    assert!(
        !blocked_ids(&storage).contains(&child.id),
        "child should be unblocked after parent link removed"
    );
}

#[test]
fn ready_and_blocked_are_disjoint() {
    let mut storage = test_db();

    let blocker = fixtures::issue("disjoint-blocker");
    let blocked_issue = fixtures::issue("disjoint-blocked");
    let free_issue = fixtures::issue("disjoint-free");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&blocked_issue, "tester").unwrap();
    storage.create_issue(&free_issue, "tester").unwrap();

    storage
        .add_dependency(
            &blocked_issue.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    let ready = ready_ids(&storage);
    let blocked = blocked_ids(&storage);

    for id in &blocked {
        assert!(
            !ready.contains(id),
            "blocked issue {id} should not appear in ready list"
        );
    }

    assert!(ready.contains(&free_issue.id), "free issue should be ready");

    assert!(blocked.contains(&blocked_issue.id));
    assert!(!ready.contains(&blocked_issue.id));
}

#[test]
fn is_blocked_matches_get_blocked_ids() {
    let mut storage = test_db();

    let blocker = fixtures::issue("ib-blocker");
    let blocked_issue = fixtures::issue("ib-blocked");
    let free_issue = fixtures::issue("ib-free");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&blocked_issue, "tester").unwrap();
    storage.create_issue(&free_issue, "tester").unwrap();

    storage
        .add_dependency(
            &blocked_issue.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    let blocked_set = storage.get_blocked_ids().unwrap();

    assert_eq!(
        storage.is_blocked(&blocked_issue.id).unwrap(),
        blocked_set.contains(&blocked_issue.id),
        "is_blocked should match for blocked issue"
    );
    assert_eq!(
        storage.is_blocked(&free_issue.id).unwrap(),
        blocked_set.contains(&free_issue.id),
        "is_blocked should match for free issue"
    );
    assert_eq!(
        storage.is_blocked(&blocker.id).unwrap(),
        blocked_set.contains(&blocker.id),
        "is_blocked should match for blocker (not blocked itself)"
    );
}

#[test]
fn blocked_cache_stores_blocker_metadata() {
    let mut storage = test_db();

    let blocker = fixtures::issue("meta-blocker");
    let blocked_issue = fixtures::issue("meta-blocked");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&blocked_issue, "tester").unwrap();

    storage
        .add_dependency(
            &blocked_issue.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    let blocked_issues = storage.get_blocked_issues().unwrap();
    let entry = blocked_issues
        .iter()
        .find(|(issue, _)| issue.id == blocked_issue.id)
        .expect("blocked entry should exist");

    assert!(
        entry.1.iter().any(|r| r.contains(&blocker.id)),
        "blocker references should contain blocker ID, got: {:?}",
        entry.1
    );
}

#[test]
fn in_progress_blocker_still_blocks() {
    let mut storage = test_db();

    let blocker = fixtures::issue("ip-blocker");
    let dependent = fixtures::issue("ip-dependent");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&dependent, "tester").unwrap();

    storage
        .add_dependency(
            &dependent.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    storage
        .update_issue(&blocker.id, &status_update(Status::InProgress), "tester")
        .unwrap();

    assert!(
        blocked_ids(&storage).contains(&dependent.id),
        "in_progress blocker should still block dependent"
    );
}

#[test]
fn force_rebuild_matches_auto_rebuild() {
    let mut storage = test_db();

    let blocker = fixtures::issue("force-blocker");
    let blocked_issue = fixtures::issue("force-blocked");
    let free_issue = fixtures::issue("force-free");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&blocked_issue, "tester").unwrap();
    storage.create_issue(&free_issue, "tester").unwrap();

    storage
        .add_dependency(
            &blocked_issue.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    let auto_blocked = storage.get_blocked_ids().unwrap();

    storage.rebuild_blocked_cache(true).unwrap();
    let force_blocked = storage.get_blocked_ids().unwrap();

    assert_eq!(
        auto_blocked, force_blocked,
        "force rebuild should produce identical result to auto-rebuild"
    );
}

#[test]
fn force_rebuild_with_direct_blocker_dependency_succeeds() {
    let mut storage = test_db();

    let blocker = fixtures::issue("ok70-force-blocker");
    let blocked_issue = fixtures::issue("ok70-force-blocked");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&blocked_issue, "tester").unwrap();

    storage
        .add_dependency(
            &blocked_issue.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    let rebuilt = storage.rebuild_blocked_cache(true).unwrap();
    assert_eq!(rebuilt, 1, "force rebuild should persist one blocked issue");
    assert!(
        blocked_ids(&storage).contains(&blocked_issue.id),
        "force rebuild should keep the direct blocker in cache"
    );
}

#[test]
fn deep_parent_child_chain_blocking() {
    let mut storage = test_db();

    let blocker = fixtures::issue("deep-blocker");
    let p1 = fixtures::issue("deep-parent-1");
    let p2 = fixtures::issue("deep-parent-2");
    let p3 = fixtures::issue("deep-parent-3");
    let leaf = fixtures::issue("deep-leaf");

    storage.create_issue(&blocker, "tester").unwrap();
    storage.create_issue(&p1, "tester").unwrap();
    storage.create_issue(&p2, "tester").unwrap();
    storage.create_issue(&p3, "tester").unwrap();
    storage.create_issue(&leaf, "tester").unwrap();

    storage
        .add_dependency(
            &p1.id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    storage
        .add_dependency(&p2.id, &p1.id, "parent-child", "tester")
        .unwrap();
    storage
        .add_dependency(&p3.id, &p2.id, "parent-child", "tester")
        .unwrap();
    storage
        .add_dependency(&leaf.id, &p3.id, "parent-child", "tester")
        .unwrap();

    let ids = blocked_ids(&storage);
    assert!(ids.contains(&p1.id), "p1 directly blocked");
    assert!(ids.contains(&p2.id), "p2 transitively blocked");
    assert!(ids.contains(&p3.id), "p3 transitively blocked");
    assert!(ids.contains(&leaf.id), "leaf transitively blocked");

    storage
        .update_issue(&blocker.id, &status_update(Status::Closed), "tester")
        .unwrap();

    let ids = blocked_ids(&storage);
    assert!(!ids.contains(&p1.id), "p1 unblocked");
    assert!(!ids.contains(&p2.id), "p2 unblocked");
    assert!(!ids.contains(&p3.id), "p3 unblocked");
    assert!(!ids.contains(&leaf.id), "leaf unblocked");
}

/// Before the fix for issue #154, transitive blocked-cache propagation was
/// capped at `MAX_DEPTH = 50`, causing descendants beyond that depth to
/// silently appear as "ready" even though an ancestor was blocked.
///
/// This test builds a chain of 75 parent-child levels to verify that
/// propagation reaches every descendant.
#[test]
fn deep_chain_beyond_50_levels_blocks_all_descendants() {
    const CHAIN_LEN: usize = 75;
    let mut storage = test_db();

    let blocker = fixtures::issue("deep75-blocker");
    storage.create_issue(&blocker, "tester").unwrap();

    let mut chain_issues: Vec<beads::model::Issue> = Vec::with_capacity(CHAIN_LEN);
    for i in 0..CHAIN_LEN {
        let issue = fixtures::issue(&format!("deep75-node-{i}"));
        storage.create_issue(&issue, "tester").unwrap();
        chain_issues.push(issue);
    }

    storage
        .add_dependency(
            &chain_issues[0].id,
            &blocker.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();

    for i in 1..CHAIN_LEN {
        storage
            .add_dependency(
                &chain_issues[i].id,
                &chain_issues[i - 1].id,
                "parent-child",
                "tester",
            )
            .unwrap();
    }

    let ids = blocked_ids(&storage);
    for (i, issue) in chain_issues.iter().enumerate() {
        assert!(
            ids.contains(&issue.id),
            "node-{i} (depth {depth}) should be blocked but was not found in blocked list \
             (old MAX_DEPTH=50 bug if depth > 50)",
            depth = i + 1,
        );
    }

    storage
        .update_issue(&blocker.id, &status_update(Status::Closed), "tester")
        .unwrap();

    let ids = blocked_ids(&storage);
    for (i, issue) in chain_issues.iter().enumerate() {
        assert!(
            !ids.contains(&issue.id),
            "node-{i} should be unblocked after closing blocker",
        );
    }
}
