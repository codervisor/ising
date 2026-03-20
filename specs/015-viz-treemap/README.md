---
status: planned
created: 2026-03-20
priority: high
tags:
- visualization
- frontend
- treemap
depends_on:
- 014-viz-infrastructure
---

# V1: Global Treemap Heatmap

> **Status**: planned · **Priority**: high · **Created**: 2026-03-20

## Overview

The treemap is the default view — it answers "Where are the hotspots in my codebase?" in one glance. It renders every file as a rectangle sized by lines of code, colored by a switchable risk metric, grouped by module. This is the primary entry point for spatial navigation.

## Design

### Layout

Squarified treemap via `d3-treemap`. Two-level hierarchy: `module → file`. Each module gets a bordered group; files within it are sized proportional to LOC.

### Visual Encoding

| Channel        | Encodes                       | Range                             |
|----------------|-------------------------------|-----------------------------------|
| Rectangle area | LOC (lines of code)           | Proportional to file size         |
| Fill color     | Switchable metric (see below) | Sequential colormap               |
| Stroke         | Selected state                | White 2px stroke on selected file |
| Module border  | Module grouping               | Dark border + label at top-left   |

### Color Modes

User-switchable via toolbar:

| Mode                | Metric            | Colormap               | Semantics                  |
|---------------------|-------------------|------------------------|----------------------------|
| `hotspot` (default) | `hotspot_score`   | YlOrRd (yellow → red)  | Hotter = more risk         |
| `defect`            | `defect_density`  | YlOrRd                 | Redder = more bugs per LOC |
| `churn`             | `churn_rate`      | YlOrRd                 | Redder = more volatile     |
| `module`            | Module membership | Categorical            | Group identity             |
| `coupling`          | `sum_coupling`    | PuBuGn                 | Higher = more entangled    |
| `age`               | Days since change | RdYlGn reversed        | Older = greener (stable)   |

Color mode changes animate fill color with a 300ms ease transition (no layout recompute).

### Interactions

| Action                  | Result                                                              |
|-------------------------|---------------------------------------------------------------------|
| Hover file              | Tooltip: path, LOC, complexity, hotspot, churn, bugs, fan-in/out    |
| Click file              | Set `selectedNode`, update V2 + V8                                  |
| Click module border     | Zoom into module (treemap re-renders with only that module's files) |
| Double-click background | Zoom back to global view                                            |

### Signal Overlay

Toggle-able via toolbar. When enabled, files that are part of any active signal get a small icon badge in their top-right corner:

| Signal             | Badge                            |
|--------------------|----------------------------------|
| `ticking_bomb`     | bomb icon (red tint)             |
| `fragile_boundary` | lightning icon (red tint)        |
| `ghost_coupling`   | ghost icon (amber tint)          |
| `stable_core`      | shield icon (green tint)         |

Badge is rendered as a small SVG icon group positioned at the top-right of the rectangle. Only shown if the rectangle is large enough (area > threshold) to avoid visual clutter.

### Text Serialization (Agent Output)

```
ising hotspots --top 10 --format table
```

Output:

```
Rank  File                        Hotspot  Complexity  Churn  Bugs
1     src/auth/login.py           0.87     42          2.3    4
2     src/auth/token.py           0.72     28          1.8    5
...
```

### Performance

- D3 layout computed once per data load or zoom change. Stored in `useMemo`.
- Color mode change only updates fill attributes — no re-layout.
- Selection change only updates stroke — no re-layout, no re-color.
- For repos with 5,000+ files, small rectangles (area < 4px²) are batched into a single "other" rect per module.

### Component Structure

```
Treemap.tsx
├── TreemapModule (per module group)
│   ├── Module border + label
│   └── TreemapFile (per file rectangle)
│       ├── Rect (fill, stroke)
│       ├── Label (filename, truncated)
│       └── SignalBadge (conditional)
└── TreemapTooltip (on hover)
```

## Plan

- [ ] Implement `views/Treemap.tsx` — main treemap container with d3-treemap layout
- [ ] Implement squarified treemap layout computation in `useMemo` from derived hierarchy
- [ ] Render SVG rects per file with fill color from active color mode
- [ ] Render module group borders with labels
- [ ] Implement color mode switching with 300ms fill transition
- [ ] Implement hover tooltip showing file metrics
- [ ] Implement click-to-select dispatching `selectedNode` to global state
- [ ] Implement module zoom (click module border → re-render with filtered hierarchy)
- [ ] Implement signal overlay badges (toggle-able)
- [ ] Implement search dimming (non-matching files opacity 0.3)
- [ ] Add `ColorModeSelector.tsx` component for toolbar integration
- [ ] Performance: batch small rectangles for large repos

## Test

- [ ] Treemap renders all files from mock data, rectangle areas proportional to LOC
- [ ] Each color mode maps the correct metric to the YlOrRd / PuBuGn / categorical scale
- [ ] Color mode switch transitions fill without re-computing layout
- [ ] Clicking a file sets `selectedNode` and applies white stroke
- [ ] Clicking module border zooms into that module; double-click background zooms out
- [ ] Hover tooltip shows correct path, LOC, complexity, hotspot, churn, bugs, fan-in/out
- [ ] Signal overlay badges appear only on files involved in signals
- [ ] Signal badges hidden on rectangles below minimum area threshold
- [ ] Search query dims non-matching files to 0.3 opacity
- [ ] Renders 1,000-file dataset within 500ms (initial paint)
- [ ] Color mode change completes within 100ms

## Notes

- The treemap is intentionally file-level only for MVP. Function-level drill-down (3-level hierarchy: module → file → function) is a Phase 2 feature gated on spec 010.
- Rectangle labels use monospace font and are truncated with ellipsis when they exceed the rectangle width. Labels are hidden entirely on very small rectangles.
- The `module` color mode uses the categorical palette from the design system. All other modes use sequential colormaps where "more" = "more dangerous."
- `d3-treemap` with `treemapSquarify` tiling produces the best aspect ratios for readability.
