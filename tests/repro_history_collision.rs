use beads::sync::history::{HistoryConfig, backup_before_export, list_backups};
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn test_backup_stem_collision() {
    let temp = TempDir::new().unwrap();
    let beads_dir = temp.path().join(".beads");
    std::fs::create_dir_all(&beads_dir).unwrap();

    let config = HistoryConfig {
        enabled: true,
        max_count: 5,
        max_age_days: 30,
        min_interval_secs: 0,
    };

    let archive_path = beads_dir.join("issues_archive.jsonl");
    let mut f = File::create(&archive_path).unwrap();
    f.write_all(b"archive data").unwrap();

    backup_before_export(&beads_dir, &config, &archive_path).unwrap();

    let history_dir = beads_dir.join(".br_history");
    let backups = list_backups(&history_dir, None).unwrap();
    assert_eq!(backups.len(), 1);
    assert!(backups[0].path.to_string_lossy().contains("issues_archive"));

    std::thread::sleep(std::time::Duration::from_secs(1));

    let issues_path = beads_dir.join("issues.jsonl");
    let mut f = File::create(&issues_path).unwrap();
    f.write_all(b"current data").unwrap();

    backup_before_export(&beads_dir, &config, &issues_path).unwrap();

    let backups = list_backups(&history_dir, None).unwrap();
    assert_eq!(backups.len(), 2, "Should have 2 backups total");

    let config_strict = HistoryConfig {
        enabled: true,
        max_count: 1,
        max_age_days: 30,
        min_interval_secs: 0,
    };

    std::fs::remove_dir_all(&history_dir).unwrap();
    std::fs::create_dir_all(&history_dir).unwrap();

    backup_before_export(&beads_dir, &config_strict, &archive_path).unwrap();
    assert_eq!(list_backups(&history_dir, None).unwrap().len(), 1);

    std::thread::sleep(std::time::Duration::from_secs(1));

    backup_before_export(&beads_dir, &config_strict, &issues_path).unwrap();

    let backups = list_backups(&history_dir, None).unwrap();

    assert_eq!(
        backups.len(),
        2,
        "Backups for different files should effectively have separate quotas, or at least not delete each other immediately"
    );
}
