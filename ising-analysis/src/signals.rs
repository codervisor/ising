//! Cross-layer signal detection.
//!
//! Each signal is a comparison between layers that reveals patterns invisible
//! from any single layer alone.

use ising_core::boundary::{BoundaryStructure, CrossingType, severity_multiplier};
use ising_core::config::{Config, ThresholdConfig};
use ising_core::graph::{EdgeLayer, EdgeType, UnifiedGraph};
use ising_core::metrics::{compute_node_metrics, percentile};
use petgraph::visit::EdgeRef;
use serde::Serialize;

/// Types of cross-layer signals.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalType {
    /// No structural dep but high temporal coupling — hidden dependency.
    GhostCoupling,
    /// Structural dep + high co-change + fault propagation — broken interface.
    FragileBoundary,
    /// Structural dep but never co-change — possibly unnecessary abstraction.
    UnnecessaryAbstraction,
    /// High fan-in, low change freq, low defects — stable foundation.
    StableCore,
    /// High hotspot + high defects + high coupling — most dangerous code.
    TickingBomb,
    /// Circular dependency between modules — architectural entanglement.
    DependencyCycle,
    /// Extreme complexity + LOC + fan-out — does too much, hard to maintain.
    GodModule,
    /// One file's changes scatter across many other files — scattered responsibility.
    ShotgunSurgery,
    /// Stable module depends on unstable module — Stable Dependencies Principle violation.
    UnstableDependency,
    /// Median/P75 complexity is high across the codebase — distributed complexity risk.
    SystemicComplexity,
    /// Function with zero callers, not an entry point — potentially dead code.
    OrphanFunction,
    /// Module with zero importers, not an entry point — potentially dead code.
    OrphanModule,
    /// Deprecated symbol still being called or imported — migration risk.
    DeprecatedUsage,
    /// Code unchanged for extended period with low connectivity — possibly obsolete.
    StaleCode,
    /// Function churns far more than siblings in the same file — intra-file hotspot.
    IntraFileHotspot,
    /// Module has high cross-boundary temporal coupling — boundary leakage.
    BoundaryLeakage,
}

impl SignalType {
    pub fn priority(&self) -> &'static str {
        match self {
            SignalType::FragileBoundary | SignalType::TickingBomb | SignalType::DependencyCycle => {
                "critical"
            }
            SignalType::GhostCoupling
            | SignalType::GodModule
            | SignalType::ShotgunSurgery
            | SignalType::UnstableDependency
            | SignalType::SystemicComplexity => "high",
            SignalType::StableCore => "guard",
            SignalType::UnnecessaryAbstraction => "info",
            SignalType::OrphanFunction | SignalType::OrphanModule => "info",
            SignalType::DeprecatedUsage => "high",
            SignalType::StaleCode => "info",
            SignalType::IntraFileHotspot => "high",
            SignalType::BoundaryLeakage => "high",
        }
    }
}

/// A detected cross-layer signal.
#[derive(Debug, Clone, Serialize)]
pub struct Signal {
    pub signal_type: SignalType,
    pub node_a: String,
    pub node_b: Option<String>,
    pub severity: f64,
    pub description: String,
}

impl Signal {
    fn new(
        signal_type: SignalType,
        node_a: &str,
        node_b: Option<&str>,
        severity: f64,
        description: String,
    ) -> Self {
        Self {
            signal_type,
            node_a: node_a.to_string(),
            node_b: node_b.map(|s| s.to_string()),
            severity,
            description,
        }
    }
}

/// Detect all cross-layer signals in the unified graph.
///
/// If `boundaries` is provided, signals are boundary-aware:
/// - Ghost coupling only fires on cross-boundary pairs
/// - Unnecessary abstraction skips cross-boundary wrappers
/// - Fragile boundary severity scaled by crossing type
/// - Boundary leakage detection enabled
pub fn detect_signals(
    graph: &UnifiedGraph,
    config: &Config,
    boundaries: Option<&BoundaryStructure>,
) -> Vec<Signal> {
    let co_change_edges = graph.edges_of_type(&EdgeType::CoChanges);
    let import_edges = graph.edges_of_type(&EdgeType::Imports);
    let node_ids: Vec<String> = graph.node_ids().map(|s| s.to_string()).collect();

    let mut signals = Vec::new();
    signals.extend(detect_ghost_coupling(
        &co_change_edges,
        &import_edges,
        graph,
        &config.thresholds,
        boundaries,
    ));
    signals.extend(detect_fragile_boundaries(
        &co_change_edges,
        graph,
        &config.thresholds,
        boundaries,
    ));
    signals.extend(detect_unnecessary_abstraction(
        &import_edges,
        graph,
        &config.thresholds,
        boundaries,
    ));
    signals.extend(detect_stable_cores(&node_ids, graph, config));
    signals.extend(detect_ticking_bombs(&node_ids, graph, config));
    signals.extend(detect_dependency_cycles(graph));
    signals.extend(detect_god_modules(&node_ids, graph, &config.thresholds));
    signals.extend(detect_shotgun_surgery(&co_change_edges, &config.thresholds));
    signals.extend(detect_unstable_dependencies(
        &import_edges,
        graph,
        &config.thresholds,
    ));
    signals.extend(detect_systemic_complexity(&node_ids, graph));
    signals.extend(detect_orphan_functions(graph));
    signals.extend(detect_orphan_modules(graph, &import_edges));
    signals.extend(detect_deprecated_usage(graph));
    signals.extend(detect_stale_code(graph));
    signals.extend(detect_intra_file_hotspots(graph));

    // Boundary-aware signals
    if let Some(bs) = boundaries {
        signals.extend(detect_boundary_leakage(&co_change_edges, graph, bs));
    }

    signals.sort_by(|a, b| {
        b.severity
            .partial_cmp(&a.severity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    signals
}

fn detect_ghost_coupling(
    co_change_edges: &[(&str, &str, f64)],
    import_edges: &[(&str, &str, f64)],
    graph: &UnifiedGraph,
    thresholds: &ThresholdConfig,
    boundaries: Option<&BoundaryStructure>,
) -> Vec<Signal> {
    // Build importer index for common-parent suppression.
    let mut importers: std::collections::HashMap<&str, std::collections::HashSet<&str>> =
        std::collections::HashMap::new();
    for (src, tgt, _) in import_edges {
        importers.entry(tgt).or_default().insert(src);
    }

    let mut signals = Vec::new();
    for (a, b, coupling) in co_change_edges {
        if graph.has_structural_edge(a, b)
            || *coupling <= thresholds.ghost_coupling_threshold
            || is_test_source_pair(a, b)
            || !is_source_file(a)
            || !is_source_file(b)
            // GAP-13: Go package-level imports resolve to all files in a directory,
            // so two .go files in the same package co-change without structural edges.
            // This is normal Go packaging, not a hidden dependency.
            || is_go_intra_package_pair(a, b)
        {
            continue;
        }

        // Boundary-aware filtering: suppress same-module ghost coupling.
        // Intra-module co-change without structural edge is almost always
        // explained by shared parent or sibling relationship.
        if let Some(bs) = boundaries {
            let crossing = bs.crossing_type(a, b);
            let mult = severity_multiplier(&crossing);
            if mult == 0.0 {
                continue; // SameModule → suppress
            }

            // Even for cross-boundary pairs, apply common-parent suppression.
            // Files orchestrated by a shared parent co-change for legitimate
            // reasons even across module boundaries.
            let empty = std::collections::HashSet::new();
            let importers_a = importers.get(a).unwrap_or(&empty);
            let importers_b = importers.get(b).unwrap_or(&empty);
            let shared_parents: Vec<&&str> = importers_a.intersection(importers_b).collect();
            let has_shared_parent = !shared_parents.is_empty() || is_cross_crate_pair(a, b);

            if has_shared_parent {
                // Suppress unless coupling is very high (≥0.9)
                if *coupling >= 0.9 {
                    let parent_names: Vec<&str> = shared_parents.iter().map(|s| **s).collect();
                    let parent_desc = if parent_names.is_empty() {
                        "workspace orchestration".to_string()
                    } else {
                        parent_names.join(", ")
                    };
                    signals.push(Signal::new(
                        SignalType::GhostCoupling,
                        a,
                        Some(b),
                        *coupling * 0.3 * mult,
                        format!(
                            "No structural dependency, but {:.0}% co-change rate. Co-change likely explained by shared parent {}, but coupling is very high.",
                            coupling * 100.0,
                            parent_desc
                        ),
                    ));
                }
                continue;
            }

            let (pkg_a, mod_a) = bs.module_of(a);
            let (pkg_b, mod_b) = bs.module_of(b);

            let scope_desc = match crossing {
                CrossingType::CrossPackage => {
                    format!("Cross-package ({} ↔ {})", pkg_a, pkg_b)
                }
                CrossingType::CrossModule => {
                    format!("Cross-module ({} ↔ {}) in {}", mod_a, mod_b, pkg_a)
                }
                CrossingType::SameModule => unreachable!(),
            };

            signals.push(Signal::new(
                SignalType::GhostCoupling,
                a,
                Some(b),
                *coupling * mult,
                format!(
                    "{}. {:.0}% co-change with no structural dependency.",
                    scope_desc,
                    coupling * 100.0
                ),
            ));
            continue;
        }

        // Legacy path (no boundaries): use common-parent suppression
        let empty = std::collections::HashSet::new();
        let importers_a = importers.get(a).unwrap_or(&empty);
        let importers_b = importers.get(b).unwrap_or(&empty);
        let shared_parents: Vec<&&str> = importers_a.intersection(importers_b).collect();
        let has_shared_parent = !shared_parents.is_empty() || is_cross_crate_pair(a, b);

        if has_shared_parent {
            // Suppress unless coupling is very high (≥0.9), in which case emit at reduced severity.
            if *coupling >= 0.9 {
                let parent_names: Vec<&str> = shared_parents.iter().map(|s| **s).collect();
                let parent_desc = if parent_names.is_empty() {
                    "workspace orchestration".to_string()
                } else {
                    parent_names.join(", ")
                };
                signals.push(Signal::new(
                    SignalType::GhostCoupling,
                    a,
                    Some(b),
                    *coupling * 0.3,
                    format!(
                        "No structural dependency, but {:.0}% co-change rate. Co-change likely explained by shared parent {}, but coupling is very high — verify no hidden dependency.",
                        coupling * 100.0,
                        parent_desc
                    ),
                ));
            }
        } else {
            signals.push(Signal::new(
                SignalType::GhostCoupling,
                a,
                Some(b),
                *coupling,
                format!(
                    "No structural dependency, but {:.0}% co-change rate. Likely missing an abstraction layer.",
                    coupling * 100.0
                ),
            ));
        }
    }
    signals
}

fn detect_fragile_boundaries(
    co_change_edges: &[(&str, &str, f64)],
    graph: &UnifiedGraph,
    thresholds: &ThresholdConfig,
    boundaries: Option<&BoundaryStructure>,
) -> Vec<Signal> {
    let mut signals = Vec::new();
    for (a, b, coupling) in co_change_edges {
        let fault_prop = graph
            .edge_weight(a, b, &EdgeType::FaultPropagates)
            .unwrap_or(0.0);
        if graph.has_structural_edge(a, b)
            && *coupling > thresholds.fragile_boundary_coupling
            && fault_prop > thresholds.fragile_boundary_fault_prop
        {
            // Boundary-aware severity scaling:
            // Cross-package fragility = 3x, cross-module = 1.5x, same-module = 0.5x
            let boundary_mult = if let Some(bs) = boundaries {
                match bs.crossing_type(a, b) {
                    CrossingType::CrossPackage => 3.0,
                    CrossingType::CrossModule => 1.5,
                    CrossingType::SameModule => 0.5,
                }
            } else {
                1.0
            };

            let base_severity = coupling * fault_prop * 10.0;
            signals.push(Signal::new(
                SignalType::FragileBoundary,
                a,
                Some(b),
                base_severity * boundary_mult,
                format!(
                    "Structural dep + {:.0}% co-change + {:.0}% fault propagation. Interface is fragile.",
                    coupling * 100.0,
                    fault_prop * 100.0
                ),
            ));
        }
    }
    signals
}

fn detect_unnecessary_abstraction(
    import_edges: &[(&str, &str, f64)],
    graph: &UnifiedGraph,
    thresholds: &ThresholdConfig,
    boundaries: Option<&BoundaryStructure>,
) -> Vec<Signal> {
    // Unnecessary Abstraction: detect likely unnecessary abstractions
    //
    // A low co-change rate between A→B is NOT a signal by itself — most stable,
    // well-designed dependencies have exactly this profile. Instead we look for:
    //
    // 1. Single-consumer wrapper: B has fan-in=1 (only A uses it), B itself
    //    has low complexity and rarely changes. The abstraction serves one
    //    consumer and never needed updating — likely unnecessary indirection.
    //
    // 2. Pass-through module: A→B→C where A and C co-change but B never does.
    //    B is an indirection layer adding no value.

    let mut fan_in_map: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (_, target, _) in import_edges {
        *fan_in_map.entry(target).or_default() += 1;
    }

    let mut import_targets: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for (src, tgt, _) in import_edges {
        import_targets.entry(src).or_default().push(tgt);
    }

    let mut signals = Vec::new();
    let mut seen_import_pairs = std::collections::HashSet::new();
    for (a, b, _) in import_edges {
        if is_reexport_module(a) || is_reexport_module(b) {
            continue;
        }
        if is_docs_example(a) || is_docs_example(b) {
            continue;
        }
        // GAP-5: Rust lib.rs/main.rs use `mod` declarations idiomatically
        if is_rust_entry_point(a) {
            continue;
        }
        // GAP-13: Go package-level imports create edges to every file in the
        // target package. Two .go files in the same package are siblings, not
        // abstraction layers — skip intra-package pairs.
        if is_go_intra_package_pair(a, b) {
            continue;
        }
        // Boundary-aware: cross-boundary thin wrappers are intentional adapters
        if let Some(bs) = boundaries {
            let crossing = bs.crossing_type(a, b);
            if matches!(
                crossing,
                CrossingType::CrossPackage | CrossingType::CrossModule
            ) {
                continue;
            }
        }
        let pair: (String, String) = if a < b {
            (a.to_string(), b.to_string())
        } else {
            (b.to_string(), a.to_string())
        };
        if !seen_import_pairs.insert(pair) {
            continue;
        }

        let coupling_ab = graph.edge_weight(a, b, &EdgeType::CoChanges).unwrap_or(0.0);
        if coupling_ab >= thresholds.unnecessary_abstraction_coupling {
            continue;
        }

        let b_fan_in = fan_in_map.get(b).copied().unwrap_or(0);
        let b_change_freq = graph
            .change_metrics
            .get(*b)
            .map(|m| m.change_freq)
            .unwrap_or(0);
        let b_complexity = graph.get_node(b).and_then(|n| n.complexity).unwrap_or(0);

        // Signal 1: Single-consumer wrapper
        if b_fan_in == 1 && b_complexity <= 5 && b_change_freq <= 1 {
            signals.push(Signal::new(
                SignalType::UnnecessaryAbstraction,
                a,
                Some(b),
                0.4,
                format!(
                    "Single-consumer wrapper: only {} imports {}, which has complexity {} and {} changes. Consider inlining.",
                    a, b, b_complexity, b_change_freq
                ),
            ));
            continue;
        }

        // Signal 2: Pass-through module (A→B→C where A↔C co-change but B is dormant)
        if let Some(b_targets) = import_targets.get(b) {
            for c in b_targets {
                let coupling_ac = graph.edge_weight(a, c, &EdgeType::CoChanges).unwrap_or(0.0);
                let coupling_bc = graph.edge_weight(b, c, &EdgeType::CoChanges).unwrap_or(0.0);
                if coupling_ac > thresholds.ghost_coupling_threshold
                    && coupling_bc < thresholds.unnecessary_abstraction_coupling
                {
                    signals.push(Signal::new(
                        SignalType::UnnecessaryAbstraction,
                        a,
                        Some(b),
                        coupling_ac * 0.5,
                        format!(
                            "Pass-through: {} imports {} imports {}, but {} and {} co-change at {:.0}% while {} is dormant. Consider removing the indirection.",
                            a, b, c, a, c, coupling_ac * 100.0, b
                        ),
                    ));
                    break;
                }
            }
        }
    }
    signals
}

fn detect_stable_cores(node_ids: &[String], graph: &UnifiedGraph, config: &Config) -> Vec<Signal> {
    let mut change_freqs: Vec<f64> = node_ids
        .iter()
        .filter_map(|id| {
            graph
                .change_metrics
                .get(id.as_str())
                .map(|m| m.change_freq as f64)
        })
        .collect();
    let mut fan_ins: Vec<f64> = node_ids
        .iter()
        .map(|id| compute_node_metrics(graph, id).fan_in as f64)
        .collect();

    let freq_p_low = percentile(&mut change_freqs, config.percentiles.stable_core_freq);
    let fan_in_p_high = percentile(&mut fan_ins, config.percentiles.stable_core_fan_in);

    let mut signals = Vec::new();
    for node_id in node_ids {
        let change = graph.change_metrics.get(node_id.as_str());
        let freq = change.map(|m| m.change_freq as f64).unwrap_or(0.0);
        let fan_in = compute_node_metrics(graph, node_id).fan_in as f64;

        // GAP-6: Require minimum absolute fan-in of 5 to avoid noise in large codebases
        // where the 80th percentile can be as low as 1.
        let min_fan_in = fan_in_p_high.max(5.0);
        if freq > 0.0
            && freq <= freq_p_low
            && fan_in >= min_fan_in
            && !is_test_file(node_id)
            && !is_docs_example(node_id)
        {
            signals.push(Signal::new(
                SignalType::StableCore,
                node_id,
                None,
                0.1,
                format!(
                    "Stable foundation: high fan-in ({:.0}), low change frequency ({:.0}). Protect from unnecessary changes.",
                    fan_in, freq
                ),
            ));
        }
    }
    signals
}

fn detect_ticking_bombs(node_ids: &[String], graph: &UnifiedGraph, config: &Config) -> Vec<Signal> {
    let mut hotspots: Vec<f64> = node_ids
        .iter()
        .filter_map(|id| {
            graph
                .change_metrics
                .get(id.as_str())
                .map(|m| m.hotspot_score)
        })
        .collect();
    let mut defect_densities: Vec<f64> = node_ids
        .iter()
        .filter_map(|id| {
            graph
                .defect_metrics
                .get(id.as_str())
                .map(|m| m.defect_density)
        })
        .collect();
    let mut sum_couplings: Vec<f64> = node_ids
        .iter()
        .filter_map(|id| {
            graph
                .change_metrics
                .get(id.as_str())
                .map(|m| m.sum_coupling)
        })
        .collect();

    let hotspot_p_high = percentile(&mut hotspots, config.percentiles.ticking_bomb_hotspot);
    let defect_p_high = percentile(
        &mut defect_densities,
        config.percentiles.ticking_bomb_defect,
    );
    let coupling_p_high = percentile(&mut sum_couplings, config.percentiles.ticking_bomb_coupling);

    let mut signals = Vec::new();
    for node_id in node_ids {
        let change = graph.change_metrics.get(node_id.as_str());
        let defect = graph.defect_metrics.get(node_id.as_str());
        let hotspot = change.map(|m| m.hotspot_score).unwrap_or(0.0);
        let defect_d = defect.map(|m| m.defect_density).unwrap_or(0.0);
        let sum_coupling = change.map(|m| m.sum_coupling).unwrap_or(0.0);

        if hotspot > hotspot_p_high
            && hotspot_p_high > 0.0
            && defect_d > defect_p_high
            && defect_p_high > 0.0
            && sum_coupling > coupling_p_high
            && coupling_p_high > 0.0
        {
            signals.push(Signal::new(
                SignalType::TickingBomb,
                node_id,
                None,
                hotspot * defect_d * 10.0,
                format!(
                    "Complex ({:.2} hotspot), buggy ({:.2} defect density), highly coupled ({:.2}). Refactor before making changes.",
                    hotspot, defect_d, sum_coupling
                ),
            ));
        }
    }
    signals
}

fn detect_dependency_cycles(graph: &UnifiedGraph) -> Vec<Signal> {
    let sccs = petgraph::algo::tarjan_scc(&graph.graph);
    let mut signals = Vec::new();
    for scc in &sccs {
        if scc.len() < 2 {
            continue;
        }
        let cycle_ids: Vec<&str> = scc
            .iter()
            .map(|&idx| graph.graph[idx].id.as_str())
            .collect();

        if !cycle_ids
            .iter()
            .all(|id| is_source_file(id) && !is_generated_code(id))
        {
            continue;
        }

        let has_structural = scc.iter().any(|&idx| {
            graph.graph.edges(idx).any(|e| {
                e.weight().edge_type.layer() == EdgeLayer::Structural && scc.contains(&e.target())
            })
        });
        if !has_structural {
            continue;
        }

        let severity = cycle_ids.len() as f64 * 0.5;
        let cycle_desc = if cycle_ids.len() <= 5 {
            cycle_ids.join(" → ")
        } else {
            format!(
                "{} → ... → {} ({} modules)",
                cycle_ids[0],
                cycle_ids[cycle_ids.len() - 1],
                cycle_ids.len()
            )
        };
        signals.push(Signal::new(
            SignalType::DependencyCycle,
            cycle_ids[0],
            cycle_ids.get(1).copied(),
            severity,
            format!(
                "Circular dependency: {}. Break the cycle to improve modularity.",
                cycle_desc
            ),
        ));
    }
    signals
}

fn detect_god_modules(
    node_ids: &[String],
    graph: &UnifiedGraph,
    thresholds: &ThresholdConfig,
) -> Vec<Signal> {
    let mut signals = Vec::new();
    for node_id in node_ids {
        let node = match graph.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        let complexity = node.complexity.unwrap_or(0);
        let loc = node.loc.unwrap_or(0);
        let metrics = compute_node_metrics(graph, node_id);
        // Use CBO (Coupling Between Objects) — distinct external modules depended on.
        // fan_out counts all outgoing structural edges including Contains edges to own
        // inner functions, which inflates the score and causes false positives.
        let cbo = metrics.cbo;

        let is_test = is_test_file(node_id);
        let is_generated = is_generated_code(node_id);

        if complexity >= thresholds.god_module_complexity
            && loc >= thresholds.god_module_loc
            && cbo >= thresholds.god_module_fan_out
            && !is_test
            && !is_generated
        {
            let severity = (complexity as f64 / 50.0) * (loc as f64 / 500.0) * (cbo as f64 / 15.0);
            signals.push(Signal::new(
                SignalType::GodModule,
                node_id,
                None,
                severity,
                format!(
                    "God module: complexity {}, {} LOC, {} external dependencies (cbo). Split into focused modules.",
                    complexity, loc, cbo
                ),
            ));
        } else if loc >= thresholds.god_module_monolith_loc
            && complexity >= thresholds.god_module_monolith_complexity
            && !is_test
            && !is_generated
        {
            // Monolith detection: catches self-contained god modules with low external
            // coupling (e.g., TypeScript's checker.ts: 50K LOC, complexity 16K, CBO=0).
            // Thresholds are 10x LOC / 4x complexity vs normal god_module, so this only
            // fires on genuinely extreme cases.
            let severity = (complexity as f64 / 50.0) * (loc as f64 / 500.0);
            signals.push(Signal::new(
                SignalType::GodModule,
                node_id,
                None,
                severity,
                format!(
                    "Monolith module: complexity {}, {} LOC, {} external dependencies (low coupling but extreme size). Split into focused modules.",
                    complexity, loc, cbo
                ),
            ));
        }
    }
    signals
}

fn detect_shotgun_surgery(
    co_change_edges: &[(&str, &str, f64)],
    thresholds: &ThresholdConfig,
) -> Vec<Signal> {
    let mut co_change_breadth: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for (a, b, _) in co_change_edges {
        *co_change_breadth.entry(a).or_default() += 1;
        *co_change_breadth.entry(b).or_default() += 1;
    }

    let mut signals = Vec::new();
    for (node_id, breadth) in &co_change_breadth {
        if *breadth >= thresholds.shotgun_surgery_breadth
            && is_source_file(node_id)
            && !is_test_file(node_id)
        {
            let severity = *breadth as f64 / thresholds.shotgun_surgery_breadth as f64;
            signals.push(Signal::new(
                SignalType::ShotgunSurgery,
                node_id,
                None,
                severity,
                format!(
                    "Shotgun surgery: changes to this file co-change with {} other files. Consolidate scattered responsibilities.",
                    breadth
                ),
            ));
        }
    }
    signals
}

fn detect_unstable_dependencies(
    import_edges: &[(&str, &str, f64)],
    graph: &UnifiedGraph,
    thresholds: &ThresholdConfig,
) -> Vec<Signal> {
    let mut signals = Vec::new();
    for (a, b, _) in import_edges {
        let metrics_a = compute_node_metrics(graph, a);
        let metrics_b = compute_node_metrics(graph, b);

        let gap = metrics_b.instability - metrics_a.instability;
        if gap >= thresholds.unstable_dep_gap
            && metrics_a.instability < 0.3
            && metrics_b.instability > 0.7
            && is_source_file(a)
            && is_source_file(b)
            && !is_test_file(a)
            && !is_reexport_module(a)
            && !is_reexport_module(b)
        {
            signals.push(Signal::new(
                SignalType::UnstableDependency,
                a,
                Some(b),
                gap,
                format!(
                    "Stable module (instability {:.2}) depends on unstable module (instability {:.2}). Dependencies should flow toward stability.",
                    metrics_a.instability, metrics_b.instability
                ),
            ));
        }
    }
    signals
}

/// Detect systemic complexity: high median/P75 complexity across the codebase.
///
/// God module detection catches individual extreme modules, but misses repos like Odoo
/// where thousands of files are moderately complex (complexity 20-49, LOC 200-499)
/// without any single file exceeding god module thresholds. This signal fires when
/// the codebase as a whole has elevated complexity, indicating distributed risk that
/// individual module analysis misses.
///
/// Thresholds:
/// - Minimum 50 modules (avoids noise in tiny repos)
/// - Median complexity >= 15 OR P75 complexity >= 30
/// - Severity scales with how far above threshold the values are
fn detect_systemic_complexity(node_ids: &[String], graph: &UnifiedGraph) -> Vec<Signal> {
    let mut complexities: Vec<u32> = node_ids
        .iter()
        .filter_map(|id| {
            let node = graph.get_node(id)?;
            let c = node.complexity.unwrap_or(0);
            if c > 0 && !is_test_file(id) && !is_generated_code(id) {
                Some(c)
            } else {
                None
            }
        })
        .collect();

    if complexities.len() < 50 {
        return Vec::new();
    }

    complexities.sort_unstable();
    let n = complexities.len();
    let median = complexities[n / 2];
    let p75 = complexities[(n as f64 * 0.75) as usize];

    let mut locs: Vec<u32> = node_ids
        .iter()
        .filter_map(|id| {
            let node = graph.get_node(id)?;
            let loc = node.loc.unwrap_or(0);
            if loc > 0 && !is_test_file(id) && !is_generated_code(id) {
                Some(loc)
            } else {
                None
            }
        })
        .collect();
    locs.sort_unstable();
    let median_loc = if locs.is_empty() {
        0
    } else {
        locs[locs.len() / 2]
    };

    const MEDIAN_COMPLEXITY_THRESHOLD: u32 = 15;
    const P75_COMPLEXITY_THRESHOLD: u32 = 30;
    const MEDIAN_LOC_THRESHOLD: u32 = 150;

    let complexity_breach =
        median >= MEDIAN_COMPLEXITY_THRESHOLD || p75 >= P75_COMPLEXITY_THRESHOLD;
    let loc_breach = median_loc >= MEDIAN_LOC_THRESHOLD;

    if !complexity_breach {
        return Vec::new();
    }

    // Severity: how far above the thresholds are we?
    let complexity_ratio = (median as f64 / MEDIAN_COMPLEXITY_THRESHOLD as f64)
        .max(p75 as f64 / P75_COMPLEXITY_THRESHOLD as f64);
    let severity = (complexity_ratio - 1.0).clamp(0.1, 3.0);

    let mut signals = Vec::new();

    let loc_note = if loc_breach {
        format!(", median LOC {median_loc}")
    } else {
        String::new()
    };

    signals.push(Signal::new(
        SignalType::SystemicComplexity,
        &format!("(codebase: {n} modules)"),
        None,
        severity,
        format!(
            "Distributed complexity: median complexity {median}, P75 complexity {p75}{loc_note} across {n} modules. \
             Individual modules may not exceed god-module thresholds, but aggregate complexity is elevated. \
             Consider identifying clusters of related modules for consolidation.",
        ),
    ));

    signals
}

/// Check if a pair of paths is a test file ↔ source file pair.
fn is_test_source_pair(a: &str, b: &str) -> bool {
    let a_is_test = is_test_file(a);
    let b_is_test = is_test_file(b);
    a_is_test != b_is_test
}

fn is_test_file(path: &str) -> bool {
    ising_core::path_utils::is_test_file(path)
}

/// Check if a path is a source code file (has a recognized source extension).
/// Filters out directories, config files, docs, lock files, etc.
fn is_source_file(path: &str) -> bool {
    let source_extensions = [
        ".py", ".ts", ".tsx", ".js", ".jsx", ".rs", ".go", ".java", ".rb", ".cpp", ".cc", ".cxx",
        ".c", ".h", ".hpp", ".hh", ".hxx", ".cs", ".csx", ".swift", ".kt", ".kts", ".scala",
        ".php", ".vue",
    ];
    source_extensions.iter().any(|ext| path.ends_with(ext))
}

/// Check if a path is a Rust entry-point file (lib.rs or main.rs).
/// These files use `mod` declarations to organize crate structure — flagging
/// their imports as unnecessary abstraction is a false positive.
fn is_rust_entry_point(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename == "lib.rs" || filename == "main.rs"
}

/// Check if a path is a documentation example (e.g., docs_src/, examples/).
/// These files naturally have fan-in=1 and rarely co-change with their imports,
/// but flagging them as unnecessary abstraction or stable core is noise.
fn is_docs_example(path: &str) -> bool {
    path.starts_with("docs_src/")
        || path.starts_with("docs/")
        || path.starts_with("examples/")
        || path.starts_with("example/")
        || path.contains("/docs_src/")
        || path.contains("/examples/")
}

/// Check if two Go files are in the same package (same directory).
/// Go's package-level imports resolve to all files in the target directory,
/// creating O(N*M) import edges between packages. This inflates unnecessary
/// abstraction signals because sibling files in a package naturally don't
/// co-change with every other file they're structurally linked to.
fn is_go_intra_package_pair(a: &str, b: &str) -> bool {
    if !a.ends_with(".go") || !b.ends_with(".go") {
        return false;
    }
    let dir_a = a.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let dir_b = b.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    dir_a == dir_b
}

/// Check if two paths are in different workspace crates.
/// Cross-crate co-change is typically explained by shared workspace orchestration,
/// since cross-crate imports aren't tracked as structural edges.
fn is_cross_crate_pair(a: &str, b: &str) -> bool {
    let crate_a = extract_crate_prefix(a);
    let crate_b = extract_crate_prefix(b);
    match (crate_a, crate_b) {
        (Some(ca), Some(cb)) => ca != cb,
        _ => false,
    }
}

/// Extract the crate prefix from a workspace-relative path.
/// E.g., "ising-builders/src/change.rs" → Some("ising-builders")
///       "src/lib.rs" → None (not in a subcrate)
fn extract_crate_prefix(path: &str) -> Option<&str> {
    // Look for pattern: <crate-name>/src/...
    let (first, rest) = path.split_once('/')?;
    if rest.starts_with("src/") || rest == "src" {
        Some(first)
    } else {
        None
    }
}

/// Check if a path is generated code (protobuf, code generators, etc.).
/// These files have high complexity/LOC/fan-out but are machine-generated
/// and not actionable for refactoring.
fn is_generated_code(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    // Protobuf generated files
    filename.ends_with(".pb.go")
        || filename.ends_with("_pb.go")
        || filename.ends_with(".pb.ts")
        || filename.ends_with("_pb.ts")
        || filename.ends_with("_pb2.py")
        || filename.ends_with("_pb2_grpc.py")
        // gRPC generated files
        || filename.ends_with("_grpc.pb.go")
        // General code generation patterns
        || filename.ends_with(".generated.ts")
        || filename.ends_with(".generated.go")
        || filename.ends_with(".generated.rs")
        || filename.ends_with(".g.dart")
        || filename.ends_with(".auto.dart")
        // Django migrations
        || (path.contains("/migrations/") && filename != "__init__.py")
        // Rails schema
        || filename == "schema.rb"
        // SQLAlchemy / Alembic auto-generated
        || path.contains("/alembic/versions/")
        // Thrift / FlatBuffers generated
        || filename.ends_with("_types.go")
            && path.contains("/gen/")
        // OpenAPI / Swagger generated
        || path.contains("/generated/")
        || (path.contains("/gen/")
            && (filename.ends_with(".go")
                || filename.ends_with(".ts")
                || filename.ends_with(".py")))
        // Lock / vendor files that look like source
        || path.contains("/vendor/")
        || path.starts_with("vendor/")
        || path.contains("/third_party/")
        || path.starts_with("third_party/")
}

/// Aggregate signal counts for health index computation.
///
/// Provides density-based metrics (per-module) rather than absolute counts,
/// making comparisons across repos of different sizes meaningful.
#[derive(Debug, Clone, Default)]
pub struct SignalSummary {
    pub total_signals: usize,
    pub god_module_count: usize,
    pub cycle_count: usize,
    pub unstable_dep_count: usize,
    pub ticking_bomb_count: usize,
    pub fragile_boundary_count: usize,
    pub shotgun_surgery_count: usize,
    pub ghost_coupling_count: usize,
    pub systemic_complexity_count: usize,
}

/// Summarize signals by type for health index computation.
pub fn summarize_signals(signals: &[Signal]) -> SignalSummary {
    let mut summary = SignalSummary {
        total_signals: signals.len(),
        ..Default::default()
    };
    for signal in signals {
        match signal.signal_type {
            SignalType::GodModule => summary.god_module_count += 1,
            SignalType::DependencyCycle => summary.cycle_count += 1,
            SignalType::UnstableDependency => summary.unstable_dep_count += 1,
            SignalType::TickingBomb => summary.ticking_bomb_count += 1,
            SignalType::FragileBoundary => summary.fragile_boundary_count += 1,
            SignalType::ShotgunSurgery => summary.shotgun_surgery_count += 1,
            SignalType::GhostCoupling => summary.ghost_coupling_count += 1,
            SignalType::SystemicComplexity => summary.systemic_complexity_count += 1,
            SignalType::StableCore
            | SignalType::UnnecessaryAbstraction
            | SignalType::OrphanFunction
            | SignalType::OrphanModule
            | SignalType::DeprecatedUsage
            | SignalType::StaleCode
            | SignalType::IntraFileHotspot
            | SignalType::BoundaryLeakage => {}
        }
    }
    summary
}

/// Detect boundary leakage: modules where >30% of change edges cross into another
/// module despite low structural coupling. Indicates hidden cross-boundary dependency.
fn detect_boundary_leakage(
    co_change_edges: &[(&str, &str, f64)],
    _graph: &UnifiedGraph,
    boundaries: &BoundaryStructure,
) -> Vec<Signal> {
    use std::collections::HashMap;

    // Count per-module: total change edges and cross-boundary change edges
    let mut total_edges: HashMap<(&str, &str), usize> = HashMap::new();
    let mut cross_edges: HashMap<(&str, &str), usize> = HashMap::new();

    for (a, b, _coupling) in co_change_edges {
        if !is_source_file(a) || !is_source_file(b) {
            continue;
        }
        let (pkg_a, mod_a) = boundaries.module_of(a);
        let (pkg_b, mod_b) = boundaries.module_of(b);

        *total_edges.entry((pkg_a, mod_a)).or_default() += 1;
        *total_edges.entry((pkg_b, mod_b)).or_default() += 1;

        if !boundaries.same_module(a, b) {
            *cross_edges.entry((pkg_a, mod_a)).or_default() += 1;
            *cross_edges.entry((pkg_b, mod_b)).or_default() += 1;
        }
    }

    let mut signals = Vec::new();
    for (&(pkg, module), &total) in &total_edges {
        if total < 3 {
            continue; // Too few edges to be meaningful
        }
        let cross = cross_edges.get(&(pkg, module)).copied().unwrap_or(0);
        let leakage_ratio = cross as f64 / total as f64;

        if leakage_ratio > 0.3 {
            let module_id = if module == "_root" {
                pkg.to_string()
            } else {
                format!("{}::{}", pkg, module)
            };
            signals.push(Signal::new(
                SignalType::BoundaryLeakage,
                &module_id,
                None,
                leakage_ratio,
                format!(
                    "{:.0}% of change edges cross module boundary ({} of {} edges). Potential encapsulation issue.",
                    leakage_ratio * 100.0,
                    cross,
                    total,
                ),
            ));
        }
    }
    signals
}

fn is_reexport_module(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename == "__init__.py"
        || filename == "index.ts"
        || filename == "index.js"
        || filename == "mod.rs"
}

/// Detect functions with zero incoming Calls edges (potential dead code).
///
/// Excludes entry points: main, __init__, test functions, init, new, etc.
fn detect_orphan_functions(graph: &UnifiedGraph) -> Vec<Signal> {
    use ising_core::graph::NodeType;

    let calls_edges = graph.edges_of_type(&EdgeType::Calls);

    // Build set of all function IDs that are called by someone
    let mut called_functions: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (_, target, _) in &calls_edges {
        called_functions.insert(target);
    }

    let mut signals = Vec::new();
    for node_id in graph.node_ids() {
        let node = match graph.get_node(node_id) {
            Some(n) if n.node_type == NodeType::Function => n,
            _ => continue,
        };

        // Skip if this function is called by anyone
        if called_functions.contains(node_id) {
            continue;
        }

        // Skip entry points and common patterns
        let func_name = node_id.rsplit("::").next().unwrap_or(node_id);
        if is_entry_point_function(func_name) {
            continue;
        }

        // Skip trait impl methods and common framework patterns that are called
        // implicitly by the language runtime, framework, or via dispatch
        if is_trait_or_framework_method(func_name) {
            continue;
        }

        // Skip test functions
        if func_name.starts_with("test_") || func_name.starts_with("Test") {
            continue;
        }

        // Skip if the function is in a test file
        if is_test_file(&node.file_path) {
            continue;
        }

        // Skip functions in crate entry points (lib.rs, main.rs, mod.rs) — these
        // are public API that gets called from other crates or the binary
        if is_entry_point_file(&node.file_path) {
            continue;
        }

        // Skip functions in config/standalone files (scripts/, bin/, .config.ts, etc.)
        if is_config_or_standalone_file(&node.file_path) {
            continue;
        }

        // Skip React/Vue components: PascalCase functions in .tsx/.jsx/.vue files
        // are called implicitly by the framework's component rendering
        if is_component_function(func_name, &node.file_path) {
            continue;
        }

        // Skip methods on structs/classes — "Type::method" pattern indicates it's
        // likely called via method dispatch (obj.method()) which our call extraction
        // may not resolve across files
        if is_qualified_method(node_id) {
            continue;
        }

        signals.push(Signal::new(
            SignalType::OrphanFunction,
            node_id,
            None,
            0.5,
            format!(
                "Function '{}' has no detected callers — may be unused or called dynamically",
                func_name
            ),
        ));
    }
    signals
}

/// Check if a function name matches common trait impl or framework callback patterns
/// that are called implicitly (not via direct function call).
fn is_trait_or_framework_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    // Rust trait impls
    if matches!(
        lower.as_str(),
        "fmt"
            | "default"
            | "clone"
            | "drop"
            | "deref"
            | "deref_mut"
            | "from"
            | "into"
            | "try_from"
            | "try_into"
            | "as_ref"
            | "as_mut"
            | "serialize"
            | "deserialize"
            | "eq"
            | "partial_cmp"
            | "cmp"
            | "hash"
            | "display"
            | "debug"
            | "index"
            | "next"
            | "poll"
            | "call"
    ) {
        return true;
    }
    // Common "from_*" / "into_*" conversion constructors
    if lower.starts_with("from_") || lower.starts_with("into_") || lower.starts_with("try_from_") {
        return true;
    }
    // Python dunder methods (called by runtime)
    if name.starts_with("__") && name.ends_with("__") {
        return true;
    }
    // Java/C#/Kotlin overrides
    if matches!(
        lower.as_str(),
        "tostring" | "hashcode" | "equals" | "compareto" | "dispose" | "finalize" | "gettype"
    ) {
        return true;
    }
    false
}

/// Check if a function looks like a React/Vue component (PascalCase in .tsx/.jsx/.vue).
fn is_component_function(name: &str, file_path: &str) -> bool {
    let is_component_file =
        file_path.ends_with(".tsx") || file_path.ends_with(".jsx") || file_path.ends_with(".vue");
    if !is_component_file {
        return false;
    }
    // PascalCase: starts with uppercase, has at least one lowercase
    name.starts_with(|c: char| c.is_ascii_uppercase())
        && name.contains(|c: char| c.is_ascii_lowercase())
}

/// Check if a node_id represents a qualified method (e.g. "file.rs::Struct::method").
/// Methods are often called via dispatch (obj.method()) which static call extraction
/// can miss across file boundaries.
fn is_qualified_method(node_id: &str) -> bool {
    // Count "::" segments — "file::Type::method" has 3 parts, "file::func" has 2
    node_id.split("::").count() >= 3
}

/// Check if a function name is a common entry point.
fn is_entry_point_function(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "main"
            | "__init__"
            | "__new__"
            | "init"
            | "new"
            | "setup"
            | "teardown"
            | "configure"
            | "run"
            | "start"
            | "serve"
            | "handle"
            | "handler"
            | "middleware"
    )
}

/// Check if a file is a common entry point.
fn is_entry_point_file(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let lower = filename.to_lowercase();
    matches!(
        lower.as_str(),
        "main.py"
            | "main.rs"
            | "main.go"
            | "main.ts"
            | "main.js"
            | "main.tsx"
            | "index.ts"
            | "index.js"
            | "index.tsx"
            | "index.jsx"
            | "lib.rs"
            | "mod.rs"
            | "app.py"
            | "app.ts"
            | "app.js"
            | "app.tsx"
            | "app.jsx"
            | "__init__.py"
            | "setup.py"
            | "conftest.py"
            | "manage.py"
            | "program.cs"
            | "startup.cs"
    )
}

/// Check if a file is a config/build file that's consumed by tooling, not imported.
fn is_config_or_standalone_file(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let lower = filename.to_lowercase();

    // Config files consumed by bundlers/tools — never imported by application code
    if lower.ends_with(".config.js")
        || lower.ends_with(".config.ts")
        || lower.ends_with(".config.mjs")
        || lower.ends_with(".config.cjs")
    {
        return true;
    }

    // Type declaration files — consumed by the compiler, not imported
    if lower.ends_with(".d.ts") {
        return true;
    }

    // Environment declaration files (vite-env.d.ts, env.d.ts, etc.)
    if lower.contains("-env.") && lower.ends_with(".ts") {
        return true;
    }

    // Standalone scripts in a scripts/ directory — run directly, not imported
    if path.starts_with("scripts/") || path.contains("/scripts/") {
        return true;
    }

    // Package entry points (bin/ directory files)
    if path.starts_with("bin/") || path.contains("/bin/") {
        return true;
    }

    false
}

/// Detect modules with zero incoming Imports edges (potential dead code).
///
/// Excludes entry points: main.py, index.ts, lib.rs, etc.
fn detect_orphan_modules(graph: &UnifiedGraph, import_edges: &[(&str, &str, f64)]) -> Vec<Signal> {
    use ising_core::graph::NodeType;

    // Build set of all module IDs that are imported by someone
    let mut imported_modules: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (_, target, _) in import_edges {
        imported_modules.insert(target);
    }

    let mut signals = Vec::new();
    for node_id in graph.node_ids() {
        let _node = match graph.get_node(node_id) {
            Some(n) if n.node_type == NodeType::Module => n,
            _ => continue,
        };

        // Skip if this module is imported by anyone
        if imported_modules.contains(node_id) {
            continue;
        }

        // Skip entry point files
        if is_entry_point_file(node_id) {
            continue;
        }

        // Skip config files, scripts, and standalone tooling files
        if is_config_or_standalone_file(node_id) {
            continue;
        }

        // Skip test files
        if is_test_file(node_id) {
            continue;
        }

        // Only flag if the module has been around long enough (has change metrics)
        // but hasn't been changed recently — avoids flagging brand new files
        let has_history = graph.change_metrics.contains_key(node_id);
        if !has_history {
            continue;
        }

        signals.push(Signal::new(
            SignalType::OrphanModule,
            node_id,
            None,
            0.3,
            format!(
                "Module '{}' is not imported by any other module — may be an unused entry point or dead code",
                node_id
            ),
        ));
    }
    signals
}

/// Detect usage of deprecated symbols.
///
/// A deprecated function/class that is still being called or imported is a migration risk.
fn detect_deprecated_usage(graph: &UnifiedGraph) -> Vec<Signal> {
    let calls_edges = graph.edges_of_type(&EdgeType::Calls);
    let import_edges = graph.edges_of_type(&EdgeType::Imports);

    let mut signals = Vec::new();

    // Check for calls to deprecated functions
    for (caller, callee, _) in &calls_edges {
        if let Some(node) = graph.get_node(callee)
            && node.deprecated
        {
            let callee_name = callee.rsplit("::").next().unwrap_or(callee);
            signals.push(Signal::new(
                SignalType::DeprecatedUsage,
                caller,
                Some(callee),
                1.5,
                format!(
                    "'{}' calls deprecated function '{}' — should be migrated",
                    caller, callee_name
                ),
            ));
        }
    }

    // Check for imports of deprecated modules
    for (importer, imported, _) in &import_edges {
        if let Some(node) = graph.get_node(imported)
            && node.deprecated
        {
            signals.push(Signal::new(
                SignalType::DeprecatedUsage,
                importer,
                Some(imported),
                1.0,
                format!(
                    "'{}' imports deprecated module '{}' — should be migrated",
                    importer, imported
                ),
            ));
        }
    }

    signals
}

/// Detect stale code — modules/functions unchanged for a long period with low connectivity.
///
/// Distinguishes "stable" (heavily depended upon, unchanged) from "stale" (nobody depends
/// on it, unchanged for > 12 months).
fn detect_stale_code(graph: &UnifiedGraph) -> Vec<Signal> {
    use ising_core::graph::NodeType;

    let now_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let stale_threshold_seconds: i64 = 365 * 86_400; // 1 year

    // Pre-compute incoming counts per module (hoist outside the loop to avoid O(modules * edges))
    let import_edges = graph.edges_of_type(&EdgeType::Imports);
    let calls_edges = graph.edges_of_type(&EdgeType::Calls);
    let mut incoming_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for (_, target, _) in &import_edges {
        *incoming_counts.entry(target).or_default() += 1;
    }
    // Count only external calls (caller is outside the module) to avoid
    // internally-chatty modules looking "heavily depended upon"
    for (caller, callee, _) in &calls_edges {
        if let Some(module_id) = callee.split("::").next()
            && !caller.starts_with(module_id)
        {
            *incoming_counts.entry(module_id).or_default() += 1;
        }
    }

    let mut signals = Vec::new();

    for node_id in graph.node_ids() {
        let _node = match graph.get_node(node_id) {
            Some(n) if n.node_type == NodeType::Module => n,
            _ => continue,
        };

        let metrics = match graph.change_metrics.get(node_id) {
            Some(m) => m,
            None => continue,
        };

        let last_changed_ts = metrics.last_changed.as_ref().and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.timestamp())
        });

        let age_seconds = match last_changed_ts {
            Some(ts) => now_timestamp - ts,
            None => continue,
        };

        if age_seconds < stale_threshold_seconds {
            continue;
        }

        let incoming_count = incoming_counts.get(node_id).copied().unwrap_or(0);

        // If heavily depended upon (>3 external importers/callers), it's stable not stale
        if incoming_count > 3 {
            continue;
        }

        if is_test_file(node_id) {
            continue;
        }

        let months = age_seconds / (30 * 86_400);
        signals.push(Signal::new(
            SignalType::StaleCode,
            node_id,
            None,
            0.3,
            format!(
                "Module '{}' unchanged for ~{} months with only {} dependent(s) — may be obsolete",
                node_id, months, incoming_count
            ),
        ));
    }

    signals
}

/// Detect intra-file hotspots — functions that churn far more than their siblings.
///
/// Uses function-level change metrics (from proportional attribution) to find
/// functions that have 3x or more churn than the median function in the same file.
fn detect_intra_file_hotspots(graph: &UnifiedGraph) -> Vec<Signal> {
    use ising_core::graph::NodeType;

    // Group functions by their parent module
    let contains_edges = graph.edges_of_type(&EdgeType::Contains);
    let mut module_functions: std::collections::HashMap<&str, Vec<(&str, f64)>> =
        std::collections::HashMap::new();

    for (module_id, func_id, _) in &contains_edges {
        if let Some(node) = graph.get_node(func_id)
            && node.node_type != NodeType::Function
        {
            continue;
        }
        let churn = graph
            .change_metrics
            .get(*func_id)
            .map(|m| m.churn_lines as f64)
            .unwrap_or(0.0);
        module_functions
            .entry(module_id)
            .or_default()
            .push((func_id, churn));
    }

    let mut signals = Vec::new();
    let hotspot_ratio = 3.0;

    for (module_id, funcs) in &module_functions {
        if funcs.len() < 3 {
            continue; // Need enough siblings for comparison
        }

        // Compute median churn
        let mut churns: Vec<f64> = funcs.iter().map(|(_, c)| *c).collect();
        churns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = churns[churns.len() / 2];

        if median < 1.0 {
            continue; // No meaningful churn to compare against
        }

        for (func_id, churn) in funcs {
            if *churn > median * hotspot_ratio {
                let func_name = func_id.rsplit("::").next().unwrap_or(func_id);
                signals.push(Signal::new(
                    SignalType::IntraFileHotspot,
                    func_id,
                    Some(module_id),
                    (*churn / median).min(5.0),
                    format!(
                        "Function '{}' churns {:.1}x more than siblings in '{}' — consider extracting",
                        func_name,
                        churn / median,
                        module_id
                    ),
                ));
            }
        }
    }

    signals
}

#[cfg(test)]
mod tests {
    use super::*;
    use ising_core::graph::Node;

    fn default_config() -> Config {
        Config::default()
    }

    #[test]
    fn test_ghost_coupling_detected() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        // No structural edge, but high co-change
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.8)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::GhostCoupling)
        );
    }

    #[test]
    fn test_no_ghost_coupling_when_structural_edge_exists() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        g.add_edge("a.py", "b.py", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.8)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::GhostCoupling)
        );
    }

    #[test]
    fn test_fragile_boundary_detected() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("a", "b", EdgeType::CoChanges, 0.7).unwrap();
        g.add_edge("a", "b", EdgeType::FaultPropagates, 0.2)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::FragileBoundary)
        );
    }

    #[test]
    fn test_unnecessary_abstraction_single_consumer_wrapper() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        // b is a trivial single-consumer module: low complexity, never changes
        let mut b_node = Node::module("b", "b.py");
        b_node.complexity = Some(2);
        g.add_node(b_node);
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        // No co-change, b has fan-in=1 and low complexity → unnecessary abstraction

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::UnnecessaryAbstraction)
        );
    }

    #[test]
    fn test_no_unnecessary_abstraction_for_stable_dependency() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        // b is used by multiple consumers — not a single-consumer wrapper
        let mut b_node = Node::module("b", "b.py");
        b_node.complexity = Some(20);
        g.add_node(b_node);
        g.add_node(Node::module("c", "c.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("c", "b", EdgeType::Imports, 1.0).unwrap();
        // b has fan-in=2 — not a single-consumer wrapper

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::UnnecessaryAbstraction)
        );
    }

    #[test]
    fn test_unnecessary_abstraction_pass_through() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        // b is non-trivial (high complexity) so it doesn't match single-consumer wrapper
        let mut b_node = Node::module("b", "b.py");
        b_node.complexity = Some(30);
        g.add_node(b_node);
        g.add_node(Node::module("c", "c.py"));
        // A→B→C import chain
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("b", "c", EdgeType::Imports, 1.0).unwrap();
        // A and C co-change heavily, but B is dormant
        g.add_edge("a", "c", EdgeType::CoChanges, 0.8).unwrap();
        // No A↔B or B↔C co-change

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::UnnecessaryAbstraction
                    && s.description.contains("Pass-through"))
        );
    }

    #[test]
    fn test_signals_sorted_by_severity() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        g.add_node(Node::module("c.py", "c.py"));
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.6)
            .unwrap();
        g.add_edge("a.py", "c.py", EdgeType::CoChanges, 0.9)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        for w in signals.windows(2) {
            assert!(w[0].severity >= w[1].severity);
        }
    }

    #[test]
    fn test_ghost_coupling_suppressed_by_common_parent() {
        // A and B are siblings imported by parent C — no ghost coupling
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        g.add_node(Node::module("parent.py", "parent.py"));
        // Parent imports both A and B
        g.add_edge("parent.py", "a.py", EdgeType::Imports, 1.0)
            .unwrap();
        g.add_edge("parent.py", "b.py", EdgeType::Imports, 1.0)
            .unwrap();
        // A and B co-change at 80% but have no direct structural edge
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.8)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::GhostCoupling),
            "Ghost coupling should be suppressed when siblings share a common parent"
        );
    }

    #[test]
    fn test_ghost_coupling_common_parent_very_high_coupling_reduced() {
        // A and B share a parent but have ≥0.9 coupling — emit at reduced severity
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        g.add_node(Node::module("parent.py", "parent.py"));
        g.add_edge("parent.py", "a.py", EdgeType::Imports, 1.0)
            .unwrap();
        g.add_edge("parent.py", "b.py", EdgeType::Imports, 1.0)
            .unwrap();
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.95)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        let ghost = signals
            .iter()
            .find(|s| s.signal_type == SignalType::GhostCoupling);
        assert!(
            ghost.is_some(),
            "Ghost coupling should still fire for very high coupling (≥0.9) with common parent"
        );
        let ghost = ghost.unwrap();
        // Severity should be reduced: 0.95 * 0.3 = 0.285
        assert!(
            ghost.severity < 0.5,
            "Severity should be reduced (got {})",
            ghost.severity
        );
        assert!(
            ghost.description.contains("shared parent"),
            "Description should mention shared parent"
        );
    }

    #[test]
    fn test_ghost_coupling_no_common_parent_still_fires() {
        // A and B have no common parent — ghost coupling should fire as before
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        // No import edges, just co-change
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.8)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::GhostCoupling),
            "Ghost coupling should still fire when no common parent exists"
        );
    }

    #[test]
    fn test_mod_rs_recognized_as_reexport_module() {
        assert!(is_reexport_module("src/languages/mod.rs"));
        assert!(is_reexport_module("mod.rs"));
    }

    #[test]
    fn test_lib_rs_not_recognized_as_reexport_module() {
        // lib.rs may contain real logic — don't blanket-recognize it
        assert!(!is_reexport_module("src/lib.rs"));
        assert!(!is_reexport_module("lib.rs"));
    }

    #[test]
    fn test_no_unnecessary_abstraction_for_mod_rs() {
        // mod.rs barrel files should not trigger unnecessary abstraction
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("src/lib.rs", "src/lib.rs"));
        let mut mod_node = Node::module("src/languages/mod.rs", "src/languages/mod.rs");
        mod_node.complexity = Some(2);
        g.add_node(mod_node);
        g.add_edge("src/lib.rs", "src/languages/mod.rs", EdgeType::Imports, 1.0)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::UnnecessaryAbstraction),
            "mod.rs barrel files should not trigger unnecessary abstraction signals"
        );
    }

    #[test]
    fn test_cross_crate_pair_detection() {
        assert!(is_cross_crate_pair(
            "crate-a/src/foo.rs",
            "crate-b/src/bar.rs"
        ));
        assert!(!is_cross_crate_pair(
            "crate-a/src/foo.rs",
            "crate-a/src/bar.rs"
        ));
        // Not in subcrates (no crate prefix)
        assert!(!is_cross_crate_pair("src/foo.rs", "src/bar.rs"));
    }

    #[test]
    fn test_ghost_coupling_suppressed_cross_crate() {
        // Files in different workspace crates should not trigger ghost coupling
        // because cross-crate imports aren't tracked as structural edges
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("crate-a/src/foo.rs", "crate-a/src/foo.rs"));
        g.add_node(Node::module("crate-b/src/bar.rs", "crate-b/src/bar.rs"));
        // High co-change but no structural edge (cross-crate)
        g.add_edge(
            "crate-a/src/foo.rs",
            "crate-b/src/bar.rs",
            EdgeType::CoChanges,
            0.8,
        )
        .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::GhostCoupling),
            "Ghost coupling should be suppressed for cross-crate pairs"
        );
    }

    #[test]
    fn test_ghost_coupling_same_crate_no_parent_still_fires() {
        // Files in the same crate without a common parent should still fire
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("mycrate/src/foo.rs", "mycrate/src/foo.rs"));
        g.add_node(Node::module("mycrate/src/bar.rs", "mycrate/src/bar.rs"));
        g.add_edge(
            "mycrate/src/foo.rs",
            "mycrate/src/bar.rs",
            EdgeType::CoChanges,
            0.8,
        )
        .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::GhostCoupling),
            "Ghost coupling should still fire for same-crate files without a common parent"
        );
    }

    // --- DependencyCycle tests ---

    #[test]
    fn test_dependency_cycle_detected() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        g.add_edge("a.py", "b.py", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("b.py", "a.py", EdgeType::Imports, 1.0).unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::DependencyCycle),
            "Should detect circular dependency between a.py and b.py"
        );
    }

    #[test]
    fn test_no_dependency_cycle_for_acyclic() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        g.add_node(Node::module("c.py", "c.py"));
        g.add_edge("a.py", "b.py", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("b.py", "c.py", EdgeType::Imports, 1.0).unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::DependencyCycle),
            "Acyclic graph should not trigger dependency cycle signal"
        );
    }

    #[test]
    fn test_dependency_cycle_three_nodes() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.rs", "a.rs"));
        g.add_node(Node::module("b.rs", "b.rs"));
        g.add_node(Node::module("c.rs", "c.rs"));
        g.add_edge("a.rs", "b.rs", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("b.rs", "c.rs", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("c.rs", "a.rs", EdgeType::Imports, 1.0).unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        let cycle = signals
            .iter()
            .find(|s| s.signal_type == SignalType::DependencyCycle);
        assert!(cycle.is_some(), "Should detect 3-node cycle");
        // Severity should be proportional to cycle length
        assert!(
            cycle.unwrap().severity >= 1.5,
            "3-node cycle should have severity >= 1.5"
        );
    }

    // --- GodModule tests ---

    #[test]
    fn test_god_module_detected() {
        let mut g = UnifiedGraph::new();
        // Create a god module with high complexity, LOC, and fan-out
        let mut god = Node::module("god.py", "god.py");
        god.complexity = Some(80);
        god.loc = Some(1200);
        g.add_node(god);

        // Add many import targets to give it high CBO (distinct external modules)
        for i in 0..20 {
            let dep_id = format!("dep{}.py", i);
            g.add_node(Node::module(dep_id.clone(), dep_id.clone()));
            g.add_edge("god.py", &dep_id, EdgeType::Imports, 1.0)
                .unwrap();
        }

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::GodModule),
            "Should detect god module with high complexity, LOC, and external dependencies"
        );
    }

    #[test]
    fn test_no_god_module_for_simple_file() {
        let mut g = UnifiedGraph::new();
        let mut simple = Node::module("simple.py", "simple.py");
        simple.complexity = Some(5);
        simple.loc = Some(50);
        g.add_node(simple);

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::GodModule),
            "Simple files should not trigger god module"
        );
    }

    #[test]
    fn test_no_god_module_for_many_inner_functions_low_cbo() {
        // A file with many inner functions (high fan_out via Contains edges)
        // but only one external dependency (low cbo) should NOT be flagged.
        // This tests the fix: GodModule uses cbo, not fan_out.
        let mut g = UnifiedGraph::new();
        let mut big = Node::module("big.rs", "big.rs");
        big.complexity = Some(120);
        big.loc = Some(800);
        g.add_node(big);

        // Add 20 inner function nodes in the SAME file (Contains edges)
        for i in 0..20 {
            let fn_id = format!("big.rs::fn_{}", i);
            let fn_node = Node::module(fn_id.clone(), "big.rs"); // same file_path
            g.add_node(fn_node);
            g.add_edge("big.rs", &fn_id, EdgeType::Contains, 1.0)
                .unwrap();
        }

        // One external import
        g.add_node(Node::module("util.rs", "util.rs"));
        g.add_edge("big.rs", "util.rs", EdgeType::Imports, 1.0)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::GodModule),
            "File with many inner functions but only 1 external dep (cbo=1) should not be GodModule"
        );
    }

    #[test]
    fn test_god_module_fires_for_high_cbo() {
        // A file that imports 15+ distinct external modules should trigger GodModule.
        let mut g = UnifiedGraph::new();
        let mut hub = Node::module("hub.rs", "hub.rs");
        hub.complexity = Some(80);
        hub.loc = Some(600);
        g.add_node(hub);

        // 15 imports to distinct external files
        for i in 0..15 {
            let ext_id = format!("ext{}.rs", i);
            g.add_node(Node::module(ext_id.clone(), ext_id.clone()));
            g.add_edge("hub.rs", &ext_id, EdgeType::Imports, 1.0)
                .unwrap();
        }

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::GodModule),
            "File importing 15 distinct external modules (cbo=15) should trigger GodModule"
        );
    }

    // --- ShotgunSurgery tests ---

    #[test]
    fn test_shotgun_surgery_detected() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("core.py", "core.py"));
        // Create many files that co-change with core.py
        for i in 0..10 {
            let dep_id = format!("dep{}.py", i);
            g.add_node(Node::module(dep_id.clone(), dep_id.clone()));
            g.add_edge("core.py", &dep_id, EdgeType::CoChanges, 0.6)
                .unwrap();
        }

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::ShotgunSurgery),
            "Should detect shotgun surgery when many files co-change"
        );
    }

    #[test]
    fn test_no_shotgun_surgery_for_few_cochanges() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        g.add_node(Node::module("c.py", "c.py"));
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.6)
            .unwrap();
        g.add_edge("a.py", "c.py", EdgeType::CoChanges, 0.6)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::ShotgunSurgery),
            "Few co-changes should not trigger shotgun surgery"
        );
    }

    // --- UnstableDependency tests ---

    #[test]
    fn test_unstable_dependency_detected() {
        let mut g = UnifiedGraph::new();
        // A: stable (high fan-in, no fan-out besides this import)
        g.add_node(Node::module("stable.py", "stable.py"));
        // Give stable.py high fan-in from many consumers
        for i in 0..5 {
            let consumer = format!("consumer{}.py", i);
            g.add_node(Node::module(consumer.clone(), consumer.clone()));
            g.add_edge(&consumer, "stable.py", EdgeType::Imports, 1.0)
                .unwrap();
        }

        // B: unstable (no fan-in, high fan-out)
        g.add_node(Node::module("unstable.py", "unstable.py"));
        for i in 0..5 {
            let dep = format!("lib{}.py", i);
            g.add_node(Node::module(dep.clone(), dep.clone()));
            g.add_edge("unstable.py", &dep, EdgeType::Imports, 1.0)
                .unwrap();
        }

        // Stable depends on unstable — SDP violation
        g.add_edge("stable.py", "unstable.py", EdgeType::Imports, 1.0)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::UnstableDependency),
            "Should detect stable module depending on unstable module"
        );
    }

    #[test]
    fn test_no_unstable_dependency_for_same_stability() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        g.add_edge("a.py", "b.py", EdgeType::Imports, 1.0).unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::UnstableDependency),
            "Modules with similar stability should not trigger signal"
        );
    }

    // --- Generated code filtering tests ---

    #[test]
    fn test_is_generated_code() {
        assert!(is_generated_code("grpc/model_service_v2_request.pb.go"));
        assert!(is_generated_code("grpc/service_grpc.pb.go"));
        assert!(is_generated_code("api/types_pb.ts"));
        assert!(is_generated_code("proto/model_pb2.py"));
        assert!(is_generated_code("proto/model_pb2_grpc.py"));
        assert!(is_generated_code("src/schema.generated.ts"));
        assert!(is_generated_code("lib/model.g.dart"));
        assert!(!is_generated_code("vcs/git.go"));
        assert!(!is_generated_code("src/main.rs"));
        assert!(!is_generated_code("api/handler.ts"));
    }

    // --- OrphanFunction tests ---

    #[test]
    fn test_orphan_function_detected() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("utils.py", "utils.py"));
        let f = Node::function("utils.py::unused_helper", "utils.py", 10, 20);
        g.add_node(f);
        g.add_edge(
            "utils.py",
            "utils.py::unused_helper",
            EdgeType::Contains,
            1.0,
        )
        .unwrap();
        // No Calls edges pointing to this function

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::OrphanFunction),
            "Function with no callers should be flagged as orphan"
        );
    }

    #[test]
    fn test_orphan_function_not_detected_when_called() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("app.py", "app.py"));
        let f1 = Node::function("app.py::helper", "app.py", 10, 20);
        let f2 = Node::function("app.py::main_fn", "app.py", 25, 40);
        g.add_node(f1);
        g.add_node(f2);
        g.add_edge("app.py::main_fn", "app.py::helper", EdgeType::Calls, 1.0)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals.iter().any(
                |s| s.signal_type == SignalType::OrphanFunction && s.node_a == "app.py::helper"
            ),
            "Function with callers should not be flagged as orphan"
        );
    }

    #[test]
    fn test_orphan_function_skips_entry_points() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("app.py", "app.py"));
        let f = Node::function("app.py::main", "app.py", 1, 10);
        g.add_node(f);
        // No callers, but "main" is an entry point

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::OrphanFunction && s.node_a == "app.py::main"),
            "Entry point functions should not be flagged as orphan"
        );
    }

    // --- OrphanModule tests ---

    #[test]
    fn test_orphan_module_detected() {
        use ising_core::graph::ChangeMetrics;
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("orphan.py", "orphan.py"));
        g.change_metrics.insert(
            "orphan.py".to_string(),
            ChangeMetrics {
                change_freq: 5,
                ..Default::default()
            },
        );
        // No import edges pointing to this module

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::OrphanModule),
            "Module with no importers should be flagged as orphan"
        );
    }

    #[test]
    fn test_orphan_module_not_detected_when_imported() {
        use ising_core::graph::ChangeMetrics;
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("lib.py", "lib.py"));
        g.add_node(Node::module("app.py", "app.py"));
        g.add_edge("app.py", "lib.py", EdgeType::Imports, 1.0)
            .unwrap();
        g.change_metrics.insert(
            "lib.py".to_string(),
            ChangeMetrics {
                change_freq: 5,
                ..Default::default()
            },
        );

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::OrphanModule && s.node_a == "lib.py"),
            "Module with importers should not be flagged as orphan"
        );
    }

    #[test]
    fn test_orphan_module_skips_entry_points() {
        use ising_core::graph::ChangeMetrics;
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("main.py", "main.py"));
        g.change_metrics.insert(
            "main.py".to_string(),
            ChangeMetrics {
                change_freq: 10,
                ..Default::default()
            },
        );

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::OrphanModule && s.node_a == "main.py"),
            "Entry point files should not be flagged as orphan modules"
        );
    }

    // --- DeprecatedUsage tests ---

    #[test]
    fn test_deprecated_usage_via_calls() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("app.py", "app.py"));
        let mut f = Node::function("lib.py::old_func", "lib.py", 10, 20);
        f.deprecated = true;
        g.add_node(f);
        g.add_edge("app.py", "lib.py::old_func", EdgeType::Calls, 1.0)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::DeprecatedUsage),
            "Calling a deprecated function should trigger DeprecatedUsage"
        );
    }

    #[test]
    fn test_deprecated_usage_via_imports() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("app.py", "app.py"));
        let mut dep = Node::module("old_module.py", "old_module.py");
        dep.deprecated = true;
        g.add_node(dep);
        g.add_edge("app.py", "old_module.py", EdgeType::Imports, 1.0)
            .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::DeprecatedUsage),
            "Importing a deprecated module should trigger DeprecatedUsage"
        );
    }

    // --- IntraFileHotspot tests ---

    #[test]
    fn test_intra_file_hotspot_detected() {
        use ising_core::graph::ChangeMetrics;
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("big.py", "big.py"));

        // Add 4 functions: 3 normal, 1 hot
        for i in 0..3 {
            let fid = format!("big.py::func_{}", i);
            let f = Node::function(&fid, "big.py", i * 10, i * 10 + 8);
            g.add_node(f);
            g.add_edge("big.py", &fid, EdgeType::Contains, 1.0).unwrap();
            g.change_metrics.insert(
                fid,
                ChangeMetrics {
                    churn_lines: 10,
                    change_freq: 5,
                    ..Default::default()
                },
            );
        }
        // Hot function: 50 churn vs median of 10 = 5x
        let hot_id = "big.py::hot_func";
        let f = Node::function(hot_id, "big.py", 30, 38);
        g.add_node(f);
        g.add_edge("big.py", hot_id, EdgeType::Contains, 1.0)
            .unwrap();
        g.change_metrics.insert(
            hot_id.to_string(),
            ChangeMetrics {
                churn_lines: 50,
                change_freq: 20,
                ..Default::default()
            },
        );

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::IntraFileHotspot
                    && s.node_a == "big.py::hot_func"),
            "Function with 5x median churn should be flagged as intra-file hotspot"
        );
    }

    // --- Entry point helpers tests ---

    #[test]
    fn test_is_entry_point_function_common_names() {
        assert!(is_entry_point_function("main"));
        assert!(is_entry_point_function("__init__"));
        assert!(is_entry_point_function("setup"));
        assert!(!is_entry_point_function("process_data"));
        assert!(!is_entry_point_function("calculate"));
    }

    #[test]
    fn test_is_entry_point_file_common_names() {
        assert!(is_entry_point_file("main.py"));
        assert!(is_entry_point_file("src/index.ts"));
        assert!(is_entry_point_file("lib.rs"));
        assert!(is_entry_point_file("__init__.py"));
        assert!(is_entry_point_file("src/main.tsx"));
        assert!(!is_entry_point_file("utils.py"));
        assert!(!is_entry_point_file("src/handler.rs"));
    }

    #[test]
    fn test_trait_method_not_flagged_as_orphan() {
        assert!(is_trait_or_framework_method("fmt"));
        assert!(is_trait_or_framework_method("default"));
        assert!(is_trait_or_framework_method("clone"));
        assert!(is_trait_or_framework_method("from_factor"));
        assert!(is_trait_or_framework_method("__str__"));
        assert!(is_trait_or_framework_method("toString"));
        assert!(!is_trait_or_framework_method("process_data"));
        assert!(!is_trait_or_framework_method("calculate_risk"));
    }

    #[test]
    fn test_component_function_not_flagged_as_orphan() {
        assert!(is_component_function(
            "BlastRadius",
            "src/views/BlastRadius.tsx"
        ));
        assert!(is_component_function("App", "src/App.jsx"));
        assert!(!is_component_function("BlastRadius", "src/views/blast.rs"));
        assert!(!is_component_function("helper", "src/views/Foo.tsx"));
    }

    #[test]
    fn test_qualified_method_not_flagged_as_orphan() {
        assert!(is_qualified_method("file.rs::Database::open"));
        assert!(is_qualified_method("lib.rs::ScipLoader::load_from_index"));
        assert!(!is_qualified_method("file.rs::standalone_func"));
        assert!(!is_qualified_method("extract_nodes"));
    }

    #[test]
    fn test_config_or_standalone_file() {
        assert!(is_config_or_standalone_file("vite.config.ts"));
        assert!(is_config_or_standalone_file("postcss.config.js"));
        assert!(is_config_or_standalone_file("scripts/publish.ts"));
        assert!(is_config_or_standalone_file("src/vite-env.d.ts"));
        assert!(is_config_or_standalone_file("packages/cli/bin/ising.js"));
        assert!(!is_config_or_standalone_file("src/main.ts"));
        assert!(!is_config_or_standalone_file("src/utils.rs"));
    }

    #[test]
    fn test_orphan_function_skips_trait_impls() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("app.rs", "app.rs"));
        let f = Node::function("app.rs::fmt", "app.rs", 10, 20);
        g.add_node(f);
        // No callers, but "fmt" is a trait impl

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::OrphanFunction && s.node_a == "app.rs::fmt"),
            "Trait impl methods should not be flagged as orphan"
        );
    }

    #[test]
    fn test_orphan_function_skips_qualified_methods() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("db.rs", "db.rs"));
        let f = Node::function("db.rs::Database::open", "db.rs", 10, 30);
        g.add_node(f);
        // No callers, but it's a struct method likely called via dispatch

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::OrphanFunction
                    && s.node_a == "db.rs::Database::open"),
            "Qualified struct methods should not be flagged as orphan"
        );
    }

    #[test]
    fn test_orphan_module_skips_config_files() {
        use ising_core::graph::ChangeMetrics;
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("vite.config.ts", "vite.config.ts"));
        g.change_metrics.insert(
            "vite.config.ts".to_string(),
            ChangeMetrics {
                change_freq: 3,
                ..Default::default()
            },
        );

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::OrphanModule && s.node_a == "vite.config.ts"),
            "Config files should not be flagged as orphan modules"
        );
    }

    #[test]
    fn test_orphan_module_skips_scripts() {
        use ising_core::graph::ChangeMetrics;
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("scripts/deploy.ts", "scripts/deploy.ts"));
        g.change_metrics.insert(
            "scripts/deploy.ts".to_string(),
            ChangeMetrics {
                change_freq: 5,
                ..Default::default()
            },
        );

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::OrphanModule
                    && s.node_a == "scripts/deploy.ts"),
            "Script files should not be flagged as orphan modules"
        );
    }

    #[test]
    fn test_no_god_module_for_generated_code() {
        let mut g = UnifiedGraph::new();
        // A generated protobuf file with god-module-level metrics
        let mut pb = Node::module("grpc/model.pb.go", "grpc/model.pb.go");
        pb.complexity = Some(200);
        pb.loc = Some(2000);
        g.add_node(pb);

        for i in 0..25 {
            let dep = format!("dep{}.go", i);
            g.add_node(Node::module(dep.clone(), dep.clone()));
            g.add_edge("grpc/model.pb.go", &dep, EdgeType::Imports, 1.0)
                .unwrap();
        }

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::GodModule),
            "Generated .pb.go files should not trigger GodModule"
        );
    }

    #[test]
    fn test_ghost_coupling_suppressed_go_intra_package() {
        // Two .go files in the same directory (same Go package) co-change
        // without structural edges. This is normal Go packaging — not a
        // hidden dependency — so ghost coupling should be suppressed.
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module(
            "pkg/server/handler.go",
            "pkg/server/handler.go",
        ));
        g.add_node(Node::module("pkg/server/routes.go", "pkg/server/routes.go"));
        g.add_edge(
            "pkg/server/handler.go",
            "pkg/server/routes.go",
            EdgeType::CoChanges,
            0.8,
        )
        .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::GhostCoupling),
            "Go files in the same package should not trigger GhostCoupling"
        );
    }

    #[test]
    fn test_ghost_coupling_not_suppressed_go_cross_package() {
        // Two .go files in different directories should still trigger
        // ghost coupling if they co-change without structural edges.
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module(
            "pkg/server/handler.go",
            "pkg/server/handler.go",
        ));
        g.add_node(Node::module("pkg/client/client.go", "pkg/client/client.go"));
        g.add_edge(
            "pkg/server/handler.go",
            "pkg/client/client.go",
            EdgeType::CoChanges,
            0.8,
        )
        .unwrap();

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::GhostCoupling),
            "Go files in different packages should still trigger GhostCoupling"
        );
    }

    #[test]
    fn test_systemic_complexity_detected() {
        // Create a codebase with 60 modules all having elevated complexity.
        // Median complexity ≥ 15 should trigger SystemicComplexity.
        let mut g = UnifiedGraph::new();
        for i in 0..60 {
            let id = format!("src/mod_{i}.py");
            let mut node = Node::module(id.clone(), id.clone());
            node.complexity = Some(20); // above median threshold of 15
            node.loc = Some(200);
            g.add_node(node);
        }

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::SystemicComplexity),
            "Elevated median complexity across 60+ modules should trigger SystemicComplexity"
        );
    }

    #[test]
    fn test_systemic_complexity_not_detected_low_complexity() {
        // 60 modules with low complexity should NOT trigger SystemicComplexity.
        let mut g = UnifiedGraph::new();
        for i in 0..60 {
            let id = format!("src/mod_{i}.py");
            let mut node = Node::module(id.clone(), id.clone());
            node.complexity = Some(5); // below median threshold of 15
            node.loc = Some(50);
            g.add_node(node);
        }

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::SystemicComplexity),
            "Low median complexity should not trigger SystemicComplexity"
        );
    }

    #[test]
    fn test_systemic_complexity_not_detected_too_few_modules() {
        // Fewer than 50 modules should not trigger SystemicComplexity,
        // even if complexity is high.
        let mut g = UnifiedGraph::new();
        for i in 0..30 {
            let id = format!("src/mod_{i}.py");
            let mut node = Node::module(id.clone(), id.clone());
            node.complexity = Some(25);
            node.loc = Some(300);
            g.add_node(node);
        }

        let signals = detect_signals(&g, &default_config(), None);
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::SystemicComplexity),
            "Fewer than 50 modules should not trigger SystemicComplexity"
        );
    }
}
