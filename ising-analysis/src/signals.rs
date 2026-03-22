//! Cross-layer signal detection.
//!
//! Each signal is a comparison between layers that reveals patterns invisible
//! from any single layer alone.

use ising_core::config::Config;
use ising_core::graph::{EdgeType, UnifiedGraph};
use ising_core::metrics::{compute_node_metrics, percentile};
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
    OverEngineering,
    /// High fan-in, low change freq, low defects — stable foundation.
    StableCore,
    /// High hotspot + high defects + high coupling — most dangerous code.
    TickingBomb,
}

impl SignalType {
    pub fn priority(&self) -> &'static str {
        match self {
            SignalType::FragileBoundary | SignalType::TickingBomb => "critical",
            SignalType::GhostCoupling => "high",
            SignalType::StableCore => "guard",
            SignalType::OverEngineering => "info",
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
pub fn detect_signals(graph: &UnifiedGraph, config: &Config) -> Vec<Signal> {
    let mut signals = Vec::new();

    let thresholds = &config.thresholds;

    // --- Edge-level signals ---

    // Precompute importer index: for each node, the set of nodes that import it.
    // Used for common-parent suppression in ghost coupling detection.
    let import_edges_all = graph.edges_of_type(&EdgeType::Imports);
    let mut importers: std::collections::HashMap<&str, std::collections::HashSet<&str>> =
        std::collections::HashMap::new();
    for (src, tgt, _) in &import_edges_all {
        importers.entry(tgt).or_default().insert(src);
    }

    // Iterate over co-change edges (Layer 2)
    let co_change_edges = graph.edges_of_type(&EdgeType::CoChanges);
    for (a, b, coupling) in &co_change_edges {
        let has_structural = graph.has_structural_edge(a, b);
        let fault_prop = graph
            .edge_weight(a, b, &EdgeType::FaultPropagates)
            .unwrap_or(0.0);

        // Ghost Coupling: no structural dep but high temporal coupling
        // Skip test↔source pairs and non-source-file pairs (directories, configs)
        if !has_structural
            && *coupling > thresholds.ghost_coupling_threshold
            && !is_test_source_pair(a, b)
            && is_source_file(a)
            && is_source_file(b)
        {
            // Common-parent suppression: if both A and B share a common importer,
            // their co-change is explained by the shared parent orchestrating both.
            let empty = std::collections::HashSet::new();
            let importers_a = importers.get(a).unwrap_or(&empty);
            let importers_b = importers.get(b).unwrap_or(&empty);
            let shared_parents: Vec<&&str> = importers_a.intersection(importers_b).collect();

            // Also check for cross-crate siblings: files in different workspace
            // crates co-change due to shared workspace orchestration, but cross-crate
            // imports aren't tracked as structural edges.
            let has_shared_parent = !shared_parents.is_empty() || is_cross_crate_pair(a, b);

            if has_shared_parent {
                // Shared parent exists — suppress or reduce severity
                if *coupling >= 0.9 {
                    // Very high coupling: still emit at reduced severity with explanation
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
                // If coupling < 0.9: skip signal entirely
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

        // Fragile Boundary: structural dep + high co-change + fault propagation
        if has_structural
            && *coupling > thresholds.fragile_boundary_coupling
            && fault_prop > thresholds.fragile_boundary_fault_prop
        {
            signals.push(Signal::new(
                SignalType::FragileBoundary,
                a,
                Some(b),
                coupling * fault_prop * 10.0, // amplify severity
                format!(
                    "Structural dep + {:.0}% co-change + {:.0}% fault propagation. Interface is fragile.",
                    coupling * 100.0,
                    fault_prop * 100.0
                ),
            ));
        }
    }

    // Over-Engineering: detect likely unnecessary abstractions
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
    //
    // We skip re-export modules (__init__.py, index.ts) which naturally have
    // many low-activity imports, and deduplicate multiple imports between the
    // same pair.
    let import_edges = graph.edges_of_type(&EdgeType::Imports);

    // Precompute structural fan-in (import edges incoming) per node
    let mut fan_in_map: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (_, target, _) in &import_edges {
        *fan_in_map.entry(target).or_default() += 1;
    }

    // Build adjacency list for import edges (for pass-through detection)
    let mut import_targets: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for (src, tgt, _) in &import_edges {
        import_targets.entry(src).or_default().push(tgt);
    }

    let mut seen_import_pairs = std::collections::HashSet::new();
    for (a, b, _) in &import_edges {
        // Skip re-export modules — these are package entry points with many low-activity imports
        if is_reexport_module(a) || is_reexport_module(b) {
            continue;
        }
        // Skip documentation examples — they naturally import core code with fan-in=1
        if is_docs_example(a) || is_docs_example(b) {
            continue;
        }
        // Deduplicate: multiple imports between same pair (e.g. 5 `from .globals import X`)
        let pair: (String, String) = if a < b {
            (a.to_string(), b.to_string())
        } else {
            (b.to_string(), a.to_string())
        };
        if !seen_import_pairs.insert(pair) {
            continue;
        }

        let coupling_ab = graph.edge_weight(a, b, &EdgeType::CoChanges).unwrap_or(0.0);

        // Skip if they do co-change — the dependency is actively used
        if coupling_ab >= thresholds.over_engineering_coupling {
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
        // B has exactly one consumer (A), B is low-complexity, and B rarely changes
        if b_fan_in == 1 && b_complexity <= 5 && b_change_freq <= 1 {
            signals.push(Signal::new(
                SignalType::OverEngineering,
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

        // Signal 2: Pass-through module
        // A→B→C where A↔C co-change but A↔B and B↔C don't
        if let Some(b_targets) = import_targets.get(b) {
            for c in b_targets {
                let coupling_ac = graph.edge_weight(a, c, &EdgeType::CoChanges).unwrap_or(0.0);
                let coupling_bc = graph.edge_weight(b, c, &EdgeType::CoChanges).unwrap_or(0.0);

                if coupling_ac > thresholds.ghost_coupling_threshold
                    && coupling_bc < thresholds.over_engineering_coupling
                {
                    signals.push(Signal::new(
                        SignalType::OverEngineering,
                        a,
                        Some(b),
                        coupling_ac * 0.5,
                        format!(
                            "Pass-through: {} imports {} imports {}, but {} and {} co-change at {:.0}% while {} is dormant. Consider removing the indirection.",
                            a, b, c, a, c, coupling_ac * 100.0, b
                        ),
                    ));
                    break; // one signal per A→B edge is enough
                }
            }
        }
    }

    // --- Node-level signals ---
    // Collect metrics for percentile computation
    let node_ids: Vec<String> = graph.node_ids().map(|s| s.to_string()).collect();

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

    let freq_p_low = percentile(&mut change_freqs, config.percentiles.stable_core_freq);
    let fan_in_p_high = percentile(&mut fan_ins, config.percentiles.stable_core_fan_in);
    let hotspot_p_high = percentile(&mut hotspots, config.percentiles.ticking_bomb_hotspot);
    let defect_p_high = percentile(
        &mut defect_densities,
        config.percentiles.ticking_bomb_defect,
    );
    let coupling_p_high = percentile(&mut sum_couplings, config.percentiles.ticking_bomb_coupling);

    for node_id in &node_ids {
        let metrics = compute_node_metrics(graph, node_id);
        let change = graph.change_metrics.get(node_id.as_str());
        let defect = graph.defect_metrics.get(node_id.as_str());

        let freq = change.map(|m| m.change_freq as f64).unwrap_or(0.0);
        let fan_in = metrics.fan_in as f64;
        let hotspot = change.map(|m| m.hotspot_score).unwrap_or(0.0);
        let defect_d = defect.map(|m| m.defect_density).unwrap_or(0.0);
        let sum_coupling = change.map(|m| m.sum_coupling).unwrap_or(0.0);

        // Stable Core: low change freq + high fan-in + low defects
        // Skip test files and documentation examples — not meaningful as "core"
        if freq > 0.0
            && freq <= freq_p_low
            && fan_in >= fan_in_p_high
            && fan_in_p_high > 0.0
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

        // Ticking Bomb: high hotspot + high defects + high coupling
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

    // Sort by severity (highest first)
    signals.sort_by(|a, b| {
        b.severity
            .partial_cmp(&a.severity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    signals
}

/// Check if a pair of paths is a test file ↔ source file pair.
fn is_test_source_pair(a: &str, b: &str) -> bool {
    let a_is_test = is_test_file(a);
    let b_is_test = is_test_file(b);
    a_is_test != b_is_test
}

fn is_test_file(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename.starts_with("test_")
        || filename.starts_with("tests_")
        || filename.ends_with("_test.py")
        || filename.ends_with(".test.ts")
        || filename.ends_with(".test.js")
        || filename.ends_with(".spec.ts")
        || filename.ends_with(".spec.js")
        || path.contains("/tests/")
        || path.contains("/test/")
        || path.starts_with("tests/")
        || path.starts_with("test/")
}

/// Check if a path is a source code file (has a recognized source extension).
/// Filters out directories, config files, docs, lock files, etc.
fn is_source_file(path: &str) -> bool {
    let source_extensions = [
        ".py", ".ts", ".tsx", ".js", ".jsx", ".rs", ".go", ".java", ".rb", ".cpp", ".c", ".h",
        ".cs", ".swift", ".kt", ".scala",
    ];
    source_extensions.iter().any(|ext| path.ends_with(ext))
}

/// Check if a path is a documentation example (e.g., docs_src/, examples/).
/// These files naturally have fan-in=1 and rarely co-change with their imports,
/// but flagging them as over-engineering or stable core is noise.
fn is_docs_example(path: &str) -> bool {
    path.starts_with("docs_src/")
        || path.starts_with("docs/")
        || path.starts_with("examples/")
        || path.starts_with("example/")
        || path.contains("/docs_src/")
        || path.contains("/examples/")
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

fn is_reexport_module(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename == "__init__.py"
        || filename == "index.ts"
        || filename == "index.js"
        || filename == "mod.rs"
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

        let signals = detect_signals(&g, &default_config());
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

        let signals = detect_signals(&g, &default_config());
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

        let signals = detect_signals(&g, &default_config());
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::FragileBoundary)
        );
    }

    #[test]
    fn test_over_engineering_single_consumer_wrapper() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        // b is a trivial single-consumer module: low complexity, never changes
        let mut b_node = Node::module("b", "b.py");
        b_node.complexity = Some(2);
        g.add_node(b_node);
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        // No co-change, b has fan-in=1 and low complexity → over-engineering

        let signals = detect_signals(&g, &default_config());
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::OverEngineering)
        );
    }

    #[test]
    fn test_no_over_engineering_for_stable_dependency() {
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

        let signals = detect_signals(&g, &default_config());
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::OverEngineering)
        );
    }

    #[test]
    fn test_over_engineering_pass_through() {
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

        let signals = detect_signals(&g, &default_config());
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::OverEngineering
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

        let signals = detect_signals(&g, &default_config());
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

        let signals = detect_signals(&g, &default_config());
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

        let signals = detect_signals(&g, &default_config());
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

        let signals = detect_signals(&g, &default_config());
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
    fn test_no_over_engineering_for_mod_rs() {
        // mod.rs barrel files should not trigger over-engineering
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("src/lib.rs", "src/lib.rs"));
        let mut mod_node = Node::module("src/languages/mod.rs", "src/languages/mod.rs");
        mod_node.complexity = Some(2);
        g.add_node(mod_node);
        g.add_edge("src/lib.rs", "src/languages/mod.rs", EdgeType::Imports, 1.0)
            .unwrap();

        let signals = detect_signals(&g, &default_config());
        assert!(
            !signals
                .iter()
                .any(|s| s.signal_type == SignalType::OverEngineering),
            "mod.rs barrel files should not trigger over-engineering signals"
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

        let signals = detect_signals(&g, &default_config());
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

        let signals = detect_signals(&g, &default_config());
        assert!(
            signals
                .iter()
                .any(|s| s.signal_type == SignalType::GhostCoupling),
            "Ghost coupling should still fire for same-crate files without a common parent"
        );
    }
}
