//! Node and graph-level metric computation.

use crate::graph::{Edge, EdgeLayer, UnifiedGraph};
use petgraph::Direction;
use petgraph::graph::EdgeReference;
use petgraph::visit::EdgeRef;

/// Computed structural metrics for a node.
#[derive(Debug, Clone, Default)]
pub struct NodeMetrics {
    pub fan_in: usize,
    pub fan_out: usize,
    /// Coupling Between Objects: distinct modules this node depends on.
    pub cbo: usize,
    /// Instability: fan_out / (fan_in + fan_out). Range [0, 1].
    pub instability: f64,
}

/// Computed graph-level metrics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphMetrics {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub structural_edges: usize,
    pub change_edges: usize,
    pub defect_edges: usize,
    pub cycle_count: usize,
}

/// Compute fan-in and fan-out for a node (structural edges only).
pub fn compute_node_metrics(graph: &UnifiedGraph, node_id: &str) -> NodeMetrics {
    let Some(idx) = graph.node_index(node_id) else {
        return NodeMetrics::default();
    };

    let is_structural =
        |e: &EdgeReference<'_, Edge>| e.weight().edge_type.layer() == EdgeLayer::Structural;

    let fan_out = graph
        .graph
        .edges_directed(idx, Direction::Outgoing)
        .filter(is_structural)
        .count();

    let fan_in = graph
        .graph
        .edges_directed(idx, Direction::Incoming)
        .filter(is_structural)
        .count();

    // CBO: count distinct file_paths of structural neighbors
    let mut neighbor_files = std::collections::HashSet::new();
    for e in graph
        .graph
        .edges_directed(idx, Direction::Outgoing)
        .filter(is_structural)
    {
        neighbor_files.insert(graph.graph[e.target()].file_path.clone());
    }

    let total = fan_in + fan_out;
    let instability = if total > 0 {
        fan_out as f64 / total as f64
    } else {
        0.0
    };

    NodeMetrics {
        fan_in,
        fan_out,
        cbo: neighbor_files.len(),
        instability,
    }
}

/// Compute graph-level metrics.
pub fn compute_graph_metrics(graph: &UnifiedGraph) -> GraphMetrics {
    let structural_edges = graph.edges_in_layer(EdgeLayer::Structural).len();
    let change_edges = graph.edges_in_layer(EdgeLayer::Change).len();
    let defect_edges = graph.edges_in_layer(EdgeLayer::Defect).len();

    // Count cycles using Tarjan's SCC (cycles = SCCs with size > 1)
    let sccs = petgraph::algo::tarjan_scc(&graph.graph);
    let cycle_count = sccs
        .iter()
        .filter(|scc: &&Vec<petgraph::graph::NodeIndex>| scc.len() > 1)
        .count();

    GraphMetrics {
        total_nodes: graph.node_count(),
        total_edges: graph.edge_count(),
        structural_edges,
        change_edges,
        defect_edges,
        cycle_count,
    }
}

/// Compute the Nth percentile of a sorted slice of f64 values.
pub fn percentile(values: &mut [f64], p: u32) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = (p as f64 / 100.0) * (values.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        values[lower]
    } else {
        let frac = rank - lower as f64;
        values[lower] * (1.0 - frac) + values[upper] * frac
    }
}

/// Normalize a value to [0, 1] given a max value.
pub fn normalize(value: f64, max: f64) -> f64 {
    if max <= 0.0 {
        0.0
    } else {
        (value / max).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgeType, Node};

    #[test]
    fn test_fan_in_fan_out() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_node(Node::module("c", "c.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("a", "c", EdgeType::Imports, 1.0).unwrap();

        let metrics_a = compute_node_metrics(&g, "a");
        assert_eq!(metrics_a.fan_out, 2);
        assert_eq!(metrics_a.fan_in, 0);
        assert_eq!(metrics_a.cbo, 2);
        assert!((metrics_a.instability - 1.0).abs() < f64::EPSILON);

        let metrics_b = compute_node_metrics(&g, "b");
        assert_eq!(metrics_b.fan_in, 1);
        assert_eq!(metrics_b.fan_out, 0);
        assert!((metrics_b.instability - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_graph_metrics() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("a", "b", EdgeType::CoChanges, 0.5).unwrap();

        let metrics = compute_graph_metrics(&g);
        assert_eq!(metrics.total_nodes, 2);
        assert_eq!(metrics.total_edges, 2);
        assert_eq!(metrics.structural_edges, 1);
        assert_eq!(metrics.change_edges, 1);
    }

    #[test]
    fn test_percentile() {
        let mut vals = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((percentile(&mut vals, 50) - 3.0).abs() < f64::EPSILON);
        assert!((percentile(&mut vals, 0) - 1.0).abs() < f64::EPSILON);
        assert!((percentile(&mut vals, 100) - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cycle_detection() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("b", "a", EdgeType::Imports, 1.0).unwrap();

        let metrics = compute_graph_metrics(&g);
        assert_eq!(metrics.cycle_count, 1);
    }
}
