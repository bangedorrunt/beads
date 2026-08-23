# Architecture Decision Records (ADR)

This directory records the decisions that govern the `bangedorrunt/beads` fork.

An ADR is an executable spec for coding agents. A human accepts the decision; an agent implements it. Read accepted ADRs before changing storage, close/ready/gate behavior, the CLI surface, or the relationship to flywheel and toron.

## Conventions

- Directory: `docs/decisions/`
- Filename: `NNNN-title-with-dashes.md` (zero-padded, present-tense verb phrase)
- Status: `proposed` | `accepted` | `rejected` | `deprecated` | `superseded`
- Code that implements a constraint carries `// governed-by: ADR-NNNN` at the file top
- This fork does **not** inherit Dicklesworthstone/beads_rust feature ADRs. Those are upstream history.

## Workflow

1. New decision starts as `proposed`.
2. Captain marks it `accepted` (or `rejected`).
3. Replacement creates a new ADR and marks the old one `superseded` with a two-way link.

## ADRs

| ID | Title | Status |
| :--- | :--- | :--- |
| [0001](0001-make-beads-the-fail-closed-work-ledger.md) | Make beads the fail-closed work-ledger for flywheel × toron | **Accepted** |
| [0002](0002-replace-fsqlite-asupersync-with-rusqlite-and-strip-platform-surface.md) | Replace fsqlite/asupersync with rusqlite and strip platform surface | **Accepted** |
| [0003](0003-absorb-beads-viewer-into-br.md) | Absorb beads_viewer (bv) into br — robot commands, graph analysis, and TUI | **Proposed** |

