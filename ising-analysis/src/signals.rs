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
        if !has_structural && *coupling > thresholds.ghost_coupling_threshold {
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

    // Over-Engineering: structural edges with no temporal activity
    let import_edges = graph.edges_of_type(&EdgeType::Imports);
    for (a, b, _) in &import_edges {
        let coupling = graph
            .edge_weight(a, b, &EdgeType::CoChanges)
            .unwrap_or(0.0);
        let fault_prop = graph
            .edge_weight(a, b, &EdgeType::FaultPropagates)
            .unwrap_or(0.0);
        if coupling < thresholds.over_engineering_coupling && fault_prop == 0.0 {
            signals.push(Signal::new(
                SignalType::OverEngineering,
                a,
                Some(b),
                0.3,
                format!(
                    "Structural dependency exists but <{:.0}% co-change and zero fault propagation. Dependency may be unnecessary.",
                    thresholds.over_engineering_coupling * 100.0
                ),
            ));
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
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        // No structural edge, but high co-change
        g.add_edge("a", "b", EdgeType::CoChanges, 0.8).unwrap();

        let signals = detect_signals(&g, &default_config());
        assert!(signals
            .iter()
            .any(|s| s.signal_type == SignalType::GhostCoupling));
    }

    #[test]
    fn test_no_ghost_coupling_when_structural_edge_exists() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("a", "b", EdgeType::CoChanges, 0.8).unwrap();

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
    fn test_over_engineering_detected() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        // No co-change edge → coupling is 0.0 (< 0.05 threshold)

        let signals = detect_signals(&g, &default_config());
        assert!(signals
            .iter()
            .any(|s| s.signal_type == SignalType::OverEngineering));
    }

    #[test]
    fn test_signals_sorted_by_severity() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_node(Node::module("c", "c.py"));
        g.add_edge("a", "b", EdgeType::CoChanges, 0.6).unwrap();
        g.add_edge("a", "c", EdgeType::CoChanges, 0.9).unwrap();

        let signals = detect_signals(&g, &default_config());
        for w in signals.windows(2) {
            assert!(w[0].severity >= w[1].severity);
        }
    }
}
