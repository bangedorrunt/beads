<!-- governed-by: ADR-0004 -->
---
status: accepted
date: 2026-09-06
decision-makers: Captain (bangedorrunt) — accepted 2026-09-07, implementation authorized
consulted: flywheel ADR-0010 D11 (delivery semantics); flywheel D6 OPEN DECISIONS consumer design
informed: flywheel maintainers; toron maintainers
---

# ADR-0005: Scout-ship deliverable types and captain hold

**Status:** Proposed (2026-09-06). Acceptance by the Captain of the beads repo is a separate act.
**Extends:** [ADR-0001](0001-make-beads-the-fail-closed-work-ledger.md), the fail-closed work-ledger decision.
**Constrained by:** [ADR-0004](0004-adopt-revisioned-witnessed-mutations.md), the revisioned witnessed-mutation protocol.

> Flywheel ADR-0010 D11 defers this design to the owning repo: flywheel must not ship a silent
> beads-schema column. This ADR specifies the deliverable contract and the captain hold here,
> where the schema lives. Flywheel D6 consumes the fields when present.

## 1. Decision summary

Every bead carries a typed **deliverable**: `diff` or `report`.

1. A `diff` bead's close gate is code evidence: a legal verdict row plus the commit sha, as today.
2. A `report` bead's close gate is artifact evidence, never `unit-test-verified`: the named
   artifact must exist (file path or `result_path`), and the close witness cites it.
3. `br create --promotes <id>` links a follow-on bead to the bead it promotes, so a scout ship
   graduates into tracked work instead of evaporating.
4. `br hold --kind captain` with a bound `corr` parks a bead behind a captain decision. A
   captain-held row closes only on answer/`--verdict` for that corr.
5. Teardown, kill, and TTL expiry cannot close a captain-held row. Expiry re-surfaces the hold;
   it never drops it.

## 2. Typed deliverable

| deliverable | Meaning | Close gate |
|---|---|---|
| `diff` | The bead ships a code change. | Legal verdict row + commit sha citing the bead id (existing fail-closed close). |
| `report` | The bead ships knowledge: an ADR, a survey, a diagnosis, a decision record. | The artifact exists at its path (or `result_path`); the close witness cites the artifact. `unit-test-verified` is refused for `report` beads: there is no code under test, so a unit-test verdict would be a false witness. |

The deliverable type is set at creation and is immutable afterwards. Changing what a bead owes
means closing it and opening a successor, not retyping it mid-flight.

## 3. Promotes

`br create --promotes <id>` records that the new bead exists because the cited bead's finding
demands follow-on work. The link is advisory, not a dependency edge: it does not gate readiness,
it preserves provenance. A scout report that graduates into implementation carries
`--promotes <report-bead-id>` so the chain from finding to fix stays auditable.

## 4. Captain hold

`br hold --kind captain --corr <corr-id>` marks the bead as awaiting a captain decision:

- The hold binds the bead row to exactly one open corr. One corr, one hold; a second corr on
  the same bead is a second hold row, never a silent overwrite.
- While any captain hold is open, the bead is not closeable by any path: not by verdict, not
  by teardown, not by kill, not by TTL expiry.
- The hold clears only when the bound corr resolves via answer or `--verdict`. Clearing records
  the resolving corr in the bead's event log.
- Flywheel D6 consumes this field: a held bead appears in OPEN DECISIONS until the corr
  resolves, and RECORD DIVERGENCE flags a closed bead with an open corr (or vice versa).

## 5. What this ADR does not do

- No flywheel-only column: flywheel reads `deliverable`, `promotes`, and the captain hold from
  the beads schema; it does not maintain a parallel copy.
- No schema migration in flywheel: migration, if any, lands here under ADR-0004's revisioned
  mutation rules.
- No change to the fail-closed close for `diff` beads: verdict row + sha remains mandatory.

## 6. Acceptance

This ADR takes effect when the Captain of the beads repo marks it accepted. Until then,
flywheel D6 treats the specified fields as absent and keeps its current behavior.
