// governed-by: ADR-0001
//! Legal-close table + loop-runnable classifier (ADR-0001 §5.3).
//!
//! Ported from flywheel `src/verdict.rs` (behavioral original). Pure data
//! and pure predicates; no IO in this module by design so `br close`,
//! `br ready`, and flywheel cannot drift.

/// Acceptance-criteria shape: is the AC a checkable command/test, or a
/// judgment ("review", "design") with no checkable command?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcShape {
    Checkable,
    Judgment,
}

/// Blast radius of a bead (ADR-0001 §5.2). High forces the P0/P1 band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blast {
    Normal,
    High,
}

/// Who produced the proof (ADR-0001 §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VerdictKind {
    /// The LOOP executed VERIFY and wrote the receipt itself.
    CommandVerified,
    /// Independent verifier ran the command/test.
    UnitTestVerified,
    /// Independent verifier exercised the live surface.
    LiveVerified,
    /// Independent verifier signed a judgment-AC bead (no command exists).
    ReviewerSigned,
    /// Worker self-report; legal only where the loop cannot run VERIFY.
    WorkerReceipt,
    /// Loop state, not a pass.
    VerifierBlocked,
    /// Loop state, not a pass.
    VerifierFailed,
}

impl VerdictKind {
    /// Every kind; the exhaustive-match surface for [`legal_close`].
    pub const ALL: [Self; 7] = [
        Self::CommandVerified,
        Self::UnitTestVerified,
        Self::LiveVerified,
        Self::ReviewerSigned,
        Self::WorkerReceipt,
        Self::VerifierBlocked,
        Self::VerifierFailed,
    ];

    /// br gate name (kebab-cased), matching `br gate report --gate`.
    #[must_use]
    pub fn gate_name(self) -> &'static str {
        match self {
            Self::CommandVerified => "command-verified",
            Self::UnitTestVerified => "unit-test-verified",
            Self::LiveVerified => "live-verified",
            Self::ReviewerSigned => "reviewer-signed",
            Self::WorkerReceipt => "worker-receipt",
            Self::VerifierBlocked => "verifier-blocked",
            Self::VerifierFailed => "verifier-failed",
        }
    }

    /// Inverse of [`gate_name`](Self::gate_name) — parse a br gate row's
    /// gate name back into a kind. Unknown names -> `None`.
    #[must_use]
    pub fn from_gate_name(name: &str) -> Option<Self> {
        Some(match name {
            "command-verified" => Self::CommandVerified,
            "unit-test-verified" => Self::UnitTestVerified,
            "live-verified" => Self::LiveVerified,
            "reviewer-signed" => Self::ReviewerSigned,
            "worker-receipt" => Self::WorkerReceipt,
            "verifier-blocked" => Self::VerifierBlocked,
            "verifier-failed" => Self::VerifierFailed,
            _ => return None,
        })
    }
}

/// Cheap-band classifier (ADR-0001 §5.3): VERIFY is loop-runnable when it
/// is a single command line with an allowlisted executable, estimated cheap
/// — the loop can execute it headless within the bead's TIMEBOX. Shell
/// composition (`&&`, `||`, `|`, `;`, backticks, `$()`) is never
/// loop-runnable; `cd x && cargo test` is the canonical non-example.
#[must_use]
pub fn is_loop_runnable(verify: &str) -> bool {
    match parse_verify_command(verify) {
        Some((exec, args)) => allowlisted_exec(&exec) && estimated_cheap(&exec, &args),
        None => false,
    }
}

/// Parse a VERIFY line into `(executable, args)`. `None` when empty,
/// multi-line, or shell composition (`&&`, `||`, `|`, `;`, backticks,
/// `$()`) — composition is never loop-runnable.
fn parse_verify_command(verify: &str) -> Option<(String, Vec<String>)> {
    let t = verify.trim();
    if t.is_empty()
        || t.contains('\n')
        || t.contains("&&")
        || t.contains("||")
        || t.contains('|')
        || t.contains(';')
        || t.contains('`')
        || t.contains("$(")
    {
        return None;
    }
    let mut parts = t.split_whitespace();
    let exec = parts.next()?.to_string();
    let args = parts.map(String::from).collect();
    Some((exec, args))
}

/// Executables the loop may run headless on the cheap band. A "bounded
/// shell" (sh/bash) is allowed only when the invocation carries an explicit
/// `timeout` bound — an unbounded shell is a time bomb.
fn allowlisted_exec(exec: &str) -> bool {
    matches!(
        exec,
        "cargo" | "test" | "pytest" | "python" | "python3" | "timeout" | "sh" | "bash"
    )
}

/// Known-heavy markers: whole-workspace matrices, long waits, live servers.
fn is_heavy_marker(arg: &str) -> bool {
    arg == "--workspace"
        || arg == "--all-features"
        || arg == "--all-targets"
        || arg == "--ignored"
        || arg.contains("verify-gates")
        || arg.starts_with("sleep")
        || arg.starts_with("serve")
}

/// Estimated-cheap heuristic: a targeted, bounded check. Whole-workspace
/// matrices and unbounded shells are time bombs — non-runnable band.
fn estimated_cheap(exec: &str, args: &[String]) -> bool {
    match exec {
        // cargo: a TARGETED test run (`-p <pkg>` and/or a test filter after
        // `test`); never the whole workspace matrix or a bare `cargo test`.
        "cargo" => {
            let Some(test_pos) = args.iter().position(|a| a == "test") else {
                return false; // cargo without `test` is not a verification
            };
            !args.iter().any(|a| is_heavy_marker(a))
                && (args[test_pos + 1..]
                    .iter()
                    .any(|a| a == "-p" || a.starts_with("-p") || a.starts_with("--package"))
                    || args[test_pos + 1..].iter().any(|a| !a.starts_with('-')))
        }
        // pytest / test runners / python: targeted by default — reject the
        // known-heavy markers.
        "pytest" | "test" | "python" | "python3" => !args.iter().any(|a| is_heavy_marker(a)),
        // Bounded shell: must carry an explicit `timeout` bound (quoted
        // args split on whitespace, so match the token as a substring).
        "sh" | "bash" => args.iter().any(|a| a.contains("timeout")),
        // An explicit `timeout` wrapper is loop-runnable by definition (the
        // bound is the caller's call).
        "timeout" => true,
        _ => false,
    }
}

/// Inputs to the legal-close decision (mirrors flywheel `legal_close`'s
/// `BeadUnit` + verdict kind). Priority is the br scale 0-4.
pub struct LegalCloseInput<'a> {
    pub priority: u8,
    pub blast: Blast,
    pub ac: AcShape,
    /// VERIFY command. Required for checkable ACs; empty means not runnable.
    pub verify: &'a str,
}

/// Legal-close table (ADR-0001 §5.3):
///
/// | Condition | Legal gate names |
/// |---|---|
/// | priority >= 2, blast Normal, VERIFY loop-runnable | `command-verified` ONLY |
/// | priority >= 2, blast Normal, VERIFY not loop-runnable | `worker-receipt`, or `unit-test-verified` / `live-verified` |
/// | priority <= 1 or blast High | `unit-test-verified` / `live-verified` only |
/// | AC is judgment | `reviewer-signed` only |
/// | any | never a FAIL row, never missing |
///
/// Exhaustive over [`VerdictKind`] with no wildcard arm: a new kind without
/// a case here does not compile.
#[must_use]
pub fn legal_close(kind: VerdictKind, bead: &LegalCloseInput<'_>) -> bool {
    let runnable = is_loop_runnable(bead.verify);
    match kind {
        // Row 1 — cheap band: loop-run CommandVerified ONLY.
        VerdictKind::CommandVerified => {
            bead.ac == AcShape::Checkable
                && bead.priority >= 2
                && bead.blast == Blast::Normal
                && runnable
        }
        // Row 2 — non-runnable band: WorkerReceipt (+ two-tick grace) or an
        // independent unit/live verification.
        VerdictKind::WorkerReceipt => {
            bead.ac == AcShape::Checkable
                && bead.priority >= 2
                && bead.blast == Blast::Normal
                && !runnable
        }
        // Rows 2+3 — independent verification. Legal for the non-runnable
        // band (ADR row 2) and for the P0/P1 / High-blast band (row 3);
        // illegal only in the cheap band, where a loop-run
        // CommandVerified is the exclusive proof.
        VerdictKind::UnitTestVerified | VerdictKind::LiveVerified => {
            bead.ac == AcShape::Checkable
                && !(bead.priority >= 2 && bead.blast == Blast::Normal && runnable)
        }
        // Row 4 — judgment AC: ReviewerSigned only. Recording any other kind
        // for a judgment bead is a false verdict.
        VerdictKind::ReviewerSigned => bead.ac == AcShape::Judgment,
        // Row 5 — never a pass.
        VerdictKind::VerifierBlocked | VerdictKind::VerifierFailed => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNABLE: &str = "cargo test -p beads verify::";
    const NOT_RUNNABLE: &str = "cargo test -p beads verify:: && sleep 3600";

    fn legal(kind: VerdictKind, priority: u8, blast: Blast, ac: AcShape, verify: &str) -> bool {
        legal_close(
            kind,
            &LegalCloseInput {
                priority,
                blast,
                ac,
                verify,
            },
        )
    }

    // ---- ADR-0001 §8.4 named contract tests ----

    #[test]
    fn legal_close_p3_loop_runnable_accepts_only_command_verified() {
        // Row 1: cheap band accepts CommandVerified ONLY.
        for kind in VerdictKind::ALL {
            let want = kind == VerdictKind::CommandVerified;
            assert_eq!(
                legal(kind, 3, Blast::Normal, AcShape::Checkable, RUNNABLE),
                want,
                "kind={kind:?}"
            );
        }
    }

    #[test]
    fn legal_close_p3_loop_runnable_rejects_worker_receipt() {
        // WorkerReceipt on a loop-runnable VERIFY is a forged-proof path.
        assert!(!legal(
            VerdictKind::WorkerReceipt,
            3,
            Blast::Normal,
            AcShape::Checkable,
            RUNNABLE
        ));
        // But legal in the non-runnable band.
        assert!(legal(
            VerdictKind::WorkerReceipt,
            3,
            Blast::Normal,
            AcShape::Checkable,
            NOT_RUNNABLE
        ));
    }

    #[test]
    fn legal_close_p0_rejects_command_verified() {
        // P0/P1 band: independent verification only; CommandVerified is
        // illegal even on a loop-runnable VERIFY.
        assert!(!legal(
            VerdictKind::CommandVerified,
            0,
            Blast::Normal,
            AcShape::Checkable,
            RUNNABLE
        ));
        assert!(legal(
            VerdictKind::UnitTestVerified,
            0,
            Blast::Normal,
            AcShape::Checkable,
            NOT_RUNNABLE
        ));
        assert!(legal(
            VerdictKind::LiveVerified,
            1,
            Blast::High,
            AcShape::Checkable,
            RUNNABLE
        ));
    }

    #[test]
    fn legal_close_judgment_ac_accepts_only_reviewer_signed() {
        for kind in VerdictKind::ALL {
            let want = kind == VerdictKind::ReviewerSigned;
            assert_eq!(
                legal(kind, 0, Blast::High, AcShape::Judgment, ""),
                want,
                "kind={kind:?}"
            );
            assert_eq!(
                legal(kind, 4, Blast::Normal, AcShape::Judgment, RUNNABLE),
                want,
                "kind={kind:?}"
            );
        }
    }

    // ---- Ported loop-runnable examples (flywheel verdict.rs tests) ----

    #[test]
    fn loop_run_classifier_accepts_targeted_checks() {
        assert!(is_loop_runnable("cargo test -p beads verdict::"));
        assert!(is_loop_runnable("cargo test --package beads verifier"));
        assert!(is_loop_runnable("pytest tests/test_gate.py"));
        assert!(is_loop_runnable("python -m pytest tests/test_x.py"));
        assert!(is_loop_runnable("timeout 300 cargo test -p beads verifier"));
        assert!(is_loop_runnable(
            "sh -c \"timeout 60 cargo test -p beads verifier\""
        ));
        assert!(is_loop_runnable("test -f Cargo.toml"));
    }

    #[test]
    fn loop_run_classifier_rejects_heavy_and_unbounded() {
        assert!(!is_loop_runnable("cargo test --workspace"));
        assert!(!is_loop_runnable("cargo test --all-features"));
        assert!(!is_loop_runnable("cargo test --all-targets"));
        assert!(!is_loop_runnable("cargo test"));
        assert!(!is_loop_runnable("cargo test --lib"));
        assert!(!is_loop_runnable("cargo build"));
        assert!(!is_loop_runnable("bash scripts/verify-gates.sh"));
        assert!(!is_loop_runnable("sh -c 'cargo test --workspace'"));
        assert!(!is_loop_runnable("sleep 3600"));
        assert!(!is_loop_runnable("git push"));
        assert!(!is_loop_runnable("npm run e2e"));
    }

    /// The canonical ADR example: composition is never loop-runnable.
    #[test]
    fn loop_run_classifier_cd_and_composition_is_not_loop_runnable() {
        assert!(!is_loop_runnable("cd x && cargo test"));
        assert!(!is_loop_runnable(""));
        assert!(!is_loop_runnable("   "));
        assert!(!is_loop_runnable("cargo test && sleep 3600"));
        assert!(!is_loop_runnable("cargo test || cargo build"));
        assert!(!is_loop_runnable("cargo test | tee out.txt"));
        assert!(!is_loop_runnable("cargo test; echo done"));
        assert!(!is_loop_runnable("cargo test\ncargo build"));
        assert!(!is_loop_runnable("cargo test `echo x`"));
        assert!(!is_loop_runnable("cargo test $(echo x)"));
    }

    // ---- Table oracle over every cell (ported from flywheel) ----

    /// The ADR-0001 §5.3 table, transcribed as a predicate. This is the
    /// oracle the implementation is checked against on every cell.
    fn expected(
        kind: VerdictKind,
        ac: AcShape,
        priority: u8,
        blast: Blast,
        runnable: bool,
    ) -> bool {
        use AcShape::{Checkable, Judgment};
        use Blast::Normal;
        use VerdictKind::*;
        match kind {
            ReviewerSigned => ac == Judgment,
            VerifierBlocked | VerifierFailed => false,
            CommandVerified => ac == Checkable && priority >= 2 && blast == Normal && runnable,
            WorkerReceipt => ac == Checkable && priority >= 2 && blast == Normal && !runnable,
            // Rows 2+3: independent verification is legal for the
            // non-runnable band AND the P0/P1 / High band; illegal only in
            // the cheap band (CommandVerified-exclusive).
            UnitTestVerified | LiveVerified => {
                ac == Checkable && !(priority >= 2 && blast == Normal && runnable)
            }
        }
    }

    /// Every kind x priority(0..=4) x blast x AC-shape x runnability cell
    /// (7 * 5 * 2 * 2 * 2 = 280 cells).
    #[test]
    fn legal_close_matches_table_over_every_cell() {
        for kind in VerdictKind::ALL {
            for priority in 0..=4u8 {
                for blast in [Blast::Normal, Blast::High] {
                    for ac in [AcShape::Checkable, AcShape::Judgment] {
                        for runnable in [true, false] {
                            let verify = if runnable { RUNNABLE } else { NOT_RUNNABLE };
                            let want = expected(kind, ac, priority, blast, runnable);
                            let got = legal(kind, priority, blast, ac, verify);
                            assert_eq!(
                                got, want,
                                "kind={kind:?} priority={priority} blast={blast:?} ac={ac:?} runnable={runnable}"
                            );
                        }
                    }
                }
            }
        }
    }
}
