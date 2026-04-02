//! Node and graph-level metric computation.

use crate::graph::{Edge, EdgeLayer, EdgeType, UnifiedGraph};
use petgraph::Direction;
use petgraph::graph::EdgeReference;
use petgraph::visit::EdgeRef;
use std::collections::HashMap;

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
    /// Fraction of modules that participate in at least one co-change edge [0.0, 1.0].
    pub cochange_coverage: f64,
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

    // Compute co-change coverage: fraction of modules with at least one co-change edge
    let module_count = graph.node_count();
    let cochange_coverage = if module_count > 0 {
        let change_edge_list = graph.edges_in_layer(EdgeLayer::Change);
        let mut connected: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (src, tgt, _) in &change_edge_list {
            connected.insert(src);
            connected.insert(tgt);
        }
        connected.len() as f64 / module_count as f64
    } else {
        0.0
    };

    GraphMetrics {
        total_nodes: graph.node_count(),
        total_edges: graph.edge_count(),
        structural_edges,
        change_edges,
        defect_edges,
        cycle_count,
        cochange_coverage,
    }
}

/// Spectral metrics for the coupling graph.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpectralMetrics {
    /// Maximum eigenvalue of the coupling adjacency matrix.
    /// λ < 1.0 = perturbations decay (stable), λ ≥ 1.0 = perturbations cascade (critical).
    pub lambda_max: f64,
    /// Eigenvector centrality: for each node, its component in the dominant eigenvector.
    /// Higher = more responsible for driving propagation.
    pub eigenvector_centrality: HashMap<String, f64>,
    /// Number of power iteration steps to convergence.
    pub iterations: usize,
    /// Whether power iteration converged within max_iterations.
    pub converged: bool,
}

/// Compute spectral metrics (λ_max and eigenvector centrality) for the coupling graph.
///
/// Uses power iteration on the combined adjacency matrix (structural + co-change edges,
/// weighted by damping factors matching the propagation model). The adjacency is treated
/// as undirected (bidirectional) since risk propagation flows both ways.
///
/// Power iteration works because the adjacency matrix of a coupling graph is non-negative,
/// and by the Perron-Frobenius theorem, the dominant eigenvalue is real and positive with
/// a non-negative eigenvector.
///
/// # Arguments
/// * `graph` - The unified graph
/// * `structural_damping` - Weight factor for Import edges (default: 0.15)
/// * `cochange_damping` - Weight factor for CoChange edges (default: 0.30)
///
/// # Performance
/// O(edges × iterations) — typically 20-50 iterations for convergence.
/// For a 40K-node graph with 500K edges, this is ~10M operations.
pub fn compute_spectral_metrics(
    graph: &UnifiedGraph,
    structural_damping: f64,
    cochange_damping: f64,
) -> SpectralMetrics {
    const MAX_ITER: usize = 200;
    const EPSILON: f64 = 1e-6;

    // Build adjacency list with combined weights (bidirectional).
    // We index by dense integer IDs for performance, mapping back to string IDs at the end.
    let node_ids: Vec<&str> = graph.node_ids().collect();
    let n = node_ids.len();

    if n == 0 {
        return SpectralMetrics {
            lambda_max: 0.0,
            eigenvector_centrality: HashMap::new(),
            iterations: 0,
            converged: true,
        };
    }

    // Map string IDs to dense indices
    let id_to_idx: HashMap<&str, usize> =
        node_ids.iter().enumerate().map(|(i, &s)| (s, i)).collect();

    // Build sparse adjacency: Vec<Vec<(neighbor_idx, weight)>>
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];

    // Add structural (Imports) edges — bidirectional
    let import_edges = graph.edges_of_type(&EdgeType::Imports);
    for &(src, tgt, weight) in &import_edges {
        if let (Some(&si), Some(&ti)) = (id_to_idx.get(src), id_to_idx.get(tgt)) {
            let w = weight * structural_damping;
            adj[si].push((ti, w));
            adj[ti].push((si, w));
        }
    }

    // Add co-change edges — bidirectional
    let cochange_edges = graph.edges_of_type(&EdgeType::CoChanges);
    for &(src, tgt, weight) in &cochange_edges {
        if let (Some(&si), Some(&ti)) = (id_to_idx.get(src), id_to_idx.get(tgt)) {
            let w = weight * cochange_damping;
            adj[si].push((ti, w));
            adj[ti].push((si, w));
        }
    }

    // Power iteration: v_{k+1} = A * v_k / ||A * v_k||
    // λ_max ≈ ||A * v_k|| / ||v_k|| (Rayleigh quotient for dominant eigenvalue)
    //
    // Note: For graphs where λ_max = -λ_min (e.g., bipartite/star graphs), the
    // eigenvector oscillates even after lambda converges. We fix this by tracking
    // the pre-normalization vector w = A*v and using |w| for eigenvector centrality.
    let mut v: Vec<f64> = vec![1.0 / (n as f64).sqrt(); n];
    let mut w_final: Vec<f64> = vec![0.0; n];
    let mut lambda_max = 0.0_f64;
    let mut converged = false;
    let mut iterations = 0;

    for iter in 0..MAX_ITER {
        iterations = iter + 1;

        // Compute w = A * v
        let mut w: Vec<f64> = vec![0.0; n];
        for (i, neighbors) in adj.iter().enumerate() {
            for &(j, weight) in neighbors {
                w[i] += weight * v[j];
            }
        }

        // Compute norm = ||w||
        let norm: f64 = w.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-15 {
            // Graph is disconnected or has no edges — λ = 0
            lambda_max = 0.0;
            converged = true;
            break;
        }

        // λ_max estimate = ||w|| (since ||v|| = 1 after normalization)
        let new_lambda = norm;

        // Keep the raw w for eigenvector extraction
        w_final.clone_from(&w);

        // Normalize: v = w / ||w||
        let inv_norm = 1.0 / norm;
        for i in 0..n {
            v[i] = w[i] * inv_norm;
        }

        // Check convergence
        if (new_lambda - lambda_max).abs() < EPSILON {
            lambda_max = new_lambda;
            converged = true;
            break;
        }
        lambda_max = new_lambda;
    }

    // After lambda convergence, do one final A*v to get a stable eigenvector direction.
    // For graphs with symmetric spectrum (λ = -λ), the vector oscillates but A*v always
    // points toward the Perron-Frobenius eigenvector (all non-negative).
    // Use absolute values to handle any residual sign oscillation.
    {
        let mut w: Vec<f64> = vec![0.0; n];
        for (i, neighbors) in adj.iter().enumerate() {
            for &(j, weight) in neighbors {
                w[i] += weight * v[j];
            }
        }
        w_final = w;
    }

    // Build eigenvector centrality map (normalize to [0, 1])
    // Use |w_final| to handle oscillation in bipartite-like graphs.
    let max_w = w_final
        .iter()
        .copied()
        .map(f64::abs)
        .fold(0.0_f64, f64::max);
    let eigenvector_centrality: HashMap<String, f64> = if max_w > 1e-15 {
        node_ids
            .iter()
            .enumerate()
            .filter(|&(i, _)| w_final[i].abs() > 1e-10) // skip near-zero entries for sparsity
            .map(|(i, &id)| (id.to_string(), w_final[i].abs() / max_w))
            .collect()
    } else {
        HashMap::new()
    };

    SpectralMetrics {
        lambda_max,
        eigenvector_centrality,
        iterations,
        converged,
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

    #[test]
    fn test_spectral_empty_graph() {
        let g = UnifiedGraph::new();
        let sm = compute_spectral_metrics(&g, 0.15, 0.30);
        assert_eq!(sm.lambda_max, 0.0);
        assert!(sm.converged);
    }

    #[test]
    fn test_spectral_isolated_nodes() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        // No edges
        let sm = compute_spectral_metrics(&g, 0.15, 0.30);
        assert_eq!(sm.lambda_max, 0.0);
        assert!(sm.converged);
    }

    #[test]
    fn test_spectral_chain_graph() {
        // A→B→C chain (no cycles). With damping=1.0 and unit weights:
        // Adjacency (undirected): A-B, B-C
        // For a path of 3 nodes, λ_max = sqrt(2) ≈ 1.414
        // With structural_damping=1.0: edges have weight 1.0
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_node(Node::module("c", "c.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("b", "c", EdgeType::Imports, 1.0).unwrap();

        let sm = compute_spectral_metrics(&g, 1.0, 0.0);
        // Path graph P_3 undirected: eigenvalues are {-sqrt(2), 0, sqrt(2)}
        // λ_max = sqrt(2) ≈ 1.414
        assert!(sm.converged);
        assert!(
            (sm.lambda_max - std::f64::consts::SQRT_2).abs() < 0.01,
            "Expected λ≈1.414, got {}",
            sm.lambda_max
        );
    }

    #[test]
    fn test_spectral_complete_graph() {
        // K_4 (4 nodes, all connected). λ_max = 3 (N-1) for unit-weight undirected.
        // With structural_damping=1.0: each edge has weight 1.0.
        let mut g = UnifiedGraph::new();
        let nodes = ["a", "b", "c", "d"];
        for &n in &nodes {
            g.add_node(Node::module(n, &format!("{n}.py")));
        }
        for i in 0..4 {
            for j in (i + 1)..4 {
                g.add_edge(nodes[i], nodes[j], EdgeType::Imports, 1.0)
                    .unwrap();
            }
        }

        let sm = compute_spectral_metrics(&g, 1.0, 0.0);
        // K_4 undirected: λ_max = 3 (since we add bidirectional edges,
        // each undirected edge becomes two directed edges with weight 1.0)
        assert!(sm.converged);
        assert!(
            (sm.lambda_max - 3.0).abs() < 0.1,
            "Expected λ≈3.0 for K_4, got {}",
            sm.lambda_max
        );
    }

    #[test]
    fn test_spectral_damping_scales_lambda() {
        // Same graph, different damping → λ should scale proportionally
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();

        let sm_full = compute_spectral_metrics(&g, 1.0, 0.0);
        let sm_half = compute_spectral_metrics(&g, 0.5, 0.0);

        // For a single undirected edge (2 nodes), λ_max = weight
        // With damping=1.0: λ = 1.0, with damping=0.5: λ = 0.5
        assert!(sm_full.converged && sm_half.converged);
        assert!(
            (sm_full.lambda_max - 1.0).abs() < 0.01,
            "Expected λ=1.0, got {}",
            sm_full.lambda_max
        );
        assert!(
            (sm_half.lambda_max - 0.5).abs() < 0.01,
            "Expected λ=0.5, got {}",
            sm_half.lambda_max
        );
    }

    #[test]
    fn test_spectral_cochange_edges() {
        // Verify co-change edges contribute to λ
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        // Only co-change, no structural
        g.add_edge("a", "b", EdgeType::CoChanges, 2.0).unwrap();

        let sm = compute_spectral_metrics(&g, 0.0, 1.0);
        // Single undirected edge weight 2.0 → λ = 2.0
        assert!(sm.converged);
        assert!(
            (sm.lambda_max - 2.0).abs() < 0.01,
            "Expected λ=2.0, got {}",
            sm.lambda_max
        );
    }

    #[test]
    fn test_spectral_eigenvector_hub_node() {
        // Star graph: A is hub, B/C/D are leaves. A→B, A→C, A→D
        // Hub should have highest eigenvector centrality
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("hub", "hub.py"));
        g.add_node(Node::module("leaf1", "leaf1.py"));
        g.add_node(Node::module("leaf2", "leaf2.py"));
        g.add_node(Node::module("leaf3", "leaf3.py"));
        g.add_edge("hub", "leaf1", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("hub", "leaf2", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("hub", "leaf3", EdgeType::Imports, 1.0).unwrap();

        let sm = compute_spectral_metrics(&g, 1.0, 0.0);
        assert!(sm.converged);
        // Hub should have centrality = 1.0 (normalized max)
        let hub_c = sm.eigenvector_centrality.get("hub").copied().unwrap_or(0.0);
        let leaf_c = sm
            .eigenvector_centrality
            .get("leaf1")
            .copied()
            .unwrap_or(0.0);
        assert!(
            (hub_c - 1.0).abs() < 0.01,
            "Hub should have centrality 1.0, got {}",
            hub_c
        );
        assert!(
            leaf_c < hub_c,
            "Leaf centrality ({leaf_c:.6}) should be less than hub ({hub_c:.6})"
        );
        // Star graph K_{1,3}: λ_max = sqrt(3) ≈ 1.732
        assert!(
            (sm.lambda_max - 3.0_f64.sqrt()).abs() < 0.05,
            "Expected λ≈1.732 for star K_1,3, got {}",
            sm.lambda_max
        );
    }
}
