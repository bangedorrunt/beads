# bv TUI UX Map — Port Contract for Rust/ratatui

Source of truth: `Dicklesworthstone/beads_viewer` (Go, Bubble Tea + lipgloss), `pkg/ui/*.go`, studied at `/tmp/bv_study`.
Purpose: a precise "UI alike" contract for porting the bv TUI to Rust ratatui. Keybindings and view switching are normative; visual details are indicative.

Conventions below:
- Key strings use Bubble Tea spelling: `enter`, `esc`, `tab`, `shift+tab`, `ctrl+d`, `alt+h`, `pgup`/`pgdown`, `home`, `end`, `f1`..`f5`, `space`.
- "Filtering" means the main list's incremental search input is active (`list.FilterState() != Unfiltered`). Most global keys are suppressed while filtering.
- Handler names refer to Go functions in `pkg/ui/model.go` unless noted.

---

## 1. Overall layout

### 1.1 Program-level setup
- Alternate screen (`tea.WithAltScreen`), mouse cell motion enabled (`tea.WithMouseCellMotion`), `tea.WithoutSignalHandler()`; custom SIGINT/SIGTERM watcher: first signal = graceful quit, second signal or 5 s timeout = kill.
- Model is born "ready" with default dimensions 120x40 (no "Initializing..." splash in practice; a loading screen exists only while the first snapshot is pending: `snapshotInitPending && snapshot == nil`).
- Final render: `JoinVertical(body, footer)` clamped to exact terminal `width`/`height`.

### 1.2 Top-level structure
There is **no persistent application title/header bar**. The screen is:

```
+--------------------------------------------------------------+
| body (height-1): one of: full-screen view | split panes |    |
|   single-column list | modal/overlay (replaces body)         |
+--------------------------------------------------------------+
| footer (1 line): status bar                                   |
+--------------------------------------------------------------+
```

Each pane/view draws its own column-header line inside the body. The shortcuts sidebar (see §5) is appended to the right of the body as an extra column when enabled.

### 1.3 Layout thresholds (terminal width)
| Width | Behavior |
|---|---|
| > 180 (UltraWide) | list rows also show label mini-tags |
| > 140 (Wide) | list rows add PageRank sparkline + assignee + labels |
| > 100 (SplitView) | auto split view: list pane + detail pane side by side |
| <= 100 | "mobile": single column; `enter` opens full-screen detail viewport |
- Split pane ratio `splitPaneRatio` default 0.5; `<` shrinks 0.05 (min 0.2), `>` expands (max 0.8).

### 1.4 List pane chrome (single-column and split)
- Header line (1 line, clamped): primary background, white/dark foreground, bold. Text: `"  TYPE PRI STATUS      ID                     TITLE"` (single-column adds more ID padding; workspace mode swaps TYPE for a `REPO` badge column).
- bubbles list (no title, no status bar, no pagination bar, no built-in help; filtering enabled).
- Page info line, right-aligned, secondary color: ` Page %d of %d (items %d-%d of %d) ` (split variant: `Page %d/%d (%d-%d of %d)` centered).
- List row layout (delegate): `[selector 2][repo badge 0-6][type icon 1-2][P0-P4 badge][priority-hint ↑↓][triage ⭐/🔓][status badge OPEN/PROG/BLKD/DEFR/DRFT/PIN/HOOK/REVW/DONE/TOMB][ID][title…][age (width>60)][💬N comments (width>60)][sparkline (width>120)][@assignee (width>100)][label tags (width>140)]`. Selected row: highlight background + bold + thick left border in Primary.
- Diff badges (time-travel mode): NEW / CLOSED / MODIFIED markers colored per row.

### 1.5 Detail pane (viewport, markdown-rendered)
Single `viewport` (bubbles) rendering Glamour markdown built per selected issue:
- `# <type-icon> <title>`
- meta table `| ID | Status | Priority | Assignee | Created |`
- `**Labels:** …` when present
- `### 🎯 Triage Insights` (score with 🔴/🟠/🔵 by 0.7/0.4 thresholds, ⭐ Quick Win, 🔴 Critical Blocker, 🔓 Unblocks N, primary + all reasons)
- `### 🔎 Search Scores` (hybrid mode only, while filtered)
- `### Graph Analysis` (Impact Depth, PR/BW/EV, Hub/Authority)
- `### Description`, `### Design Notes`, dependencies (blockers/blocked-by), comments history
- update banner `⭐ **Update Available:**` at top when an update exists.

### 1.6 Footer / status bar (1 line, `renderFooter`)
Priority: **status message** (if set) replaces the whole bar: `✓ msg` green (open-bg) or `✗ msg` red bold (critical-bg). Status message auto-clears on any keypress.
Otherwise, left→right badges:
1. **Filter badge**: icon + text — `📋 ALL` / `📂 OPEN` / `✅ CLOSED` / `🚀 READY` / `📑 RECIPE-NAME` / `🔍 label:x`; special texts: label dashboard (`🏷️ LABELS: j/k nav • h detail • d drilldown • enter filter`), label graph analysis, label drilldown.
2. **Search-mode badge** while filtering: `🔎 fuzzy` / `🔎 semantic` / `🔎 semantic (indexing)` / `🔎 hybrid/<preset>[ (metrics)]`.
3. **Sort badge** when non-default: `↕ Created ↑|Created ↓|Priority|Updated`.
4. **Context hints** (dim text, varies per view; see §3.9).
5. **Stats**: `○Nopen ◉Nready ◈Nblocked ●Nclosed` (colored). Time-travel mode replaces with `⏱ <rev>: +N ✅N ~N`.
6. **Worker/freshness badges**: `◌ metrics…` (phase 2 pending); spinner `<frame> refreshing` (after 250 ms grace); `⚠ <age> ago`; `⚠ STALE: <age> ago`; `✗ bg <phase> (<N>x)`; `⚠ bg <phase> (<age>)`; `⚠ worker unresponsive`; `↻ recovered xN`.
7. **Watcher badge**: `polling [fstype] [interval]` when fsnotify unavailable.
8. **Update badge**: `⭐ Update <tag>`.
9. **Dataset warning** (tiered perf mode): large/huge warning, critical styling on huge.
10. **Alerts badge**: `⚠ N alerts (!)` (critical>0) / `⚡ N alerts (!)` (warning) / `ℹ N alerts (!)`.
11. **Instance badge**: `⚠ PID <pid>` when a second bv instance holds the lock.
12. **Session badge**: `📎N`/`📎9+` cass coding sessions for the selected bead.
13. **Workspace badge**: `📦 <summary>`; **repo filter badge**: `🗂 a, b, +N`.
14. **Key hints**: context-aware, keys bold, separator ` │ ` (see §3.9).
15. **Count badge**: `N issues` (filtered count).

### 1.7 Modals and overlays (replace the body, priority order in `View()`)
1. quit confirm (centered rounded box: "Quit bv? — Press Esc or Y to quit / Press any other key to cancel", Blocked-red border)
2. AGENTS.md agent prompt modal (centered; 3 buttons Yes/No/Never)
3. cass session preview modal (centered)
4. self-update modal (centered, progress states)
5. label health detail (from label dashboard `h`)
6. label graph analysis (from label drilldown `g`)
7. label drilldown (from label dashboard `d`)
8. alerts panel
9. time-travel revision input prompt
10. recipe picker overlay
11. repo picker overlay (workspace mode)
12. label picker overlay (has always-focused text input)
13. help overlay (`?`/`f1`)
14. tutorial overlay (full screen, backtick)
15. loading screen (initial snapshot pending)

---

## 2. The view/focus model

### 2.1 `focus` enum (keyboard focus owner) — model.go
```
focusList, focusDetail, focusBoard, focusGraph, focusTree,
focusLabelDashboard, focusInsights, focusActionable, focusRecipePicker,
focusRepoPicker, focusHelp, focusQuitConfirm, focusTimeTravelInput,
focusHistory, focusAttention, focusLabelPicker, focusSprint,
focusAgentPrompt, focusFlowMatrix, focusTutorial, focusCassModal,
focusUpdateModal
```
(`focusAttention` is declared but the attention view actually runs inside insights with `showAttentionView=true`.)

### 2.2 View flags (mutually exclusive "one view at a time")
`isBoardView`, `isGraphView`, `isActionableView`, `isHistoryView`, `isSprintView`, plus `focused == focusTree / focusInsights / focusLabelDashboard / focusFlowMatrix` (these are entered by setting `focused` directly and clearing the four flags). Opening any view clears the other view flags. Closing a view returns `focused = focusList`. `isSplitView` is automatic (width > 100) and independent. `showAttentionView` is a sub-state of insights.

### 2.3 Context enum for help/UX (context.go) — derived, priority order
Overlays first: `label-picker, recipe-picker, help, quit-confirm, label-health-detail, label-drilldown, label-graph-analysis, time-travel-input, alerts, repo-picker, agent-prompt, cass-session`.
Views: `insights (or attention sub-view), flow-matrix, label-dashboard, graph, board, actionable, history, sprint`.
Detail states: `time-travel, split, detail`.
Then `filter` (list search active), default `list`.

### 2.4 There is no tab bar
No top-level tabs. "Switching views" = single-key toggles evaluated in `Update()` **after** per-view key handlers, so unclaimed keys fall through from any view to the view-toggle block (cross-view switching, e.g. `b` while in graph opens board). `q`/`esc` close the current view layer by layer; at the top list, first `esc` clears filters, second opens quit confirm; `q` at top quits (no confirm).

### 2.5 View-opening keys (authoritative switch map)
| Key | Opens / toggles | Sets | Notes |
|---|---|---|---|
| `b` | Kanban board | `isBoardView`, `focused=focusBoard` | toggle; clears graph/actionable/history; refreshes board data for current filter |
| `g` | Dependency graph | `isGraphView`, `focused=focusGraph` | toggle (see §4 gg combo) |
| `a` | Actionable (execution plan) | `isActionableView`, `focused=focusActionable` | builds plan from analyzer |
| `h` | History view | `isHistoryView`, `focused=focusHistory` | toggle |
| `i` | Insights panel | `focused=focusInsights` | toggle (q/esc return to list) |
| `E` | Hierarchical tree | `focused=focusTree` | toggle; builds from snapshot |
| `[` / `f3` | Label dashboard | `focused=focusLabelDashboard` | computes/caches label health |
| `]` / `f4` | Attention view | `focused=focusInsights` + `showAttentionView=true` | renders attention text as insights extra text |
| `f` | Flow matrix | `focused=focusFlowMatrix` | computes cross-label flow |
| `'` | Recipe picker overlay | `focused=focusRecipePicker` | toggle |
| `w` | Repo picker overlay | `focused=focusRepoPicker` | workspace mode only |
| `l` | Label picker overlay | `focused=focusLabelPicker` | list context only |
| `!` | Alerts panel | overlay | only if non-dismissed alerts exist |
| `?` / `f1` | Help overlay | `focused=focusHelp` | stores `focusBeforeHelp` for restore |
| `` ` `` | Tutorial overlay | `focused=focusTutorial` | closes help if open |
| `p` | Priority hints column | (not a view) | toggles ↑/↓ hints in list rows |
| `t` / `T` | Time-travel mode | `timeTravelMode` | `t` prompts for revision; `T` = HEAD~5 quick |
| — | **Sprint view** | `isSprintView` | **no key opens it in current code** — vestigial; handlers (`P`/`esc`/`j`/`k`) exist, only tests reach it. Port decision: either wire a key (docs suggest P) or drop. |

### 2.6 Focus-restore rules
- Help: `focusBeforeHelp` saved; dismissal restores insights/label-dashboard/sprint/flow-matrix/board/graph/tree etc. correctly (any unrecognized key also closes help).
- Tutorial closes back to `focusList`.
- All pickers/modals close to `focusList`.
- Board `enter` and graph `enter` and history `enter` and actionable `enter`: select the issue in the main list, close the view, open detail (or focus detail pane in split view).

---

## 3. Complete keybinding tables

Dispatch order in `Update` (each stage consumes or falls through):
1. Modal/overlay interceptors (agent prompt, cass, update modal, label health detail, label drilldown, label graph analysis, attention, alerts, repo picker, recipe picker, quit confirm).
2. Global toggles (when not filtering): `?`/`f1` help, `` ` `` tutorial, `ctrl+r`/`f5` force refresh, `;`/`f2` shortcuts sidebar, `ctrl+j`/`ctrl+k` sidebar scroll.
3. List-context specials (focused==list, not filtering): `H` hybrid toggle, `alt+h`/`alt+H` hybrid preset, `ctrl+s` semantic toggle.
4. Help/tutorial/time-travel-input/board-search/history-search-or-filetree/label-picker submodes capture all keys.
5. **Truly global keys** (not filtering): `ctrl+c`, `q`, `esc`, `tab`, `<`, `>`.
6. **Focus-specific handlers** (each claims only its keys; others fall through).
7. **View-toggle block** (§2.5).
8. `handleListKeys` (remaining list keys).
9. bubbles list gets everything else when `focused==focusList` (including filter input).

### 3.0 Truly global keys (all views, when not filtering)
| Key | Action |
|---|---|
| `ctrl+c` | quit immediately (works even inside pickers/search submodes) |
| `q` | close current thing: detail (non-split) → insights → flow-matrix drilldown → flow-matrix → graph → board → actionable → history → label picker → label dashboard → tree → sprint; else quit |
| `esc` | same closing chain; at top list: clear all filters if active, else show quit-confirm; in split view does NOT move focus (use tab) |
| `tab` | split view only (and not board view): toggle `focusList ↔ focusDetail` |
| `<` | split view: shrink list pane 5% (min 20%) |
| `>` | split view: expand list pane 5% (max 80%) |

### 3.1 Global toggles (all views, when not filtering)
| Key | Action |
|---|---|
| `?` or `f1` | toggle help overlay |
| `` ` `` | toggle interactive tutorial |
| `;` or `f2` | toggle shortcuts sidebar (reflows body; status msg `Shortcuts sidebar: ; hide \| ctrl+j/k scroll`) |
| `ctrl+j` / `ctrl+k` | scroll shortcuts sidebar (when visible) |
| `ctrl+r` or `f5` | force refresh via background worker (1 s debounce; status `Refreshing…`) |

### 3.2 List view (`focusList`) — `handleListKeys` + view toggles
| Key | Description | Category |
|---|---|---|
| `j` / `k` / arrows | move selection (bubbles list) | Navigation |
| `g` / `G` / `home` / `end` | jump to start / end (bubbles list: `home`/`g` start, `end`/`G` end; `G`,`end`,`home` re-implemented in handleListKeys) | Navigation |
| `ctrl+d` / `ctrl+u` | page down / up by `height/3` | Navigation |
| `enter` | open details (non-split only): `showDetails=true`, focus detail, viewport to top | Navigation |
| `/` | start incremental fuzzy/semantic filter (bubbles list; filtering suppresses global keys) | Filters |
| `o` | filter: open issues only | Filters |
| `c` | filter: closed issues only | Filters |
| `r` | filter: ready (open+in_progress, no open blockers; excludes blocked/draft/deferred) | Filters |
| `l` | open label picker overlay | Filters |
| `s` | cycle sort mode: Default → Created ↑ → Created ↓ → Priority → Updated | Sort |
| `S` | apply "triage" recipe (sort by triage score) | Sort |
| `'` | recipe picker overlay | Actions |
| `w` | repo picker overlay (workspace mode) | Actions |
| `b` `g` `a` `h` `i` `E` `[` `]` `f` | open views (§2.5) | Views |
| `p` | toggle priority hints column | Views |
| `!` | alerts panel | Views |
| `t` | time-travel: exit mode if active, else show revision input prompt (default HEAD~5 if submitted empty) | Actions |
| `T` | time-travel quick: toggle mode / HEAD~5 | Actions |
| `x` | export current view to markdown file | Actions |
| `y` | copy selected issue ID to clipboard | Actions |
| `C` | copy full selected issue to clipboard | Actions |
| `O` | open selected issue in `$EDITOR`; on exit diff frontmatter and apply via `br update` | Actions |
| `V` | cass session preview modal for selected bead | Actions |
| `U` | self-update modal (check + install) | Actions |
| `H` | toggle hybrid search ranking (list focus only) | Search |
| `alt+h`/`alt+H` | cycle hybrid search preset | Search |
| `ctrl+s` | toggle semantic search index (list focus only) | Search |
- Note: `handleListKeys` contains dead branches for `a` (filter all) and `h` (history) shadowed by the view-toggle block — unreachable; the port should pick one owner per key.
- While filtering (bubbles defaults): typing filters; `enter`/`tab` accept; `esc` cancels (or clears applied filter); `ctrl+n`/`ctrl+p` or `up`/`down` navigate matches; `n`/`N` next/prev match after filter applied. Search badge shows fuzzy/semantic/hybrid mode.

### 3.3 Detail view (`focusDetail`)
| Key | Description |
|---|---|
| `j`/`k`/`up`/`down` | scroll viewport |
| `ctrl+d` / `ctrl+u` | half-page scroll |
| `pgup` / `pgdown` | page scroll |
| `home`/`g`, `end`/`G` | top / bottom |
| `O` | open in `$EDITOR` (intercepted before viewport) |
| `esc` / `q` | back to list |
| `C` | copy full issue (via list handler path when not split) |
| `y` | copy ID |
| `x` | export markdown |
- In split view, `tab` flips back to list. `q`/`esc` in split view do nothing (only non-split closes).

### 3.4 Board view (`focusBoard`) — `handleBoardKeys`
Normal mode:
| Key | Description |
|---|---|
| `h`/`l` or `left`/`right` | previous / next column |
| `j`/`k` or `down`/`up` | move down / up within column |
| `home` | top of column; `G`/`end` bottom of column |
| `0` / `$` | first / last item in column (vim) |
| `1` `2` `3` `4` | jump to column Open / In Progress / Blocked / Closed |
| `H` / `L` | jump to first / last column |
| `ctrl+d` / `ctrl+u` | page down / up (height/3) |
| `gg` (200 ms window) | move to top |
| single `g` (timeout) | switch to graph view |
| `/` | start card search mode |
| `n` / `N` | next / previous search match (when matches exist) |
| `y` | copy selected card's issue ID |
| `o` / `c` / `r` | filter open / closed / ready (same as list) |
| `s` | cycle swimlane mode: Status → Priority → Type |
| `e` | cycle empty-column visibility (auto/show-all/hide) |
| `d` | toggle inline card expansion |
| `tab` | toggle detail panel |
| `ctrl+j` / `ctrl+k` | scroll detail panel down / up (when shown) |
| `enter` | jump to card's issue: select in list, exit board, open detail |
Search mode (all keys consumed):
| Key | Description |
|---|---|
| printable chars | append to query |
| `backspace` | delete char |
| `enter` | finish search (keep results) |
| `esc` | cancel search |
| `n` / `N` | next / prev match |

### 3.5 Graph view (`focusGraph`) — `handleGraphKeys`
| Key | Description |
|---|---|
| `h`/`l`/`j`/`k` or arrows | move left/right/down/up through node list |
| `H` / `L` | scroll viewport left / right |
| `ctrl+d`/`pgdown`, `ctrl+u`/`pgup` | page through nodes |
| `enter` | select issue in main list, exit graph, open detail |
- Layout: left node list (28/24 cols) with status icons; right visual graph + metric ranks; narrow (<80) shows graph only.

### 3.6 Tree view (`focusTree`) — `handleTreeKeys`
| Key | Description |
|---|---|
| `j`/`k` or arrows | move down / up |
| `enter` / `space` | toggle expand/collapse |
| `h`/`left` | collapse or jump to parent |
| `l`/`right` | expand or move to child |
| `G` | jump to bottom |
| `o` / `O` | expand all / collapse all |
| `ctrl+d`/`pgdown`, `ctrl+u`/`pgup` | page down / up |
| `E` / `esc` | return to list |
| `tab` | sync selection and jump to detail pane (split view) |
| `gg` combo / single `g`→graph | same 200 ms combo mechanism as board |

### 3.7 Insights panel (`focusInsights`) — `handleInsightsKeys`
Panels (cycle order): Bottlenecks 🚧, Keystones 🏛️, Influencers 🌐, Hubs 🛰️, Authorities 📚, Cores, Articulation, Slack, Cycles, Priority.
| Key | Description |
|---|---|
| `h`/`left` | previous panel |
| `l`/`right`/`tab` | next panel |
| `j`/`k` or arrows | move down / up in item list |
| `ctrl+j` / `ctrl+k` | scroll detail pane down / up |
| `e` | toggle metric explanations (what/why/how/formula) |
| `x` | toggle calculation details |
| `m` | toggle heatmap view |
| `enter` | jump to selected issue in list/detail |
| `esc` | back to list |
- Attention sub-view (`]`): `esc`/`q`/`d` close; `1`-`9` quick-filter to ranked label (`label:<name>`).

### 3.8 History view (`focusHistory`) — `handleHistoryKeys`
Two modes: **bead mode** (beads left, commits right) and **git mode** (`v`; commits left, related beads right). Optional third pane: file tree (`f`/`F`).
Normal:
| Key | Description |
|---|---|
| `v` | toggle bead/git mode |
| `j`/`k` or arrows | navigate left list (mode-aware) |
| `J` / `K` | git mode: next/prev related bead; bead mode: next/prev commit |
| `tab` | cycle focus: list → detail → file tree (if visible) → list |
| `enter` | jump to selected bead in main list → detail |
| `y` | copy selected commit SHA |
| `c` | cycle commit-confidence threshold (bead mode) |
| `o` | open commit in browser (needs git remote) |
| `g` | jump to graph view centered on selected bead |
| `f` / `F` | toggle file tree panel |
| `/` | start search |
| `h` / `esc` | exit history view |
Search active (all keys consumed): printable input; `enter` finish (keep filter); `esc` cancel.
File tree focused (all keys consumed):
| Key | Description |
|---|---|
| `j`/`k` | move down / up |
| `enter` / `l` | expand dir / select file (filters) |
| `h` | collapse dir |
| `esc` | clear file filter, else leave tree |
| `tab` | return focus to panes |

### 3.9 Context key-hint strings (footer)
- list (default): `⏎ details │ t diff │ S triage │ l labels │ Ctrl+R refresh │ ? help` (+`w repos` in workspace mode)
- split: `tab focus │ C copy │ x export │ Ctrl+R refresh │ ? help`
- detail: `esc back │ C copy │ O edit │ Ctrl+R refresh │ ? help`
- time-travel: `t exit diff │ C copy │ abgi views │ ? help`
- filtering: `esc cancel │ ctrl+s <mode> │ ⏎ select` (+`H hybrid │ alt+h preset` when semantic)
- graph: `hjkl nav │ H/L scroll │ ⏎ view │ g list`
- board: `hjkl nav │ G bottom │ ⏎ view │ b list`; search mode: `/<query> [i/n] │ n/N:match │ enter:done │ esc:cancel`
- actionable: `j/k nav │ ⏎ view │ a list │ ? help`
- history: `j/k nav │ tab focus │ ⏎ jump │ H close`
- insights: `h/l panels │ e explain │ ⏎ jump │ ? help │ A attention │ F flow` (footer label variants exist)
- flow matrix: `j/k nav │ tab panel │ ⏎ drill │ esc back │ f close`
- label dashboard (filter badge): `LABELS: j/k nav • h detail • d drilldown • enter filter`
- attention: `A:attention • 1-9 filter • esc close`
- help: `Press any key to close`

### 3.10 Actionable view (`focusActionable`)
| Key | Description |
|---|---|
| `j`/`k` or arrows | move down / up |
| `enter` | jump to selected issue in list/detail |
| `a` | close (view toggle) |

### 3.11 Flow matrix (`focusFlowMatrix`) — `handleFlowMatrixKeys`
| Key | Description |
|---|---|
| `j`/`k` or arrows | move down / up |
| `tab` | toggle panel (matrix ↔ drilldown list focus) |
| `enter` | drilldown on selected label; in drilldown: jump to selected issue in list/detail |
| `g` / `home` | go to start |
| `G` / `end` | go to end |
| `f` / `q` / `esc` | close drilldown first, then close view |

### 3.12 Label dashboard (`focusLabelDashboard`) — own Update + model.go intercepts
| Key | Description |
|---|---|
| `j`/`k`, arrows, `home`/`G`/`end` | navigate table (scroll-aware) |
| `enter` | filter main list by selected label (`label:x`) and return |
| `h` | open label health detail modal |
| `d` | open label drilldown overlay |
- Table columns: Label, Health, Blocked, Velocity 7d/30d, Stale; sorted critical → warning → ok, then blocked desc, health asc, name.

### 3.13 Pickers and modals
**Label picker** (overlay, input always focused; keys consumed before globals):
| Key | Description |
|---|---|
| typing | fuzzy-filter label list |
| `j`/`down`/`ctrl+n`, `k`/`up`/`ctrl+p` | navigate |
| `enter` | apply `label:<sel>` filter |
| `esc` | cancel (also global `q` closes it) |

**Recipe picker**: `j`/`k` nav; `enter` apply recipe (sets filter `recipe:<name>` + sort); `esc` cancel.

**Repo picker** (workspace): `j`/`k` nav; `space` toggle repo; `a` select all; `enter` apply (empty=all); `esc`/`q` cancel.

**Quit confirm**: `esc`/`y`/`Y` quit; any other key cancels (note: `q` therefore cancels).

**Alerts panel**: `j`/`down`, `k`/`up` navigate; `enter` jump to alert's issue (select in list, close); `d` dismiss selected (close when none left); `esc`/`q`/`!` close.

**Time-travel input**: text input; `enter` submit (empty → `HEAD~5`); `esc` cancel; requires git repo + beads at revision; builds diff snapshots, adds NEW/CLOSED/MODIFIED badges; `t` exits the mode.

**Label health detail modal**: `esc`/`q`/`enter`/`h` close; `d` open drilldown for that label.

**Label drilldown overlay**: `enter` filter list by the label; `g` compute + show label graph analysis sub-view (subgraph PageRank + critical path); `esc`/`q`/`d` close.

**Label graph analysis**: `esc`/`q`/`g` close.

**Agent prompt modal** (AGENTS.md blurb): `h`/`l`/`left`/`right`/`tab`/`shift+tab` select button; `enter`/`space` confirm; `y`/`Y` accept; `n`/`N` decline; `d`/`D` never ask; `esc`/`q` decline.

**Cass session modal**: internal `j`/`down`, `k`/`up`, `y` open session; dismissed by `V`/`esc`/`enter`/`q`.

**Self-update modal**: `enter` confirm/close when complete; `n`/`N` cancel while confirming; `esc`/`q` close unless install in progress.

**Help overlay** (`?`): scrollable multi-panel reference (see §5.2).

**Tutorial overlay**: `esc`/`q` close; `t` toggle TOC; `tab` switch content↔TOC (or next page when TOC hidden); content: `l`/`n`/`right`/`space` next page, `h`/`p`/`left`/`shift+tab` prev, `j`/`k` scroll, `ctrl+d`/`ctrl+u` half page, `g`/`home` top, `G`/`end` bottom, `1`-`9` jump to page; TOC: `j`/`k`, `g`/`home`, `G`/`end`, `enter`/`space` select, `h`/`left` back to content.

### 3.14 Registered-docs table (keybindings.go `GetKeyBindingDocs`)
The registry mirrors the doc table below (registry is authoritative for the shortcuts sidebar; runtime dispatch is the Update chain above). Contexts: `all`, `list`, `detail`, `board`, `graph`, `insights`, `history`, `actionable`, `label`, `tree`, `flow`, `sprint`.

| Key | Desc | Category | Context(s) |
|---|---|---|---|
| j | Move down | Navigation | all |
| k | Move up | Navigation | all |
| G | Go to end | Navigation | all |
| gg | Go to start | Navigation | all |
| ctrl+d | Page down | Navigation | all |
| ctrl+u | Page up | Navigation | all |
| enter | Open details | Navigation | all |
| esc | Back/close | Navigation | all |
| q | Quit | Navigation | all |
| a | Actionable view | Views | list,detail |
| b | Board view | Views | list,detail |
| g | Graph view | Views | list,detail |
| h | History view | Views | list,detail |
| i | Insights panel | Views | list,detail |
| ? | Help overlay | Views | all |
| ; | Shortcuts sidebar | Views | all |
| p | Priority hints | Views | list,detail |
| o | Open issues only | Filters | list |
| c | Closed issues only | Filters | list |
| r | Ready (unblocked) | Filters | list |
| l | Label picker | Filters | list |
| / | Search/filter | Filters | list |
| t | Time travel (forward) | Actions | list,detail |
| T | Time travel (back) | Actions | list,detail |
| x | Export to markdown | Actions | list,detail |
| y | Copy issue ID | Actions | all |
| C | Copy full issue | Actions | detail |
| O | Open in $EDITOR | Actions | detail |
| ' | Recipe picker | Actions | list |
| U | Self-update check | Actions | all |
| V | Cass sessions | Actions | list |
| hjkl | Navigate graph | Graph | graph |
| H | Scroll left | Graph | graph |
| L | Scroll right | Graph | graph |
| PgUp | Scroll up | Graph | graph |
| PgDn | Scroll down | Graph | graph |
| h | Previous column | Board | board |
| l | Next column | Board | board |
| tab | Toggle detail | Board | board |
| ctrl+j | Scroll detail down | Board | board |
| ctrl+k | Scroll detail up | Board | board |
| h | Previous panel | Insights | insights |
| l | Next panel | Insights | insights |
| e | Toggle explanations | Insights | insights |
| x | Calculation proof | Insights | insights |
| m | Heatmap toggle | Insights | insights |
| v | Toggle git/bead mode | History | history |
| tab | Toggle focus | History | history |
| J | Detail scroll down | History | history |
| K | Detail scroll up | History | history |
| o | Open in browser | History | history |

---

## 4. Navigation idioms (port these exactly)
- **vim motion**: `j`/`k` (or arrows) everywhere; `h`/`l` for horizontal (columns, panels, tree depth); `g`/`G`/`home`/`end` start/bottom; `ctrl+d`/`ctrl+u` pages (list: height/3); `pgup`/`pgdown` in graph/tree.
- **gg combo (board/tree only)**: first `g` starts a 200 ms timer (`comboTimeout`); second `g` within window = jump to top; timer expiry (comboTickMsg) with same pending key and same focus = single `g` = toggle graph view. Any other key clears pending combo.
- **Enter = drill-down**: every list-like view's `enter` selects the item in the main list and opens the detail view (or focuses the detail pane in split view). Never modifies data.
- **esc/q = pop**: layered close order (see §3.0). At the top list, `esc` = clear filters first, then quit-confirm; `q` = quit directly.
- **`/` search**: main list (fuzzy default, semantic with ctrl+s, hybrid with H); board cards; history. All search submodes consume all keys; `n`/`N` cycle matches (board/list).
- **`?` help + `;` sidebar** always available (not while filtering).
- **tab = focus rotation**: split list↔detail; board detail toggle; insights next panel; history list→detail→file tree; flow matrix panel toggle; tutorial TOC; help `tab` also closes (any key).
- **Number keys jump**: board `1-4` columns; attention `1-9` label filters; tutorial `1-9` pages.
- **Shift variants = "other axis"**: `H`/`L` big jumps (board first/last column; graph scroll), `J`/`K` cross-pane nav (history), `T` vs `t` (quick vs prompted time travel), `F` alias `f`, `O` vs `o` (editor vs browser/expand-all), `S` vs `s` (triage recipe vs sort cycle).
- **Clipboard**: `y` copies ID (list/board), `C` copies full issue (detail), history `y` copies commit SHA.
- **`$`/`0`**: board last/first item in column.

---

## 5. Shortcuts sidebar, help overlay, tutorial, context help

### 5.1 Shortcuts sidebar (`;` or `f2`) — bv-3qi5
- Fixed width 34, appended as right column (`JoinHorizontal(body, sidebar)`); body panes reflow via `applyContentSizing()` so nothing overflows.
- Content auto-generated from the KeyRegistry for the current focus (`AllBindingsForFocus`), grouped by Category in registration order; falls back to hardcoded sections per context (Navigation / Views / Graph / Insights / History / Board / Filters / Actions) when registry empty.
- Scrollable: `ctrl+j`/`ctrl+k` line scroll; reset to top on open; opening sets status message `Shortcuts sidebar: ; hide | ctrl+j/k scroll`.
- Stays visible while working (unlike the help overlay). Focus never enters the sidebar.

### 5.2 Help overlay (`?` / `f1`)
- Full-body multi-column panel layout: 3 columns ≥120 wide, 2 ≥80, else 1; min column width 28.
- Panels (icon + colored rounded border, per-panel color from a 6-color gradient): Navigation 🧭, Views 👁 (b/g/i/h/a/f/[/]), Global 🌐 (?, ;, !, ', w, q, ctrl+c), Filters & Sort 🔍 (/, ctrl+s, H, alt+h, o, c, r, l, s, S), Graph View 📊, Insights 💡, Status 🩺 (badge legend), History 📜, Actions ⚡ (p, ctrl+r, f5, t, T, x, C, O).
- Title bar + subtitle (`space → interactive tutorial`). Scroll with j/k/ctrl+d/ctrl+u/g/G; `space` opens tutorial; any other key closes and restores previous focus.

### 5.3 Tutorial (`` ` ``) and context help
- Full-screen markdown tutorial (Glamour), page-based with TOC sidebar (`t`), progress tracking, context-filtered pages (each context maps to recommended page ids, context.go `TutorialPages`).
- Entry points wired today: `` ` `` direct toggle; `space` inside help overlay.
- **`~` context-help key and the CapsLock double-tap tracker exist in capslock.go but are NOT wired into Update** (CapsLock is intercepted by most terminals; the file documents this). Port note: implement `` ` `` + help-space; treat `~` as optional.

---

## 6. Theme & styles (palette structure)

### 6.1 Theme system
- `Theme` struct wraps a lipgloss `Renderer` plus `AdaptiveColor` fields (each has Light/Dark hex). Dark variant is the Dracula palette; light variant is WCAG-AA-tuned.
- Background assumption: auto-detected, overridable by `BV_THEME=light|dark` env or `--theme` flag via `SetThemeOverride` (pins the global renderer).
- Color-profile gating (colorprofile.Detect, once at init):
  - `ThemeBg(hex)`: truecolor only, else `NoColor` (use terminal bg).
  - `ThemeFg(hex)`: ANSI256+ gets hex; 16-color gets ANSI 7.

### 6.2 Core palette (styles.go design tokens; dark values)
| Token | Light / Dark |
|---|---|
| Bg | #FFFFFF / #282A36 |
| BgDark | #F5F5F5 / #1E1F29 |
| BgSubtle | #E8E8E8 / #363949 |
| BgHighlight | #D0D0D0 / #44475A |
| Text | #1A1A1A / #F8F8F2 |
| Subtext | #555555 / #BFBFBF |
| Muted | #666666 / #6272A4 |
| Primary (purple) | #6B47D9 / #BD93F9 |
| Secondary | #555555 / #6272A4 |
| Info (cyan) | #006080 / #8BE9FD |
| Success (green) | #007700 / #50FA7B |
| Warning (orange) | #B06800 / #FFB86C |
| Danger (red) | #CC0000 / #FF5555 |

### 6.3 Semantic color groups
- **Status**: open green, in_progress cyan, blocked red, deferred/draft orange, pinned blue #6699FF, hooked teal #00CED1, review purple, closed gray, tombstone #44475A. Each has a paired *Bg* token for badges (dark bgs like #1A3D2A, #3D1A1A…).
- **Priority**: P0 critical red, P1 high orange, P2 medium #F1FA8C, P3 low green, P4 muted; paired Bg tokens. Badges render as bold `P0`..`P4` fg-on-bg.
- **Type**: bug red 🐛, feature orange ✨, task yellow 📋, epic purple 🚀, chore cyan 🧹 (epic intentionally avoids emoji with variation selectors).
- **Footer** (higher contrast): FooterHint #C8C8D0, FooterKey #E0E0E8 (bold), FooterSep #8888A0, FooterDim #A0A0B8 (dark values).

### 6.4 Panel & component styles
- `PanelStyle`: rounded border, border = BgHighlight. `FocusedPanelStyle`: rounded border, border = Primary. (Split view indicates focus by border color.)
- Selected list row: BgHighlight background + bold + thick left border in Primary.
- Badges: priority (`P0`-`P4`), status (`OPEN PROG BLKD DEFR DRFT PIN HOOK REVW DONE TOMB`), repo (`[API]`).
- Metrics: sparkline (5 chars) + heatmap color by score; `RenderMiniBar` `█░` bars colored by quartile (≥0.75 green, ≥0.5 orange, ≥0.25 cyan, else muted); `RenderRankBadge` `#N` colored by percentile (≤10% green, ≤25% cyan, ≤50% orange, else muted).
- Dividers: `─` line in BgHighlight; subtle `·` dots in Muted. Spacing tokens XS..XL = 1..6.
- Quit/help modals: rounded border; quit confirm uses Blocked red.
- Help overlay panel colors cycle a 6-color gradient: #BD93F9, #FF79C6, #8BE9FD, #50FA7B, #FFB86C, #F1FA8C.

---

## 7. Background worker & snapshot model (brief)

- **Two-phase analysis** (`analysis.Analyzer.AnalyzeAsync`):
  - Phase 1 (synchronous, instant): build graph, out/in-degree, topological order, density. Available the moment the model is constructed (`NewModel` calls `AnalyzeAsync` before first render).
  - Phase 2 (background goroutine, per-metric timeouts): PageRank, betweenness, eigenvector, HITS hubs/authorities, critical path, cycles, k-core, articulation points, slack. Results surface via `Phase2ReadyMsg`; UI swaps in a new immutable snapshot (`snapshot.WithPhase2`) and refreshes list items (scores, triage) without rebuilding the filtered set. Footer shows `◌ metrics…` until `IsPhase2Ready()`.
- **DataSnapshot**: immutable, self-contained render payload (issues + map, analyzer/stats, counts, list items with scores, triage maps, tree roots, board columns per swimlane mode, graph layout with rank tables, insights). UI thread only reads the current pointer; worker publishes new snapshots by atomic pointer swap. Incremental list rebuild when changed-issue ratio ≤ 0.2.
- **BackgroundWorker** (optional, `BV_BACKGROUND_MODE`): owns the file watcher (fsnotify with polling fallback — footer badge `polling [fstype] [interval]`), debounces + coalesces file-change events, content-hash dedup (skip identical rebuilds), rebuilds snapshots off-thread, watchdog + heartbeat health (footer: unresponsive/recovered/error badges), records activity on every key/mouse event (idle GC). `ForceRefresh()` bypasses dedup (ctrl+r/f5, 1 s UI-side debounce).
- **Dataset tiers** (bv-9thm): small <1k, medium <5k, large <20k, huge ≥20k. Large disables precomputation (triage/tree/board/graph layout/insights computed on demand); huge also skips Phase 2, may load open-issues-only, and shows a footer warning badge.
- Watch sources beyond the worker: legacy in-process watcher path (sync reload) when background mode is unavailable; `FileChangedMsg` triggers reload; sprints and history reload alongside.

## 8. Mouse support level
- Program runs with `WithMouseCellMotion` (motion + release events delivered).
- **Wheel up/down**: routed per focus — list (move selection ±1, sync split detail), detail (viewport scroll 3), insights (MoveUp/Down), board, graph (PageUp/Down), tree, actionable, history, flow matrix.
- **Left click (press only)**: ignored while any overlay is open; no-op in full-screen views (insights/flow/tree/graph/board/actionable/history/sprint/label-dashboard). In split view: click left pane (width ≤ listInnerWidth+4) focuses list and selects the row under the cursor (row = y − listChromeLines: border + header + filter bar); click right pane focuses detail. Single-column list: focuses list and selects row.
- No right-click, no drag, no hover, no mouse in modals/pickers.

---

## 9. Port checklist (UI-alike acceptance)
1. Implement the two-layer dispatch: global keys → focus handler → view toggles → list handler, with the "not filtering" gate and modal interceptors in the documented order.
2. Reproduce the focus/view-flag duality exactly (including `q`/`esc` close chains and help focus restore).
3. Single-key view toggles + cross-view fall-through are the "tab switching" contract; no tab bar.
4. gg 200 ms combo in board/tree; single-g → graph on timeout.
5. Footer badge order & context hint strings; status message replaces the bar; auto-clear on keypress.
6. Split view at >100 cols with `<`/`>` resize and border-color focus indication; tab toggle except board.
7. Shortcuts sidebar (34 cols, right, registry-driven, ctrl+j/k scroll) and the help overlay panels.
8. Adaptive Dracula/light palette with profile gating; badge components.
9. Two-phase metrics with `◌ metrics…` indicator and Phase2 refresh.
10. Mouse: wheel per focus + left-click row selection in list/split only.
11. Decide sprint view fate (unreachable in Go today) and dead `a`/`h` branches in handleListKeys.
