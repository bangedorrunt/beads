//! bv-compatible `data_hash` (ADR-0003 §3.1 envelope contract).
// governed-by: ADR-0003
//!
//! Reproduces beads_viewer `analysis.ComputeDataHash` byte-for-byte so a
//! `br` hash equals a `bv` hash over the same issue set (docs/research/
//! bv-robot-surface.md §1): SHA-256 over ID-sorted issues, NUL-separated
//! fields, labels and dependencies sorted, `0x01` issue separator, first 16
//! hex chars. Optional fields hash as empty when absent (bv's loader
//! defaults). Timestamps use Go's RFC3339Nano form (trailing-zero-trimmed
//! fraction, no fraction when nanos == 0).

use sha2::{Digest, Sha256};

use crate::model::Issue;

/// Format a UTC timestamp exactly like Go `time.RFC3339Nano`: fractional
/// digits trimmed of trailing zeros, omitted entirely when nanos are zero.
fn go_rfc3339_nano(t: &chrono::DateTime<chrono::Utc>) -> String {
    use chrono::Timelike;
    let base = t.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = t.nanosecond() % 1_000_000_000;
    if nanos == 0 {
        format!("{base}Z")
    } else {
        let frac = format!("{nanos:09}");
        let trimmed = frac.trim_end_matches('0');
        format!("{base}.{trimmed}Z")
    }
}

fn field(h: &mut Sha256, s: &str) {
    Digest::update(h, s.as_bytes());
    Digest::update(h, [0u8]);
}

/// Compute the 16-hex data hash of an issue set; `"empty"` when empty.
pub fn compute_data_hash(issues: &[Issue]) -> String {
    if issues.is_empty() {
        return "empty".to_string();
    }

    let mut sorted: Vec<&Issue> = issues.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));

    let mut h = Sha256::new();
    for issue in sorted {
        field(&mut h, &issue.id);
        field(&mut h, issue.title.as_str());
        field(&mut h, issue.description.as_deref().unwrap_or(""));
        field(&mut h, issue.notes.as_deref().unwrap_or(""));
        field(&mut h, issue.design.as_deref().unwrap_or(""));
        field(&mut h, issue.acceptance_criteria.as_deref().unwrap_or(""));
        field(&mut h, issue.assignee.as_deref().unwrap_or(""));
        field(&mut h, issue.source_repo.as_deref().unwrap_or(""));
        // bv's model: nil ExternalRef writes nothing before its NUL.
        field(&mut h, issue.external_ref.as_deref().unwrap_or(""));
        field(&mut h, issue.status.as_str());
        field(&mut h, issue.issue_type.as_str());
        field(&mut h, &issue.priority.0.to_string());
        // bv: nil EstimatedMinutes writes nothing before its NUL.
        field(
            &mut h,
            &issue
                .estimated_minutes
                .map_or_else(String::new, |m| m.to_string()),
        );
        field(&mut h, &go_rfc3339_nano(&issue.created_at));
        field(&mut h, &go_rfc3339_nano(&issue.updated_at));
        field(
            &mut h,
            &issue
                .closed_at
                .as_ref()
                .map_or_else(String::new, go_rfc3339_nano),
        );
        field(
            &mut h,
            &issue
                .defer_until
                .as_ref()
                .map_or_else(String::new, go_rfc3339_nano),
        );

        // Labels sorted.
        let mut labels = issue.labels.clone();
        labels.sort();
        for label in &labels {
            field(&mut h, label);
        }
        Digest::update(&mut h, [0u8]);

        // Dependencies sorted by (depends_on, type, created_at, created_by).
        let mut deps: Vec<&crate::model::Dependency> = issue.dependencies.iter().collect();
        deps.sort_by(|a, b| {
            a.depends_on_id
                .cmp(&b.depends_on_id)
                .then_with(|| a.dep_type.as_str().cmp(b.dep_type.as_str()))
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.created_by.cmp(&b.created_by))
        });
        for dep in deps {
            field(&mut h, &dep.depends_on_id);
            field(&mut h, dep.dep_type.as_str());
            field(&mut h, &go_rfc3339_nano(&dep.created_at));
            field(&mut h, dep.created_by.as_deref().unwrap_or(""));
        }
        Digest::update(&mut h, [0u8]);

        // Comments sorted by (id, created_at, author, text).
        let mut comments: Vec<&crate::model::Comment> = issue.comments.iter().collect();
        comments.sort_by(|a, b| {
            a.id.cmp(&b.id)
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.author.cmp(&b.author))
                .then_with(|| a.body.cmp(&b.body))
        });
        for c in comments {
            field(&mut h, &c.id.to_string());
            field(&mut h, &c.author);
            field(&mut h, &c.body);
            field(&mut h, &go_rfc3339_nano(&c.created_at));
        }

        Digest::update(&mut h, [1u8]); // issue separator
    }

    let digest = crate::util::hex_encode(&h.finalize());
    digest[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Minimal issue via serde (the model has no Default; JSON mirrors the
    /// JSONL wire form).
    fn fx(id: &str) -> Issue {
        serde_json::from_str(&format!(
            r#"{{"id": "{id}", "title": "t", "status": "open",
                "priority": 2, "issue_type": "task",
                "created_at": "2026-08-01T10:00:00Z",
                "updated_at": "2026-08-10T10:00:00Z"}}"#
        ))
        .expect("test issue parses")
    }

    #[test]
    fn empty_store_hashes_to_empty() {
        assert_eq!(compute_data_hash(&[]), "empty");
    }

    #[test]
    fn go_rfc3339_nano_matches_go_forms() {
        let t = chrono::Utc.timestamp_opt(1_754_042_400, 0).unwrap(); // 2025-08-01T10:00:00Z
        assert_eq!(go_rfc3339_nano(&t), "2025-08-01T10:00:00Z");
        let half = chrono::Utc
            .timestamp_opt(1_754_042_400, 500_000_000)
            .unwrap();
        assert_eq!(go_rfc3339_nano(&half), "2025-08-01T10:00:00.5Z");
        let micro = chrono::Utc
            .timestamp_opt(1_754_042_400, 123_456_789)
            .unwrap();
        assert_eq!(go_rfc3339_nano(&micro), "2025-08-01T10:00:00.123456789Z");
    }

    #[test]
    fn hash_is_order_independent_and_id_sorted() {
        let mut a = fx("x-1");
        a.title = "same".into();
        let mut b = fx("x-2");
        b.title = "same".into();
        let h1 = compute_data_hash(&[a.clone(), b.clone()]);
        let h2 = compute_data_hash(&[b, a]);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
    }

    /// Cross-tool parity: hashing the committed fixture workspace's JSONL
    /// (12 issues) must equal the `data_hash` recorded in the golden
    /// `robot-next.json` captured from real bv v0.21 output.
    #[test]
    fn fixture_data_hash_matches_bv() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/bv_parity/fixture_issues.jsonl"
        );
        let raw = std::fs::read_to_string(path).expect("fixture jsonl");
        let issues: Vec<Issue> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("fixture line parses"))
            .collect();
        assert_eq!(issues.len(), 12);

        let golden: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/bv_parity/robot-next.json"
            ))
            .expect("golden robot-next"),
        )
        .expect("golden json");
        let expected = golden["data_hash"].as_str().expect("data_hash");
        assert_eq!(&compute_data_hash(&issues), expected);
    }
}
