---
status: planned
created: 2026-03-20
priority: high
tags:
- visualization
- frontend
- signals
- sidebar
depends_on:
- 014-viz-infrastructure
---

# V3 Signal Feed + V8 File Detail Sidebar

> **Status**: planned · **Priority**: high · **Created**: 2026-03-20

## Overview

The right sidebar contains two coupled views that provide context alongside the main canvas:

1. **V8: File Detail Panel** (top) — answers "How risky is this specific file?" with a focused metrics dashboard for the selected node
2. **V3: Signal Feed** (bottom, scrollable) — answers "What are the most dangerous anomalies?" with a prioritized, filterable list of cross-layer signals

Both views react to the global `selectedNode` state and provide click targets that update selection and drive the main canvas views.

## Design

### V8: File Detail Panel

Displayed at the top of the right sidebar. Shows metrics for the currently selected node. Empty state when no node is selected ("Click a file to see details").

**Section A — Identity**:

```
src/auth/login.py
auth/ · Python · LOC 380 · Fan-in 3 · Fan-out 5 · Bugs 4
```

**Section B — Risk Metrics** (horizontal bar charts):

| Metric                | Max reference       | Color         |
|-----------------------|---------------------|---------------|
| Hotspot Score         | 1.0                 | YlOrRd mapped |
| Cyclomatic Complexity | 50 (configurable)   | Blue          |
| Churn Rate            | 3.0 (configurable)  | Amber         |
| Defect Density        | 0.15 (configurable) | Red           |
| Change Frequency      | max in repo          | Purple        |
| Sum of Coupling       | max in repo          | Teal          |

Each bar shows: label (left), value (right), filled bar (proportional to max). Uses the `MetricBar` component.

**Section C — Active Signals** (compact list):

Only signals involving this file. Same format as V3 cards but condensed to a single line each:

```
⚡ → token.py           0.92
👻 ↔ auth_routes.py      0.78
```

**Section D — Dependency Summary** (compact):

```
Structural: → 5 files  ← 3 files
Temporal:   ↔ 3 files (coupling > 0.3)
Defect:     → 1 file (fault propagation)
```

### V3: Signal Feed

Displayed below V8 in the right sidebar. Vertical scrollable list grouped by signal type.

**Group Order** (severity descending):

1. `ticking_bomb` — bomb icon
2. `fragile_boundary` — lightning icon
3. `ghost_coupling` — ghost icon
4. `over_engineering` — wrench icon
5. `stable_core` — shield icon

**Card Layout** (per signal):

```
┌─────────────────────────────────────────────┐
│ ⚡ auth/login.py → auth/token.py      0.92  │
│ Structural dep + co-change 0.82 + fault     │
│ propagation 0.18.                           │
└─────────────────────────────────────────────┘
```

| Element        | Encoding                                                       |
|----------------|----------------------------------------------------------------|
| Icon           | Signal type icon (left)                                        |
| Title          | `nodeA → nodeB` (or just `nodeA` for node-level signals)      |
| Severity badge | Numeric score, background color = signal type color with alpha |
| Detail text    | One-line explanation (from signal engine)                       |
| Card border    | Highlighted if `selectedNode` matches nodeA or nodeB           |

**Counts**: Show total count per group and global total in header.

### Interactions

| Action              | Result                                                          |
|---------------------|----------------------------------------------------------------|
| Click signal card   | Set `selectedNode = signal.nodeA`, switch to blast view, V2 re-centers |
| Filter by type      | Toggle signal types on/off                                     |
| Filter by severity  | Slider: min severity threshold (default: 0.0, show all)       |
| Search              | Text filter on file paths (shared with global search)          |

### Text Serialization (Agent Output)

```
ising signals --min-severity 0.5 --format table
```

Output:

```
TYPE               NODE_A              NODE_B              SEVERITY
─────────────────────────────────────────────────────────────────────
ticking_bomb       auth/login.py       -                   0.95
ticking_bomb       auth/token.py       -                   0.85
fragile_boundary   auth/login.py       auth/token.py       0.92
ghost_coupling     auth/login.py       events/notif...     0.78
```

### Performance

- Signal feed uses `react-window` for virtualized scrolling when signal count exceeds 100
- Signal index (node_id → signals map) is pre-computed at data load time for O(1) lookup in V8

### Component Structure

```
Sidebar.tsx
├── FileDetail.tsx (V8)
│   ├── IdentitySection
│   ├── MetricBars (uses MetricBar component)
│   ├── ActiveSignals (compact list)
│   └── DependencySummary
└── SignalFeed.tsx (V3)
    ├── SignalFilters (type toggles, severity slider)
    ├── SignalGroup (per signal type)
    │   ├── GroupHeader (icon, name, count)
    │   └── SignalCard (per signal)
    │       ├── Icon + Title
    │       ├── SeverityBadge
    │       └── Detail text
    └── EmptyState ("No signals match filters")
```

## Plan

- [ ] Implement `views/FileDetail.tsx` — file identity, metric bars, active signals, dependency summary
- [ ] Implement `views/SignalFeed.tsx` — grouped signal list with cards
- [ ] Implement `Sidebar.tsx` — layout container splitting V8 (top) and V3 (bottom, scroll)
- [ ] Implement signal type filter toggles (show/hide signal groups)
- [ ] Implement severity slider filter (min severity threshold)
- [ ] Implement signal card click → set `selectedNode`, switch to blast view
- [ ] Implement card border highlighting when `selectedNode` matches signal nodes
- [ ] Implement `MetricBar.tsx` — horizontal bar with label, value, filled proportion
- [ ] Implement `SeverityBadge.tsx` — colored chip with numeric score
- [ ] Add virtualized scrolling via `react-window` for > 100 signals
- [ ] Implement empty states for both V8 (no selection) and V3 (no matching signals)
- [ ] Wire global search to filter signal feed by file path

## Test

- [ ] V8 shows correct file identity (path, module, language, LOC, fan-in/out, bugs)
- [ ] V8 metric bars show correct values with bars proportional to max references
- [ ] V8 active signals list shows only signals involving the selected file
- [ ] V8 dependency summary counts match actual edge counts per layer
- [ ] V8 shows empty state when no node is selected
- [ ] V3 groups signals by type in correct severity order
- [ ] V3 shows correct total counts per group and globally
- [ ] V3 signal card click sets `selectedNode` and switches to blast view
- [ ] V3 signal cards highlight when `selectedNode` matches nodeA or nodeB
- [ ] V3 type filter hides/shows signal groups correctly
- [ ] V3 severity slider filters signals below threshold
- [ ] V3 search filter matches signals by file path
- [ ] V3 virtualizes when > 100 signals (no render lag)
- [ ] Signal feed filter completes within 50ms

## Notes

- V8 and V3 are in the same spec because they share the sidebar layout and have tight interaction coupling (V8 shows signals for the selected file; V3 clicking updates V8).
- The severity slider defaults to 0.0 (show all). Users can raise it to focus on critical signals — useful for large repos with many low-severity signals.
- Card border highlighting provides visual feedback connecting the sidebar to the main canvas — "this signal involves the file you're looking at."
- `react-window` is added as a dependency only for the signal feed. If the project prefers zero extra dependencies, a simpler approach is to cap the rendered list at 200 items with "show more" pagination.
