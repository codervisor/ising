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
    // Iterate over co-change edges (Layer 2)
    let co_change_edges = graph.edges_of_type(&EdgeType::CoChanges);
    for (a, b, coupling) in &co_change_edges {
        let has_structural = graph.has_structural_edge(a, b);
        let fault_prop = graph
            .edge_weight(a, b, &EdgeType::FaultPropagates)
            .unwrap_or(0.0);

        // Ghost Coupling: no structural dep but high temporal coupling
        // Skip test↔source pairs and non-source-file pairs (directories, configs)
        if !has_structural && *coupling > thresholds.ghost_coupling_threshold
            && !is_test_source_pair(a, b)
            && is_source_file(a) && is_source_file(b)
        {
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
        // Deduplicate: multiple imports between same pair (e.g. 5 `from .globals import X`)
        let pair: (String, String) = if a < b {
            (a.to_string(), b.to_string())
        } else {
            (b.to_string(), a.to_string())
        };
        if !seen_import_pairs.insert(pair) {
            continue;
        }

        let coupling_ab = graph
            .edge_weight(a, b, &EdgeType::CoChanges)
            .unwrap_or(0.0);

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
        let b_complexity = graph
            .get_node(b)
            .and_then(|n| n.complexity)
            .unwrap_or(0);

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
                let coupling_ac = graph
                    .edge_weight(a, c, &EdgeType::CoChanges)
                    .unwrap_or(0.0);
                let coupling_bc = graph
                    .edge_weight(b, c, &EdgeType::CoChanges)
                    .unwrap_or(0.0);

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
        .filter_map(|id| graph.change_metrics.get(id.as_str()).map(|m| m.change_freq as f64))
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
    let defect_p_high = percentile(&mut defect_densities, config.percentiles.ticking_bomb_defect);
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
        if freq > 0.0 && freq <= freq_p_low && fan_in >= fan_in_p_high && fan_in_p_high > 0.0 {
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

fn is_reexport_module(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename == "__init__.py" || filename == "index.ts" || filename == "index.js"
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
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.8).unwrap();

        let signals = detect_signals(&g, &default_config());
        assert!(signals
            .iter()
            .any(|s| s.signal_type == SignalType::GhostCoupling));
    }

    #[test]
    fn test_no_ghost_coupling_when_structural_edge_exists() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        g.add_edge("a.py", "b.py", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.8).unwrap();

        let signals = detect_signals(&g, &default_config());
        assert!(!signals
            .iter()
            .any(|s| s.signal_type == SignalType::GhostCoupling));
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
        assert!(signals
            .iter()
            .any(|s| s.signal_type == SignalType::FragileBoundary));
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
        assert!(signals
            .iter()
            .any(|s| s.signal_type == SignalType::OverEngineering));
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
        assert!(!signals
            .iter()
            .any(|s| s.signal_type == SignalType::OverEngineering));
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
        assert!(signals
            .iter()
            .any(|s| s.signal_type == SignalType::OverEngineering
                && s.description.contains("Pass-through")));
    }

    #[test]
    fn test_signals_sorted_by_severity() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a.py", "a.py"));
        g.add_node(Node::module("b.py", "b.py"));
        g.add_node(Node::module("c.py", "c.py"));
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.6).unwrap();
        g.add_edge("a.py", "c.py", EdgeType::CoChanges, 0.9).unwrap();

        let signals = detect_signals(&g, &default_config());
        for w in signals.windows(2) {
            assert!(w[0].severity >= w[1].severity);
        }
    }
}
