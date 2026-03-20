---
status: planned
created: 2026-03-20
priority: high
tags:
- visualization
- frontend
- infrastructure
depends_on:
- 013-viz-data-export
---

# Visualization SPA Infrastructure

> **Status**: planned · **Priority**: high · **Created**: 2026-03-20

## Overview

This spec defines the foundation for the Ising visualization: a static SPA that loads graph data from a JSON export and renders an opinionated risk dashboard. Every pixel answers: *"Where is danger, and how confident are we?"*

The SPA is offline-first (no backend server required), dark-themed to match IDE environments, and produces agent-compatible output (Mermaid/JSON) alongside interactive views.

This spec covers the scaffolding, data loading, global state, layout shell, visual design system, and responsive behavior. Individual views (Treemap, Blast Radius, Signal Feed, File Detail) are specified in separate specs.

## Design

### Tech Stack

| Component    | Technology                         | Rationale                                              |
|--------------|------------------------------------|---------------------------------------------------------|
| Framework    | React 18+                         | Component model, hooks, ecosystem                       |
| Build        | Vite                               | Fast dev server, optimized production build             |
| Graph layout | D3.js (d3-force, d3-treemap)      | Industry standard, fine-grained control                 |
| Rendering    | SVG (via React + D3 math)         | Declarative, inspectable, animatable                    |
| Styling      | Tailwind CSS                       | Rapid iteration, consistent spacing/color               |
| State        | React useState + useContext        | Simple enough for MVP, no Redux needed                  |
| Data loading | fetch + JSON parse                 | Static file or MCP SSE endpoint                         |
| Animation    | CSS transitions + d3-transition   | Smooth state changes                                    |

### File Structure

```
ising-viz/
├── index.html
├── package.json
├── vite.config.ts
├── tailwind.config.ts
├── src/
│   ├── main.tsx
│   ├── App.tsx                     # Layout shell, global state, routing
│   ├── types.ts                    # TypeScript interfaces for data schema
│   ├── data/
│   │   ├── loader.ts               # Load from JSON file or MCP endpoint
│   │   ├── derived.ts              # Compute treemap hierarchy, percentiles, indexes
│   │   └── mock.ts                 # Mock data for development
│   ├── state/
│   │   └── context.tsx             # AppState context + reducer
│   ├── views/                      # (created by view-specific specs)
│   ├── components/
│   │   ├── Header.tsx              # Top toolbar
│   │   ├── Tooltip.tsx             # Shared tooltip component
│   │   ├── LayerToggle.tsx         # Layer on/off buttons
│   │   ├── ColorModeSelector.tsx   # Treemap color mode buttons
│   │   ├── SearchBar.tsx           # File path search
│   │   ├── SeverityBadge.tsx       # Signal severity chip
│   │   └── MetricBar.tsx           # Horizontal bar chart row
│   ├── utils/
│   │   ├── colors.ts               # Color scales, module palette assignment
│   │   ├── graph.ts                # BFS neighborhood extraction, edge filtering
│   │   └── format.ts               # Number formatting, path shortening
│   └── styles/
│       └── globals.css             # Font imports, base styles
└── public/
    └── sample-data.json            # Example ising export for demo
```

### Global State

```typescript
interface AppState {
  // Selection
  selectedNode: string | null;
  hoveredNode: string | null;

  // View
  activeView: "treemap" | "blast";
  treemapColorMode: "hotspot" | "defect" | "churn" | "module" | "coupling" | "age";
  signalOverlay: boolean;

  // Blast Radius config
  blastDepth: 1 | 2 | 3;
  activeLayers: {
    structural: boolean;
    change: boolean;
    defect: boolean;
  };

  // Filters
  signalTypeFilter: Set<SignalType>;
  signalMinSeverity: number;         // 0.0 - 1.0
  searchQuery: string;

  // Zoom
  treemapZoomModule: string | null;
}
```

All views share a single `selectedNode` state. Clicking anywhere propagates selection globally.

### Data Loading

Two data source options:

1. **Static JSON** (offline) — `fetch("./sample-data.json")`, no server needed. Suitable for CI/CD artifact embedding.
2. **MCP server** (live) — Connect to Ising MCP server via SSE. Calls `ising_impact`, `ising_signals`, `ising_hotspots` tools.

The `loader.ts` module detects the data source from URL parameters (`?data=path/to/export.json` or `?mcp=http://localhost:3000`).

### Derived Data (Computed at Load Time)

| Derived               | Computation                                    | Used in |
|-----------------------|------------------------------------------------|---------|
| Treemap hierarchy     | Group nodes by `module`, value = `loc`         | V1      |
| Percentile ranks      | For each metric, compute rank across all nodes | V8      |
| Neighborhood subgraph | BFS from selected node on edge list            | V2      |
| Signal index          | Map `node_id → [signals]` for fast lookup      | V3, V8  |
| Module color map      | Assign colors to unique module names           | All     |

### Layout Shell

```
┌──────────────────────────────────────────────────────────┐
│ HEADER (48px fixed)                                      │
│ [Logo] [View Tabs] [Color Mode / Layer Toggles] [Search] │
├──────────────────────────────────────────┬───────────────┤
│                                          │               │
│  MAIN CANVAS                             │  RIGHT SIDEBAR│
│  (flex: 1)                               │  (340px fixed)│
│                                          │               │
│  V1 Treemap  or  V2 Blast Radius         │  V8 Detail    │
│  (switched by tab)                       │  (top, auto-h)│
│                                          │               │
│                                          ├───────────────┤
│                                          │               │
│                                          │  V3 Signals   │
│                                          │  (bottom,     │
│                                          │   scroll)     │
│                                          │               │
└──────────────────────────────────────────┴───────────────┘
```

Header toolbar adapts to active view:
- Treemap active → color mode selector
- Blast Radius active → layer toggles + depth slider
- Always → signal overlay toggle, search input

### Responsive Behavior

| Breakpoint | Layout change                                                   |
|------------|------------------------------------------------------------------|
| >= 1200px  | Full layout as above                                             |
| 900-1199px | Sidebar collapses to 280px, labels truncate                     |
| < 900px    | Sidebar moves to bottom sheet (overlay), main canvas full width |

### Visual Design System

**Theme** — Dark only for MVP (code tools are used alongside dark-themed IDEs):

```
Background:        #080c14  (near-black blue)
Surface:           #0f172a  (slate-900)
Surface elevated:  #1e293b  (slate-800)
Border:            #334155  (slate-700)
Text primary:      #f1f5f9  (slate-100)
Text secondary:    #94a3b8  (slate-400)
Text muted:        #64748b  (slate-500)
```

**Module color palette** (categorical, distinguishable on dark background):

```
auth:        #ef4444  (red-500)      api:         #3b82f6  (blue-500)
db:          #10b981  (emerald-500)  events:      #f59e0b  (amber-500)
middleware:  #8b5cf6  (violet-500)   utils:       #6b7280  (gray-500)
tests:       #06b6d4  (cyan-500)     config:      #d946ef  (fuchsia-500)
```

For repos with 8+ modules, fall back to `d3.schemeTableau10`.

**Signal colors**:

| Signal             | Primary   | Background (20% alpha) |
|--------------------|-----------|------------------------|
| `ticking_bomb`     | `#dc2626` | `#dc262633`            |
| `fragile_boundary` | `#ef4444` | `#ef444433`            |
| `ghost_coupling`   | `#f59e0b` | `#f59e0b33`            |
| `over_engineering`  | `#6b7280` | `#6b728033`            |
| `stable_core`      | `#10b981` | `#10b98133`            |

**Layer edge styles**:

| Layer      | Color     | Dash             | Semantics              |
|------------|-----------|------------------|------------------------|
| Structural | `#94a3b8` | Solid            | Static code dependency |
| Change     | `#f59e0b` | `6,3` long dash  | Temporal coupling      |
| Defect     | `#ef4444` | `3,3` short dash | Fault propagation      |

**Typography**:

```
Monospace (primary): "JetBrains Mono", "Fira Code", "SF Mono", monospace
Sans-serif (headings): "Inter", system-ui, sans-serif
```

Monospace for all file paths, metrics, and code-related text. Sans-serif only for high-level labels.

**Sequential colormaps**:

| Metric type        | Colormap                                 |
|--------------------|------------------------------------------|
| Risk / heat        | `d3.interpolateYlOrRd` (Yellow → Red)   |
| Health / stability | `d3.interpolateRdYlGn` (reversed)       |
| Coupling intensity | `d3.interpolatePuBuGn`                   |

### Interaction Flow

```
User clicks file in V1 (Treemap)
  → selectedNode = file.id
  → V2 (if visible) re-computes neighborhood, animates to new center
  → V8 updates metrics panel
  → V3 highlights relevant signals
  → V1 highlights selected rectangle

User clicks signal in V3 (Signal Feed)
  → selectedNode = signal.node_a
  → activeView switches to "blast" automatically
  → V2 centers on node_a, highlights the signal edge
  → V8 updates

User toggles layer in V2 toolbar
  → edges of that layer animate in/out (opacity transition)
  → force simulation re-runs

User changes color mode in V1 toolbar
  → all treemap rectangles transition fill color (300ms ease)

User types in search box
  → V1: matching files full opacity, others dim to 0.3
  → V3: signals filtered to those involving matching files
```

### Performance Targets

| Metric                                | Target          |
|---------------------------------------|-----------------|
| Initial load (with data)              | < 500ms         |
| Treemap re-render (color mode change) | < 100ms         |
| Blast radius center change            | < 300ms         |
| Signal feed filter                    | < 50ms          |
| Max supported nodes                   | 5,000 files     |
| Max supported nodes (blast radius)    | 50 visible      |
| Bundle size                           | < 300KB gzipped |

## Plan

- [ ] Scaffold `ising-viz/` with Vite + React + TypeScript
- [ ] Configure Tailwind CSS with dark theme tokens matching the design system
- [ ] Define TypeScript interfaces in `types.ts` matching the viz-json schema
- [ ] Implement `data/loader.ts` — fetch static JSON, parse, validate
- [ ] Implement `data/derived.ts` — treemap hierarchy, percentile ranks, signal index, module color map
- [ ] Implement `data/mock.ts` — generate realistic mock data for development (100 files, 3 layers)
- [ ] Implement `state/context.tsx` — AppState context provider + dispatch actions
- [ ] Build `App.tsx` layout shell — header, main canvas, right sidebar
- [ ] Build `components/Header.tsx` — logo, view tabs, context-dependent toolbar
- [ ] Build `components/Tooltip.tsx` — shared tooltip positioned near cursor
- [ ] Build `components/SearchBar.tsx` — text filter with debounced dispatch
- [ ] Build `components/MetricBar.tsx` — horizontal bar chart row component
- [ ] Build `components/SeverityBadge.tsx` — colored severity chip
- [ ] Implement `utils/colors.ts` — module palette assignment, sequential colormap wrappers
- [ ] Implement `utils/format.ts` — number formatting, path shortening
- [ ] Implement `utils/graph.ts` — BFS neighborhood extraction, edge filtering by layer
- [ ] Add responsive breakpoint handling
- [ ] Verify bundle size < 300KB gzipped

## Test

- [ ] `data/loader.ts` correctly parses the viz-json schema and rejects malformed input
- [ ] `data/derived.ts` produces correct treemap hierarchy grouping nodes by module
- [ ] `data/derived.ts` percentile rank assigns 1.0 to the highest value and 0.0 to the lowest
- [ ] `state/context.tsx` selection change propagates to all consuming components
- [ ] Module color assignment is deterministic (same modules always get same colors)
- [ ] `utils/graph.ts` BFS with depth=2 returns correct neighborhood for a known graph
- [ ] Layout renders correctly at 1200px, 1000px, and 800px breakpoints
- [ ] Header toolbar switches between treemap controls and blast radius controls
- [ ] Search filter dims non-matching nodes and filters signal list
- [ ] Bundle size stays under 300KB gzipped

## Notes

- Dark theme only for MVP. Light theme is a Phase 2 feature.
- No backend server required — the SPA reads a static JSON file. This makes it embeddable in CI/CD artifacts, GitHub Pages, or any static host.
- State management uses plain React context, not Redux. If state complexity grows beyond MVP, migrate to Zustand.
- D3 is used for math (layout algorithms, color scales) only — React handles DOM rendering via SVG elements. This avoids the D3-React impedance mismatch.
- `sample-data.json` in `public/` enables `npm run dev` to work immediately without running `ising build` first.
