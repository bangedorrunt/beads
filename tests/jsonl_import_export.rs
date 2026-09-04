mod common;

use beads::model::{Comment, DependencyType, Issue, Priority, Status};
use beads::storage::{IssueUpdate, SqliteStorage};
use beads::sync::{
    ExportConfig, ImportConfig, export_gates_to_jsonl, export_to_jsonl, finalize_export,
    import_from_jsonl, import_gates_from_jsonl, read_issues_from_jsonl,
};
use chrono::{Duration, TimeZone, Utc};
use common::fixtures;
use std::fs;
use tempfile::TempDir;

fn issue_with_id(id: &str, title: &str) -> Issue {
    let mut issue = fixtures::issue(title);
    issue.id = id.to_string();
    issue
}

#[test]
fn export_import_roundtrip_preserves_relationships() {
    let mut storage = SqliteStorage::open_memory().unwrap();
    let mut alpha = fixtures::issue("Alpha");
    // Ensure created_at is strictly before any updates (SQLite CURRENT_TIMESTAMP has low precision)
    alpha.created_at = Utc::now() - Duration::hours(1);
    alpha.updated_at = alpha.created_at;
    let mut beta = fixtures::issue("Beta");
    beta.created_at = alpha.created_at;
    beta.updated_at = alpha.created_at;

    alpha.priority = Priority::HIGH;
    alpha.external_ref = Some("ext-1".to_string());
    beta.status = Status::InProgress;

    storage.create_issue(&alpha, "tester").unwrap();
    storage.create_issue(&beta, "tester").unwrap();
    storage
        .add_dependency(
            &beta.id,
            &alpha.id,
            DependencyType::Blocks.as_str(),
            "tester",
        )
        .unwrap();
    storage.add_label(&alpha.id, "alpha", "tester").unwrap();
    storage
        .add_comment(&alpha.id, "tester", "first comment")
        .unwrap();

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    let export = export_to_jsonl(&storage, &path, &ExportConfig::default()).unwrap();
    assert_eq!(export.exported_count, 2);

    let mut imported = SqliteStorage::open_memory().unwrap();
    let import = import_from_jsonl(
        &mut imported,
        &path,
        &ImportConfig::default(),
        Some("test-"),
    )
    .unwrap();
    assert_eq!(import.imported_count, 2);

    let imported_alpha = imported.get_issue(&alpha.id).unwrap().unwrap();
    assert_eq!(imported_alpha.title, alpha.title);
    assert_eq!(imported_alpha.external_ref, Some("ext-1".to_string()));

    let labels = imported.get_labels(&alpha.id).unwrap();
    assert_eq!(labels, vec!["alpha".to_string()]);

    let deps = imported.get_dependencies(&beta.id).unwrap();
    assert_eq!(deps, vec![alpha.id.clone()]);

    let comments = imported.get_comments(&alpha.id).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].body, "first comment");
}

#[test]
fn import_reads_multiple_jsonl_lines_without_buffer_accumulation() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    let issue_a = issue_with_id("test-a", "First");
    let issue_b = issue_with_id("test-b", "Second");
    let json_a = serde_json::to_string(&issue_a).unwrap();
    let json_b = serde_json::to_string(&issue_b).unwrap();
    fs::write(&path, format!("{json_a}\n{json_b}\n")).unwrap();

    let mut storage = SqliteStorage::open_memory().unwrap();
    let import =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();

    assert_eq!(import.imported_count, 2);
    assert!(storage.get_issue("test-a").unwrap().is_some());
    assert!(storage.get_issue("test-b").unwrap().is_some());
}

#[test]
fn export_sorts_by_id() {
    let mut storage = SqliteStorage::open_memory().unwrap();
    let issue_b = issue_with_id("test-b", "B");
    let issue_a = issue_with_id("test-a", "A");

    storage.create_issue(&issue_b, "tester").unwrap();
    storage.create_issue(&issue_a, "tester").unwrap();

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    export_to_jsonl(&storage, &path, &ExportConfig::default()).unwrap();

    let issues = read_issues_from_jsonl(&path).unwrap();
    let ids: Vec<&str> = issues.iter().map(|issue| issue.id.as_str()).collect();
    assert_eq!(ids, vec!["test-a", "test-b"]);
}

#[test]
fn export_sorts_comments_canonically_after_loading_relations() {
    let mut storage = SqliteStorage::open_memory().unwrap();
    let timestamp = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
    let mut issue = issue_with_id("test-comment-sort", "Comment sort");
    issue.created_at = timestamp;
    issue.updated_at = timestamp;
    issue.comments = vec![
        Comment {
            id: 0,
            issue_id: issue.id.clone(),
            author: "zara".to_string(),
            body: "second by canonical order".to_string(),
            created_at: timestamp,
        },
        Comment {
            id: 0,
            issue_id: issue.id.clone(),
            author: "alice".to_string(),
            body: "first by canonical order".to_string(),
            created_at: timestamp,
        },
    ];

    storage.create_issue(&issue, "tester").unwrap();

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    export_to_jsonl(&storage, &path, &ExportConfig::default()).unwrap();

    let issues = read_issues_from_jsonl(&path).unwrap();
    assert_eq!(issues.len(), 1);
    let authors: Vec<&str> = issues[0]
        .comments
        .iter()
        .map(|comment| comment.author.as_str())
        .collect();
    assert_eq!(authors, vec!["alice", "zara"]);
}

#[test]
fn import_rejects_malformed_json() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    fs::write(&path, "not json\n").unwrap();

    let mut storage = SqliteStorage::open_memory().unwrap();
    let err = import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-"))
        .unwrap_err();
    assert!(err.to_string().contains("Invalid JSON"));
}

#[test]
fn import_rejects_prefix_mismatch() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    let issue = issue_with_id("xx-001", "Mismatch");
    let json = serde_json::to_string(&issue).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    let mut storage = SqliteStorage::open_memory().unwrap();
    let err = import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-"))
        .unwrap_err();
    assert!(err.to_string().contains("Prefix mismatch"));
}

#[test]
fn import_sets_closed_at_when_missing() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    let mut issue = issue_with_id("test-closed", "Closed");
    issue.status = Status::Closed;
    issue.created_at = Utc::now() - Duration::hours(2);
    issue.updated_at = Utc::now() - Duration::hours(1);
    issue.closed_at = None;
    let json = serde_json::to_string(&issue).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    let mut storage = SqliteStorage::open_memory().unwrap();
    import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();

    let imported = storage.get_issue(&issue.id).unwrap().unwrap();
    assert_eq!(imported.closed_at, Some(issue.updated_at));
}

#[test]
fn export_import_roundtrip_keeps_optional_text_fields_integrity_safe() {
    let mut storage = SqliteStorage::open_memory().unwrap();
    let issue = issue_with_id("test-opttext", "Optional text fields");
    storage.create_issue(&issue, "tester").unwrap();

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    export_to_jsonl(&storage, &path, &ExportConfig::default()).unwrap();

    let db_path = temp.path().join("import.db");
    let mut imported = SqliteStorage::open(&db_path).unwrap();
    import_from_jsonl(
        &mut imported,
        &path,
        &ImportConfig::default(),
        Some("test-"),
    )
    .unwrap();

    let imported_issue = imported.get_issue(&issue.id).unwrap().unwrap();
    assert_eq!(imported_issue.design, None);
    assert_eq!(imported_issue.acceptance_criteria, None);
}

#[test]
fn import_rejects_conflict_markers() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    fs::write(&path, "<<<<<<< HEAD\n").unwrap();

    let mut storage = SqliteStorage::open_memory().unwrap();
    let err = import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-"))
        .unwrap_err();
    assert!(err.to_string().contains("Merge conflict markers detected"));
}

// ===== Safety Guard Tests =====

#[test]
fn export_empty_db_guard_blocks_overwrite() {
    // Empty database should not overwrite non-empty JSONL without --force
    let storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create existing JSONL with content
    let existing = issue_with_id("test-existing", "Existing issue");
    let json = serde_json::to_string(&existing).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    // Try to export empty database (should fail)
    let config = ExportConfig {
        force: false,
        ..Default::default()
    };
    let result = export_to_jsonl(&storage, &path, &config);

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("empty database"),
        "Expected 'empty database' error, got: {err}"
    );
}

#[test]
fn export_empty_db_guard_bypassed_with_force() {
    // Empty database CAN overwrite non-empty JSONL with --force
    let storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create existing JSONL with content
    let existing = issue_with_id("test-existing", "Existing issue");
    let json = serde_json::to_string(&existing).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    // Export empty database with force (should succeed)
    let config = ExportConfig {
        force: true,
        ..Default::default()
    };
    let result = export_to_jsonl(&storage, &path, &config);

    assert!(result.is_ok());
    let export = result.unwrap();
    assert_eq!(export.exported_count, 0);
}

// ===== Tombstone Protection Tests =====

#[test]
fn import_tombstone_protection_prevents_resurrection() {
    // Tombstones in DB should never be resurrected by import
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create a tombstone in the database
    let mut tombstone = issue_with_id("test-tomb", "Tombstone issue");
    tombstone.status = Status::Tombstone;
    tombstone.deleted_at = Some(Utc::now());
    storage.create_issue(&tombstone, "tester").unwrap();

    // Create JSONL trying to resurrect the tombstone
    let mut incoming = issue_with_id("test-tomb", "Resurrected issue");
    incoming.status = Status::Open;
    incoming.updated_at = Utc::now() + Duration::hours(1);
    let json = serde_json::to_string(&incoming).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    // Import should skip the tombstone
    let result =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();
    assert_eq!(result.tombstone_skipped, 1);

    // Verify the issue is still a tombstone
    let still_tombstone = storage.get_issue("test-tomb").unwrap().unwrap();
    assert_eq!(still_tombstone.status, Status::Tombstone);
}

// ===== Collision Detection Tests =====

#[test]
fn import_collision_by_id_updates_when_newer() {
    // When importing an issue with same ID but newer timestamp, it should update
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create existing issue with older timestamp
    let mut existing = issue_with_id("test-001", "Old title");
    existing.updated_at = Utc::now() - Duration::hours(1);
    storage.create_issue(&existing, "tester").unwrap();

    // Create JSONL with same ID but newer timestamp
    let mut incoming = issue_with_id("test-001", "New title");
    incoming.updated_at = Utc::now();
    let json = serde_json::to_string(&incoming).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    // Import should update
    let result =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();
    assert_eq!(result.imported_count, 1);

    // Verify the update
    let updated = storage.get_issue("test-001").unwrap().unwrap();
    assert_eq!(updated.title, "New title");
}

#[test]
fn import_collision_by_id_skips_when_older() {
    // When importing an issue with same ID but older timestamp, it should skip
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create existing issue with newer timestamp
    let mut existing = issue_with_id("test-001", "Newer title");
    existing.created_at = Utc::now() - Duration::hours(2);
    existing.updated_at = Utc::now();
    storage.create_issue(&existing, "tester").unwrap();

    // Create JSONL with same ID but older timestamp
    let mut incoming = issue_with_id("test-001", "Older title");
    incoming.created_at = existing.created_at;
    incoming.updated_at = Utc::now() - Duration::hours(1);
    let json = serde_json::to_string(&incoming).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    // Import should skip
    let result =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();
    assert_eq!(result.skipped_count, 1);

    // Verify no change
    let unchanged = storage.get_issue("test-001").unwrap().unwrap();
    assert_eq!(unchanged.title, "Newer title");
}

#[test]
fn import_collision_by_external_ref() {
    // When importing an issue with matching external_ref, it should match (phase 1)
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create existing issue with external_ref
    let mut existing = issue_with_id("test-001", "Existing");
    existing.external_ref = Some("JIRA-123".to_string());
    existing.updated_at = Utc::now() - Duration::hours(1);
    storage.create_issue(&existing, "tester").unwrap();

    // Create JSONL with SAME external_ref and same ID, newer timestamp
    let mut incoming = issue_with_id("test-001", "Incoming updated");
    incoming.external_ref = Some("JIRA-123".to_string());
    incoming.updated_at = Utc::now();
    let json = serde_json::to_string(&incoming).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    // Import should update (matched by external_ref in phase 1)
    let result =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();
    assert_eq!(result.imported_count, 1);

    // Verify the update
    let updated = storage.get_issue("test-001").unwrap().unwrap();
    assert_eq!(updated.title, "Incoming updated");
}

// ===== Ephemeral Issue Tests =====

#[test]
fn import_skips_ephemeral_issues() {
    // Ephemeral issues should be skipped during import
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create JSONL with ephemeral issue
    let mut ephemeral = issue_with_id("test-eph", "Ephemeral issue");
    ephemeral.ephemeral = true;
    let json = serde_json::to_string(&ephemeral).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    // Import should skip
    let result =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();
    assert_eq!(result.skipped_count, 1);
    assert_eq!(result.imported_count, 0);

    // Verify the issue was not created
    assert!(storage.get_issue("test-eph").unwrap().is_none());
}

// ===== Prefix Validation Tests =====

#[test]
fn import_skip_prefix_validation_allows_mismatch() {
    // With skip_prefix_validation, mismatched prefixes should be allowed
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create JSONL with different prefix
    let issue = issue_with_id("other-001", "Different prefix");
    let json = serde_json::to_string(&issue).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    // Import with skip_prefix_validation should succeed
    let config = ImportConfig {
        skip_prefix_validation: true,
        ..Default::default()
    };
    let result = import_from_jsonl(&mut storage, &path, &config, Some("test-")).unwrap();
    assert_eq!(result.imported_count, 1);
}

// ===== Deterministic Export Tests =====

#[test]
fn export_produces_deterministic_content_hash() {
    // Multiple exports of the same data should produce the same content hash
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();

    // Create test issue
    let issue = issue_with_id("test-det", "Deterministic test");
    storage.create_issue(&issue, "tester").unwrap();

    let config = ExportConfig::default();

    // Export twice to different files
    let path1 = temp.path().join("export1.jsonl");
    let path2 = temp.path().join("export2.jsonl");

    let result1 = export_to_jsonl(&storage, &path1, &config).unwrap();
    let result2 = export_to_jsonl(&storage, &path2, &config).unwrap();

    // Hashes should be identical
    assert_eq!(result1.content_hash, result2.content_hash);
    assert!(!result1.content_hash.is_empty());
}

// ===== Empty Lines Handling Tests =====

#[test]
fn import_handles_empty_lines_gracefully() {
    // JSONL with empty lines interspersed should still import correctly
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create JSONL with empty lines
    let issue = issue_with_id("test-001", "Valid issue");
    let json = serde_json::to_string(&issue).unwrap();
    let content = format!("\n\n{json}\n\n\n");
    fs::write(&path, content).unwrap();

    // Import should succeed, ignoring empty lines
    let result =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();
    assert_eq!(result.imported_count, 1);
}

// ===== New Issue Creation Tests =====

#[test]
fn import_creates_new_issues() {
    // New issues (not in DB) should be created
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create JSONL with new issue
    let issue = issue_with_id("test-new", "Brand new issue");
    let json = serde_json::to_string(&issue).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    // Import should create the issue
    let result =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();
    assert_eq!(result.imported_count, 1);
    assert_eq!(result.skipped_count, 0);

    // Verify the issue exists
    let created = storage.get_issue("test-new").unwrap().unwrap();
    assert_eq!(created.title, "Brand new issue");
}

#[test]
fn import_repopulates_export_hashes() {
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Create and export an issue
    let issue = issue_with_id("test-hash", "Hash Test");
    storage.create_issue(&issue, "tester").unwrap();
    let export_result = export_to_jsonl(&storage, &path, &ExportConfig::default()).unwrap();
    finalize_export(
        &mut storage,
        &export_result,
        Some(&export_result.issue_hashes),
        &path,
    )
    .unwrap();
    let original_hash = export_result.issue_hashes[0].1.clone();

    // Verify hash exists
    assert_eq!(
        storage.get_export_hash("test-hash").unwrap().unwrap().0,
        original_hash
    );

    // Clear hash manually
    storage.clear_all_export_hashes().unwrap();
    assert!(storage.get_export_hash("test-hash").unwrap().is_none());

    // Import the file back
    import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();

    // Verify hash is restored
    let (restored_hash, _) = storage.get_export_hash("test-hash").unwrap().unwrap();
    assert_eq!(restored_hash, original_hash);
}

#[test]
fn import_deduplicates_export_hash_rebuild_when_multiple_records_target_same_issue() {
    let mut storage = SqliteStorage::open_memory().unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    let base_time = Utc::now() - Duration::hours(2);
    let mut existing = issue_with_id("test-existing", "Existing");
    existing.created_at = base_time;
    existing.updated_at = base_time;
    existing.external_ref = Some("EXT-1".to_string());
    storage.create_issue(&existing, "tester").unwrap();
    storage
        .set_export_hashes(&[("test-existing".to_string(), "stale-hash".to_string())])
        .unwrap();

    let mut by_external_ref = issue_with_id("test-remap", "Intermediate update");
    by_external_ref.created_at = base_time + Duration::minutes(5);
    by_external_ref.updated_at = base_time + Duration::minutes(10);
    by_external_ref.external_ref = Some("EXT-1".to_string());

    let mut by_id = issue_with_id("test-existing", "Final update");
    by_id.created_at = base_time + Duration::minutes(15);
    by_id.updated_at = base_time + Duration::minutes(20);

    let json = format!(
        "{}\n{}\n",
        serde_json::to_string(&by_external_ref).unwrap(),
        serde_json::to_string(&by_id).unwrap()
    );
    fs::write(&path, json).unwrap();

    import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();

    assert!(
        storage.get_issue("test-remap").unwrap().is_none(),
        "collision-matched issue should be merged into the existing record"
    );

    let imported = storage.get_issue("test-existing").unwrap().unwrap();
    assert_eq!(imported.title, "Final update");

    let (stored_hash, _) = storage.get_export_hash("test-existing").unwrap().unwrap();
    assert_eq!(Some(stored_hash.as_str()), imported.content_hash.as_deref());
}

#[test]
fn import_rejects_invalid_id_format() {
    // Import now validates issues, so invalid IDs should be rejected.
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    let issue = issue_with_id("test-INVALID", "Invalid ID");
    let json = serde_json::to_string(&issue).unwrap();
    fs::write(&path, format!("{json}\n")).unwrap();

    let mut storage = SqliteStorage::open_memory().unwrap();
    let result = import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-"));

    assert!(result.is_err(), "Import should fail for invalid IDs");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Validation failed"),
        "Expected validation error, got: {err}"
    );
}

// governed-by: ADR-0001
// Wave-5 L1 (beads_rust-schema-v18-uyb3): schema-17 JSONL lines carry none of
// the typed work-ledger fields; import must fill serde defaults and persist
// them through the v18 columns.
#[test]
fn schema18_import_of_schema17_jsonl_fills_defaults() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Raw schema-17 line: no verify/principles/wave/pin/commit_sha/
    // close_verdict/ac_shape/blast keys at all.
    let legacy_line = r#"{"id":"bd-schema17","title":"legacy row","status":"open","priority":2,"issue_type":"task","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","created_by":"ubuntu"}"#;
    fs::write(&path, format!("{legacy_line}\n")).unwrap();

    let mut storage = SqliteStorage::open_memory().unwrap();
    let import =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("bd-")).unwrap();
    assert_eq!(import.imported_count, 1);

    let issue = storage
        .get_issue("bd-schema17")
        .unwrap()
        .expect("legacy row imported");

    // Serde defaults filled for every absent v18 field.
    assert!(issue.verify.is_none(), "verify defaults to None");
    assert!(
        issue.principles.is_empty(),
        "principles default to an empty citation list"
    );
    assert!(issue.wave.is_none(), "wave defaults to None");
    assert!(issue.pin.is_none(), "pin defaults to None");
    assert!(issue.commit_sha.is_none(), "commit_sha defaults to None");
    assert!(
        issue.close_verdict.is_none(),
        "close_verdict defaults to None"
    );
    assert_eq!(
        issue.ac_shape,
        beads::model::AcShape::Checkable,
        "ac_shape defaults to checkable"
    );
    assert_eq!(
        issue.blast,
        beads::model::Blast::Normal,
        "blast defaults to normal"
    );

    // Round-trip: enriched fields survive export + re-import.
    let mut updated = issue.clone();
    updated.verify = Some("cargo test --offline schema18".to_string());
    updated.principles = vec![beads::model::PrincipleCitation {
        name: "prove-it-works".to_string(),
        decision: "stamp only after integrity_check passes".to_string(),
    }];
    updated.wave = Some(5);
    updated.commit_sha = Some("8615bac8".to_string());
    storage.upsert_issue_for_import(&updated).unwrap();

    let out_path = temp.path().join("out.jsonl");
    export_to_jsonl(&storage, &out_path, &ExportConfig::default()).unwrap();

    let mut fresh = SqliteStorage::open_memory().unwrap();
    let re =
        import_from_jsonl(&mut fresh, &out_path, &ImportConfig::default(), Some("bd-")).unwrap();
    assert_eq!(re.imported_count, 1);
    let round_tripped = fresh.get_issue("bd-schema17").unwrap().unwrap();
    assert_eq!(
        round_tripped.verify.as_deref(),
        Some("cargo test --offline schema18")
    );
    assert_eq!(round_tripped.principles.len(), 1);
    assert_eq!(round_tripped.principles[0].name, "prove-it-works");
    assert_eq!(
        round_tripped.principles[0].decision,
        "stamp only after integrity_check passes"
    );
    assert_eq!(round_tripped.wave, Some(5));
    assert_eq!(round_tripped.commit_sha.as_deref(), Some("8615bac8"));
}

/// ADR-0004 (beads_rust-revisioned-storage-contract-hcvq3): the durable
/// `revision` CAS token is not part of content identity and defaults to one
/// for legacy JSONL records that predate it. A legacy row without the key
/// must import cleanly with `revision == 1`, survive export, and never
/// collide with an explicit revision carried by a newer record.
// governed-by: ADR-0004
#[test]
fn legacy_jsonl_without_revision_imports_with_revision_one() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");

    // Raw legacy line: no `revision` key (and none of the newer v18 fields).
    let legacy_line = r#"{"id":"bd-legacy-rev","title":"legacy row","status":"open","priority":2,"issue_type":"task","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","created_by":"ubuntu"}"#;
    // Explicit-revision line: a newer record that carries its own token.
    let revised_line = r#"{"id":"bd-revised-rev","title":"revised row","status":"open","priority":2,"issue_type":"task","created_at":"2026-01-02T00:00:00Z","updated_at":"2026-01-02T00:00:00Z","created_by":"ubuntu","revision":7}"#;
    fs::write(&path, format!("{legacy_line}\n{revised_line}\n")).unwrap();

    let mut storage = SqliteStorage::open_memory().unwrap();
    let import =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("bd-")).unwrap();
    assert_eq!(import.imported_count, 2);

    let legacy = storage.get_issue("bd-legacy-rev").unwrap().unwrap();
    assert_eq!(
        legacy.revision, 1,
        "legacy record without `revision` defaults to 1"
    );

    let revised = storage.get_issue("bd-revised-rev").unwrap().unwrap();
    assert_eq!(
        revised.revision, 7,
        "explicit `revision` in the source record is preserved"
    );

    // Round-trip: both tokens survive export + re-import unchanged.
    let out_path = temp.path().join("out.jsonl");
    export_to_jsonl(&storage, &out_path, &ExportConfig::default()).unwrap();

    let mut fresh = SqliteStorage::open_memory().unwrap();
    let re =
        import_from_jsonl(&mut fresh, &out_path, &ImportConfig::default(), Some("bd-")).unwrap();
    assert_eq!(re.imported_count, 2);
    assert_eq!(
        fresh.get_issue("bd-legacy-rev").unwrap().unwrap().revision,
        1,
        "legacy token stays 1 after round-trip"
    );
    assert_eq!(
        fresh.get_issue("bd-revised-rev").unwrap().unwrap().revision,
        7,
        "explicit token stays 7 after round-trip"
    );
}

/// ADR-0001 §5.4 (beads_rust-gates-jsonl-ea54): gate verdicts are ledger rows
/// that survive a git clone. Flush writes the `.beads/gates.jsonl` sidecar;
/// import-only reloads it; the roundtrip proves a fresh database can still
/// authorize the closes those rows licensed.
// governed-by: ADR-0001
#[test]
fn gates_jsonl_roundtrip_flush_import() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("beads.db");
    let issues_path = temp.path().join("issues.jsonl");
    let gates_path = temp.path().join("gates.jsonl");

    {
        let mut storage = SqliteStorage::open(&db_path).unwrap();
        storage
            .create_issue(&issue_with_id("test-g1", "Gated work"), "tester")
            .unwrap();
        storage
            .record_scoped_gate_result(
                "test-g1",
                "open",
                0,
                "closed",
                "unit-test-verified",
                "verifier",
                true,
                Some("cargo test => green"),
                "tester",
            )
            .unwrap();

        // FLUSH: one call exports both files; the sidecar lands next to the
        // issues export with the derived name.
        export_to_jsonl(&storage, &issues_path, &ExportConfig::default()).unwrap();
        export_gates_to_jsonl(&storage, &gates_path, &ExportConfig::default()).unwrap();
    }

    let sidecar = fs::read_to_string(&gates_path).unwrap();
    assert!(
        sidecar.contains("\"issue_id\":\"test-g1\"")
            || sidecar.contains("\"issue_id\": \"test-g1\""),
        "sidecar must carry the verdict row: {sidecar}"
    );
    assert_eq!(sidecar.lines().count(), 1, "one row per line");

    // WIPE: simulate a fresh clone — no database, only the JSONL pair.
    fs::remove_file(&db_path).unwrap();

    let mut fresh = SqliteStorage::open(&db_path).unwrap();
    import_from_jsonl(
        &mut fresh,
        &issues_path,
        &ImportConfig::default(),
        Some("test-"),
    )
    .unwrap();
    import_gates_from_jsonl(&mut fresh, &gates_path, &ImportConfig::default()).unwrap();

    let rows = fresh
        .get_scoped_gate_results("test-g1", "open", "closed")
        .unwrap();
    assert_eq!(rows.len(), 1, "PASS row must be queryable after reimport");
    assert!(rows[0].passed);
    assert_eq!(rows[0].gate, "unit-test-verified");
    assert_eq!(rows[0].provider, "verifier");
}

/// ADR-0001 §5.4: `data_hash` covers BOTH exported files and is
/// deterministic for identical content; touching either file changes it.
#[test]
fn ledger_data_hash_covers_issues_and_gates_sidecar() {
    use beads::sync::compute_ledger_data_hash;

    let temp = TempDir::new().unwrap();
    let issues_path = temp.path().join("issues.jsonl");
    let gates_path = temp.path().join("gates.jsonl");

    assert!(
        compute_ledger_data_hash(&issues_path, &gates_path).is_none(),
        "no fingerprint when the issues export is missing"
    );

    fs::write(&issues_path, "{\"id\":\"test-h1\"}\n").unwrap();

    fs::write(&gates_path, "").unwrap(); // absent sidecar == empty frame
    let baseline = compute_ledger_data_hash(&issues_path, &gates_path).unwrap();

    // Same content, different path: identical hash.
    let twin_issues = temp.path().join("twin.jsonl");
    fs::write(&twin_issues, "{\"id\":\"test-h1\"}\n").unwrap();
    assert_eq!(
        compute_ledger_data_hash(&issues_path, &gates_path),
        compute_ledger_data_hash(&twin_issues, &gates_path)
    );

    // A verdict-row change flips the combined hash.
    fs::write(&gates_path, "{\"issue_id\":\"test-g1\",\"passed\":true}\n").unwrap();
    let after_gate = compute_ledger_data_hash(&issues_path, &gates_path).unwrap();
    assert_ne!(baseline, after_gate);

    // So does an issue-row change.
    fs::write(&issues_path, "{\"id\":\"test-h2\"}\n").unwrap();
    assert_ne!(
        after_gate,
        compute_ledger_data_hash(&issues_path, &gates_path).unwrap()
    );
}

/// ADR-0001 §5.2 (beads_rust-fence-import-ou8g): one-shot fence import.
/// Legacy beads carry ## VERIFY / ## PRINCIPLES markdown fences in their
/// description; the helper stamps the typed fields EXACTLY once — later
/// fence edits are ignored because the field, not the prose, is the source
/// of truth. The description itself is never modified.
#[test]
fn fence_import_copies_verify_once_then_ignores_later_fence_edits() {
    use beads::sync::fence_import::sync_fences_into_typed_fields;

    let mut storage = SqliteStorage::open_memory().unwrap();
    let now = Utc::now();
    let mut legacy = fixtures::issue("Legacy fenced bead");
    legacy.id = "test-f1".to_string();
    legacy.created_at = now;
    legacy.updated_at = now;
    legacy.verify = None;
    legacy.principles = Vec::new();
    legacy.description = Some(
        "Work items.\n\n## VERIFY\n```\ntimeout 600 cargo test --offline fence_import\n```\n\n\
         ## PRINCIPLES\nprove-it-works — proof survives git clone\n\
         not-a-principle no dash here\n"
            .to_string(),
    );
    legacy.acceptance_criteria = None;
    legacy.design = None;
    legacy.notes = None;
    storage.create_issue(&legacy, "tester").unwrap();

    // First import: fence contents land in the typed fields.
    let outcome = sync_fences_into_typed_fields(&mut storage, "fence-import").unwrap();
    assert_eq!(outcome.issues_updated, 1, "the legacy bead gets stamped");

    let stamped = storage.get_issue("test-f1").unwrap().unwrap();
    assert_eq!(
        stamped.verify.as_deref(),
        Some("timeout 600 cargo test --offline fence_import"),
        "VERIFY fence command copied into the typed field"
    );
    assert_eq!(
        stamped.principles.len(),
        1,
        "only 'name — decision' lines cite"
    );
    assert_eq!(stamped.principles[0].name, "prove-it-works");
    assert_eq!(stamped.principles[0].decision, "proof survives git clone");
    let description_after_first = stamped.description.clone().unwrap();
    assert!(
        description_after_first.contains("## VERIFY"),
        "import is lossless: description untouched"
    );

    // Later fence edit: fields are already set, so the edit is IGNORED.
    let mut edited = stamped.clone();
    edited.description = Some(
        "Work items.\n\n## VERIFY\n```\ncargo test --offline something_else_entirely\n```\n\n\
         ## PRINCIPLES\nchase-shiny — later fence edits must not win\n"
            .to_string(),
    );
    let updates = IssueUpdate {
        description: Some(Some(edited.description.unwrap())),
        ..IssueUpdate::default()
    };
    storage.update_issue("test-f1", &updates, "tester").unwrap();

    let outcome2 = sync_fences_into_typed_fields(&mut storage, "fence-import").unwrap();
    assert_eq!(
        outcome2.issues_updated, 0,
        "one-shot: nothing left to stamp"
    );

    let final_issue = storage.get_issue("test-f1").unwrap().unwrap();
    assert_eq!(
        final_issue.verify.as_deref(),
        Some("timeout 600 cargo test --offline fence_import"),
        "later fence edits are ignored once the field is set"
    );
    assert_eq!(final_issue.principles.len(), 1);
    assert_eq!(final_issue.principles[0].name, "prove-it-works");
}

/// §5.2: an issue whose typed fields are ALREADY set is skipped even though
/// its description still carries fences (the pre-stamped shape).
#[test]
fn fence_import_skips_beads_with_typed_fields_already_set() {
    use beads::sync::fence_import::sync_fences_into_typed_fields;

    let mut storage = SqliteStorage::open_memory().unwrap();
    let now = Utc::now();
    let mut modern = fixtures::issue("Modern bead");
    modern.id = "test-f2".to_string();
    modern.created_at = now;
    modern.updated_at = now;
    modern.verify = Some("br doctor --check".to_string());
    modern.principles = Vec::new();
    modern.description = Some("## VERIFY\ncargo build\n".to_string());
    storage.create_issue(&modern, "tester").unwrap();

    let outcome = sync_fences_into_typed_fields(&mut storage, "fence-import").unwrap();
    assert_eq!(outcome.issues_updated, 0);
    let issue = storage.get_issue("test-f2").unwrap().unwrap();
    assert_eq!(issue.verify.as_deref(), Some("br doctor --check"));
}

/// beads_rust-svtxe: the importer must not count inline dependencies it
/// later silently deletes. Resolvable and `external:*` targets persist;
/// a target that exists neither in the database nor in the JSONL is a
/// hard, precise error — never a silent drop followed by an opaque
/// post-recovery row-count mismatch.
#[test]
fn import_persists_inline_dependencies_and_rejects_unresolvable_targets() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    let mk = |id: &str, deps: &[(&str, &str)]| {
        let mut issue = issue_with_id(id, id);
        issue.created_at = Utc::now() - Duration::hours(1);
        issue.updated_at = issue.created_at;
        issue.dependencies = deps
            .iter()
            .map(|(target, dep_type)| beads::model::Dependency {
                issue_id: id.to_string(),
                depends_on_id: (*target).to_string(),
                dep_type: dep_type
                    .parse()
                    .unwrap_or_else(|_| DependencyType::Custom((*dep_type).to_string())),
                created_at: issue.created_at,
                created_by: Some("tester".to_string()),
                metadata: None,
                thread_id: None,
            })
            .collect();
        serde_json::to_string(&issue).unwrap()
    };

    // Happy path: resolvable in-file target + external:* cross-tracker
    // targets all persist; counted == persisted.
    let lines = [
        mk(
            "test-a",
            &[("test-b", "blocks"), ("external:ext-7", "blocks")],
        ),
        mk("test-b", &[("external:other-tracker-7", "related")]),
    ];
    fs::write(&path, format!("{}\n{}\n", lines[0], lines[1])).unwrap();

    let mut storage = SqliteStorage::open_memory().unwrap();
    let import =
        import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-")).unwrap();

    assert_eq!(import.imported_count, 2);
    assert_eq!(
        import.dependencies_imported, 3,
        "all three inline deps must be counted AND persisted"
    );

    let deps_a = storage.get_dependencies("test-a").unwrap();
    assert!(
        deps_a.contains(&"test-b".to_string()),
        "resolvable dep persisted"
    );
    assert!(
        deps_a.contains(&"external:ext-7".to_string()),
        "external dep persisted"
    );
    let deps_b = storage.get_dependencies("test-b").unwrap();
    assert_eq!(deps_b, vec!["external:other-tracker-7".to_string()]);
}

/// beads_rust-svtxe: a dangling non-external dependency target fails the
/// import up front with a diagnostic naming the target, instead of the
/// old silent-drop-then-fail-closed "row count mismatch".
#[test]
fn import_rejects_inline_dependency_with_unknown_target_up_front() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("issues.jsonl");
    let mk = |id: &str, deps: &[(&str, &str)]| {
        let mut issue = issue_with_id(id, id);
        issue.created_at = Utc::now() - Duration::hours(1);
        issue.updated_at = issue.created_at;
        issue.dependencies = deps
            .iter()
            .map(|(target, dep_type)| beads::model::Dependency {
                issue_id: id.to_string(),
                depends_on_id: (*target).to_string(),
                dep_type: dep_type
                    .parse()
                    .unwrap_or_else(|_| DependencyType::Custom((*dep_type).to_string())),
                created_at: issue.created_at,
                created_by: Some("tester".to_string()),
                metadata: None,
                thread_id: None,
            })
            .collect();
        serde_json::to_string(&issue).unwrap()
    };

    let line_a = mk("test-a", &[("ghost", "blocks")]);
    fs::write(&path, format!("{line_a}\n")).unwrap();

    let mut storage = SqliteStorage::open_memory().unwrap();
    let err = import_from_jsonl(&mut storage, &path, &ImportConfig::default(), Some("test-"))
        .expect_err("dangling dep target must fail the import");

    let msg = err.to_string();
    assert!(
        msg.contains("ghost"),
        "error must name the unresolvable target: {msg}"
    );
    assert!(
        msg.contains("test-a"),
        "error must name the referencing issue: {msg}"
    );
}
