---
status: complete
created: 2026-03-24
priority: high
tags:
- signal-quality
- refactoring
- self-analysis
- metrics
depends_on:
- 021-self-analysis
- 024-go-language-support
created_at: 2026-03-24T08:26:36.655396556Z
updated_at: 2026-03-24T09:17:57.011546488Z
completed_at: 2026-03-24T09:17:57.011546488Z
transitions:
- status: in-progress
  at: 2026-03-24T09:04:42.739808109Z
- status: complete
  at: 2026-03-24T09:17:57.011546488Z
---

# Signal Engine Improvements: Self-Analysis Round 2 Findings

## Overview

A second self-analysis run (after adding Go support in spec 024) surfaced four actionable improvements to the signal engine: one metric bug causing a false positive, two code quality issues in the hottest files, and one feature gap that silences two signal types entirely.

## Requirements

### 1. Fix GodModule fan-out metric (false positive)
- [ ] Replace `fan_out >= thresholds.god_module_fan_out` with `metrics.cbo >= thresholds.god_module_fan_out` in `ising-analysis/src/signals.rs` (~line 468)
- [ ] Verify `main.rs` no longer fires as a god module (its cbo=1; all 22 fan-out edges are \`Contains\` edges to its own inner functions, not external dependencies)
- [ ] Add a unit test: a module with many inner functions but cbo=1 should not trigger GodModule
- [ ] Add a unit test: a module importing 15+ distinct external modules should trigger GodModule

### 2. Move export format renderers out of CLI
- [ ] Move \`generate_dot\` from \`ising-cli/src/main.rs\` into \`ising-db/src/export.rs\` as \`Database::get_dot_export(&self) -> Result<String, DbError>\`
- [ ] Move \`generate_mermaid\` similarly as \`Database::get_mermaid_export(&self) -> Result<String, DbError>\`
- [ ] Update \`cmd_export\` in \`main.rs\` to call the DB methods instead
- [ ] \`main.rs\` shrinks from ~595 to ~480 lines with no logic loss

### 3. Decompose \`detect_signals\` into per-detector functions
- [ ] Extract each signal type into its own private function: \`detect_ghost_coupling\`, \`detect_fragile_boundaries\`, \`detect_unnecessary_abstraction\`, \`detect_stable_cores\`, \`detect_ticking_bombs\`, \`detect_dependency_cycles\`, \`detect_god_modules\`, \`detect_shotgun_surgery\`, \`detect_unstable_dependencies\`
- [ ] \`detect_signals\` becomes a thin dispatcher calling each in sequence
- [ ] Add at least one isolated unit test per detector function
- [ ] \`signals.rs\` complexity drops from 128 to <20 per function (it is the #1 hotspot at score 0.83 with 10 changes in 26 commits)

### 4. Implement churn\_lines and churn\_rate
- [ ] In \`ising-builders/src/change.rs\`, compute per-file lines added + deleted while walking the commit graph (the diff is already being computed in \`get_changed_files\` — extend it to return line counts)
- [ ] Populate \`ChangeMetrics::churn_lines\` and \`ChangeMetrics::churn_rate\` (churn_lines / change_freq) instead of hardcoding 0
- [ ] Verify that TickingBomb and UnstableDependency signals can now fire (they require defect/churn data; without churn_rate they are permanently silent)
- [ ] Add a test asserting churn_lines > 0 for a commit that modifies file content

## Non-Goals

- Changing threshold values for existing signals
- Adding new signal types beyond the four listed
- Addressing the ghost coupling / unnecessary abstraction false positives from spec 023 (handled there)

## Technical Notes

- **Finding 1 (GodModule):** \`NodeMetrics.cbo\` (Coupling Between Objects) already exists in \`ising-core/src/metrics.rs\` and is computed correctly — distinct external \`file_path\` values of outgoing structural neighbors. No new metric needed, just swap the field used.
- **Finding 2 (export):** \`generate_dot\` and \`generate_mermaid\` both accept \`&Database\` and call \`db.get_signals()\` / \`db.get_hotspots()\`. They are format serializers, not CLI handlers. The DB layer already has \`export.rs\` with \`VizExport\` as precedent.
- **Finding 3 (decompose):** The current \`detect_signals\` shares precomputed state (importers map, fan_in_map, co_change_edges) across detector sections. Extracted functions should take these as parameters or recompute locally — local recomputation is fine given codebase sizes.
- **Finding 4 (churn):** \`get_changed_files\` in \`change.rs\` currently only records file paths from diffs. Extend to also sum \`lines_added\` and \`lines_deleted\` from \`gix\` diff stats. The \`gix\` diff API used already gives access to change counts per file.

## Acceptance Criteria

- [ ] Self-analysis on the ising repo produces 0 god_module signals for \`main.rs\`
- [ ] \`ising-cli/src/main.rs\` contains no export format logic
- [ ] \`detect_signals\` function body is <30 lines (dispatcher only)
- [ ] \`ising build\` on any repo with file edits populates non-zero churn_lines for changed files
- [ ] All existing tests pass