//! Risk computation for code modules.
//!
//! Computes change load, capacity, propagated risk, and safety factors.
//! Uses influence propagation along both co-change and structural edges.

use ising_core::config::Config;
use ising_core::fea::{
    LoadCase, LoadPoint, NodeRisk, NodeRiskDelta, RiskDelta, RiskField, SafetyZone,
};
use ising_core::graph::{EdgeType, NodeType, UnifiedGraph};
use ising_core::metrics::{compute_node_metrics, normalize};
use std::collections::HashMap;

/// Maximum safety factor value (clamp to avoid infinity).
const MAX_SAFETY_FACTOR: f64 = 10.0;

/// Small epsilon to avoid division by zero.
const DIV_EPSILON: f64 = 1e-10;

/// Minimum capacity floor.
const MIN_CAPACITY: f64 = 0.05;

/// Collected max values across the graph for normalization.
struct GraphMaxes {
    complexity: f64,
    cbo: f64,
    loc: f64,
    change_pressure: f64,
    coupling: f64,
}

/// Collect normalization maxes from all Module-type nodes.
fn collect_maxes(graph: &UnifiedGraph) -> GraphMaxes {
    let mut max_complexity: f64 = 0.0;
    let mut max_cbo: f64 = 0.0;
    let mut max_loc: f64 = 0.0;
    let mut max_change_pressure: f64 = 0.0;
    let mut max_coupling: f64 = 0.0;

    for node_id in graph.node_ids() {
        let node = match graph.get_node(node_id) {
            Some(n) if n.node_type == NodeType::Module => n,
            _ => continue,
        };

        max_complexity = max_complexity.max(node.complexity.unwrap_or(0) as f64);
        max_loc = max_loc.max(node.loc.unwrap_or(0) as f64);

        let metrics = compute_node_metrics(graph, node_id);
        max_cbo = max_cbo.max(metrics.cbo as f64);
        max_coupling = max_coupling.max((metrics.fan_in + metrics.fan_out) as f64);

        if let Some(cm) = graph.change_metrics.get(node_id) {
            max_change_pressure = max_change_pressure.max(cm.change_freq as f64 * cm.churn_rate);
        }
    }

    GraphMaxes {
        complexity: max_complexity,
        cbo: max_cbo,
        loc: max_loc,
        change_pressure: max_change_pressure,
        coupling: max_coupling,
    }
}

/// Compute change load for a node: how much change pressure it faces [0, 1+].
fn compute_change_load(
    graph: &UnifiedGraph,
    node_id: &str,
    maxes: &GraphMaxes,
    pressure_multiplier: f64,
) -> f64 {
    let cm = match graph.change_metrics.get(node_id) {
        Some(cm) => cm,
        None => return 0.0,
    };
    let raw = cm.change_freq as f64 * cm.churn_rate;
    normalize(raw, maxes.change_pressure) * pressure_multiplier
}

/// Compute structural weight for a node [0, 1].
fn compute_structural_weight(graph: &UnifiedGraph, node_id: &str, maxes: &GraphMaxes) -> f64 {
    let node = match graph.get_node(node_id) {
        Some(n) => n,
        None => return 0.0,
    };
    let metrics = compute_node_metrics(graph, node_id);
    let coupling = (metrics.fan_in + metrics.fan_out) as f64;

    (normalize(node.loc.unwrap_or(0) as f64, maxes.loc)
        + normalize(node.complexity.unwrap_or(0) as f64, maxes.complexity)
        + normalize(coupling, maxes.coupling))
        / 3.0
}

/// Compute capacity for a node: how resilient it is [MIN_CAPACITY, 1.0].
///
/// High capacity = low complexity burden, low instability, low coupling.
/// A well-factored, stable, loosely-coupled module can absorb more change.
fn compute_capacity(graph: &UnifiedGraph, node_id: &str, maxes: &GraphMaxes) -> f64 {
    let node = match graph.get_node(node_id) {
        Some(n) => n,
        None => return 1.0,
    };
    let metrics = compute_node_metrics(graph, node_id);

    let complexity_burden = normalize(node.complexity.unwrap_or(0) as f64, maxes.complexity);
    let instability = if metrics.fan_in + metrics.fan_out > 0 {
        metrics.fan_out as f64 / (metrics.fan_in + metrics.fan_out) as f64
    } else {
        0.0
    };
    let coupling_burden = normalize(metrics.cbo as f64, maxes.cbo);

    // Capacity is the inverse of burden: low complexity + stable + low coupling = high capacity
    let burden = complexity_burden * 0.4 + instability * 0.3 + coupling_burden * 0.3;
    (1.0 - burden).max(MIN_CAPACITY)
}

/// Build adjacency list from both CoChanges and structural edges.
fn build_adjacency<'a>(
    graph: &'a UnifiedGraph,
    config: &Config,
) -> HashMap<&'a str, Vec<(&'a str, f64)>> {
    let mut neighbors: HashMap<&str, Vec<(&str, f64)>> = HashMap::new();

    // Co-change edges (bidirectional, higher damping)
    let co_change_edges = graph.edges_of_type(&EdgeType::CoChanges);
    for &(src, tgt, weight) in &co_change_edges {
        let w = weight * config.fea.cochange_damping;
        neighbors.entry(src).or_default().push((tgt, w));
        neighbors.entry(tgt).or_default().push((src, w));
    }

    // Structural import edges (bidirectional for risk propagation, lower damping)
    let import_edges = graph.edges_of_type(&EdgeType::Imports);
    for &(src, tgt, weight) in &import_edges {
        let w = weight * config.fea.structural_damping;
        neighbors.entry(src).or_default().push((tgt, w));
        neighbors.entry(tgt).or_default().push((src, w));
    }

    neighbors
}

/// Run risk propagation on the graph.
///
/// Uses a Jacobi-style iteration where propagated risk is separate from local load.
/// Each iteration: propagated[i] = sum(propagated[j] * normalized_weight) for neighbors j.
/// Total risk = local_load + propagated.
///
/// Weights per node are normalized so they sum to at most 1.0, guaranteeing convergence.
///
/// Returns (total_risk_per_node, iteration_count, converged).
fn propagate_risk(
    graph: &UnifiedGraph,
    local_loads: &HashMap<String, f64>,
    config: &Config,
) -> (HashMap<String, f64>, usize, bool) {
    let epsilon = config.fea.epsilon;
    let max_iter = config.fea.max_iterations;
    let raw_neighbors = build_adjacency(graph, config);

    // Normalize weights per node so incoming influence sums to at most MAX_SPECTRAL_RADIUS.
    // Keeping the spectral radius strictly < 1 ensures the Jacobi iteration contracts.
    const MAX_SPECTRAL_RADIUS: f64 = 0.95;
    let neighbors: HashMap<&str, Vec<(&str, f64)>> = raw_neighbors
        .into_iter()
        .map(|(node, nbrs)| {
            let total_weight: f64 = nbrs.iter().map(|&(_, w)| w).sum();
            if total_weight > MAX_SPECTRAL_RADIUS {
                let scale = MAX_SPECTRAL_RADIUS / total_weight;
                let normalized: Vec<(&str, f64)> =
                    nbrs.into_iter().map(|(n, w)| (n, w * scale)).collect();
                (node, normalized)
            } else {
                (node, nbrs)
            }
        })
        .collect();

    // Track the propagated component separately from local load.
    // propagated[i] starts at local_load[i] and converges to local_load + neighbor influence.
    let mut propagated: HashMap<String, f64> = local_loads.clone();
    let mut converged = false;
    let mut iterations = 0;

    for iter in 0..max_iter {
        iterations = iter + 1;
        let mut max_delta: f64 = 0.0;
        let mut next = HashMap::new();

        for (node_id, &local_load) in local_loads {
            let neighbor_contribution: f64 = neighbors
                .get(node_id.as_str())
                .map(|nbrs| {
                    nbrs.iter()
                        .map(|&(nbr, weight)| propagated.get(nbr).copied().unwrap_or(0.0) * weight)
                        .sum()
                })
                .unwrap_or(0.0);

            let new_val = local_load + neighbor_contribution;
            let old_val = propagated.get(node_id).copied().unwrap_or(0.0);
            max_delta = max_delta.max((new_val - old_val).abs());
            next.insert(node_id.clone(), new_val);
        }

        propagated = next;

        if max_delta < epsilon {
            converged = true;
            break;
        }
    }

    (propagated, iterations, converged)
}

/// Compute the full risk field for the graph.
pub fn compute_risk_field(graph: &UnifiedGraph, config: &Config) -> RiskField {
    compute_risk_field_with_loads(graph, config, &HashMap::new())
}

/// Compute risk field with optional per-node pressure multipliers.
fn compute_risk_field_with_loads(
    graph: &UnifiedGraph,
    config: &Config,
    pressure_multipliers: &HashMap<String, f64>,
) -> RiskField {
    let maxes = collect_maxes(graph);

    // Collect module node IDs
    let module_ids: Vec<String> = graph
        .node_ids()
        .filter(|id| {
            graph
                .get_node(id)
                .is_some_and(|n| n.node_type == NodeType::Module)
        })
        .map(|s| s.to_string())
        .collect();

    // Compute per-node values
    let mut capacities: HashMap<String, f64> = HashMap::new();
    let mut structural_weights: HashMap<String, f64> = HashMap::new();
    let mut local_loads: HashMap<String, f64> = HashMap::new();

    for node_id in &module_ids {
        let multiplier = pressure_multipliers.get(node_id).copied().unwrap_or(1.0);
        let change_load = compute_change_load(graph, node_id, &maxes, multiplier);
        let capacity = compute_capacity(graph, node_id, &maxes);
        let weight = compute_structural_weight(graph, node_id, &maxes);

        local_loads.insert(node_id.clone(), change_load);
        capacities.insert(node_id.clone(), capacity);
        structural_weights.insert(node_id.clone(), weight);
    }

    // Propagate risk through coupling graph
    let (propagated, iterations, converged) = propagate_risk(graph, &local_loads, config);

    // Build final NodeRisk results
    let mut nodes: Vec<NodeRisk> = Vec::with_capacity(module_ids.len());
    for node_id in &module_ids {
        let change_load = local_loads.get(node_id).copied().unwrap_or(0.0);
        let capacity = capacities.get(node_id).copied().unwrap_or(1.0);
        let structural_weight = structural_weights.get(node_id).copied().unwrap_or(0.0);
        let raw_total = propagated.get(node_id).copied().unwrap_or(change_load);
        let raw_propagated = raw_total - change_load;

        // GAP-3: Attenuate propagated risk for modules with zero/near-zero change load.
        // Re-export modules (e.g. __init__.py) absorb risk purely from neighbors,
        // which over-amplifies their criticality. Scale down propagated risk by
        // structural_weight so lightweight pass-through modules aren't over-flagged.
        let propagated_risk = if change_load < 0.01 {
            raw_propagated * structural_weight.max(0.1)
        } else {
            raw_propagated
        };
        let total_risk = change_load + propagated_risk;

        let safety_factor = (capacity / total_risk.max(DIV_EPSILON)).min(MAX_SAFETY_FACTOR);
        let zone = SafetyZone::from_factor(safety_factor);

        let file_path = graph
            .get_node(node_id)
            .map(|n| n.file_path.clone())
            .unwrap_or_default();

        nodes.push(NodeRisk {
            node_id: node_id.clone(),
            file_path,
            change_load,
            structural_weight,
            propagated_risk,
            risk_score: total_risk,
            capacity,
            safety_factor,
            zone,
        });
    }

    // Sort by safety factor ascending (most critical first)
    nodes.sort_by(|a, b| {
        a.safety_factor
            .partial_cmp(&b.safety_factor)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    RiskField {
        nodes,
        iterations,
        converged,
    }
}

/// Simulate a load case: apply pressure multipliers and compute resulting risk.
pub fn simulate_load_case(
    graph: &UnifiedGraph,
    config: &Config,
    load_case: &LoadCase,
) -> RiskField {
    let multipliers: HashMap<String, f64> = load_case
        .loads
        .iter()
        .map(|lp| (lp.node_id.clone(), lp.pressure))
        .collect();
    compute_risk_field_with_loads(graph, config, &multipliers)
}

/// Compare two risk fields to produce per-node deltas.
pub fn compare_risk_fields(before: &RiskField, after: &RiskField) -> RiskDelta {
    let before_map: HashMap<&str, &NodeRisk> = before
        .nodes
        .iter()
        .map(|n| (n.node_id.as_str(), n))
        .collect();

    let mut deltas: Vec<NodeRiskDelta> = Vec::new();

    for after_node in &after.nodes {
        if let Some(before_node) = before_map.get(after_node.node_id.as_str()) {
            deltas.push(NodeRiskDelta {
                node_id: after_node.node_id.clone(),
                file_path: after_node.file_path.clone(),
                risk_before: before_node.risk_score,
                risk_after: after_node.risk_score,
                safety_factor_before: before_node.safety_factor,
                safety_factor_after: after_node.safety_factor,
                zone_before: before_node.zone,
                zone_after: after_node.zone,
            });
        }
    }

    // Sort by largest safety factor decrease (most impacted first)
    deltas.sort_by(|a, b| {
        let delta_a = a.safety_factor_before - a.safety_factor_after;
        let delta_b = b.safety_factor_before - b.safety_factor_after;
        delta_b
            .partial_cmp(&delta_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    RiskDelta { deltas }
}

/// Generate a load case for a single-file change scenario.
pub fn single_file_change(graph: &UnifiedGraph, file_path: &str) -> LoadCase {
    let mut loads = vec![LoadPoint {
        node_id: file_path.to_string(),
        pressure: 2.0,
    }];

    // Add co-change neighbors at reduced pressure
    let co_changes = graph.edges_of_type(&EdgeType::CoChanges);
    for &(src, tgt, _weight) in &co_changes {
        if src == file_path {
            loads.push(LoadPoint {
                node_id: tgt.to_string(),
                pressure: 1.5,
            });
        } else if tgt == file_path {
            loads.push(LoadPoint {
                node_id: src.to_string(),
                pressure: 1.5,
            });
        }
    }

    LoadCase {
        name: format!("single_file_change:{file_path}"),
        loads,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ising_core::graph::{ChangeMetrics, Node};

    fn make_test_graph() -> UnifiedGraph {
        let mut g = UnifiedGraph::new();

        // Module A: high complexity, high coupling, high churn
        let mut a = Node::module("a.py", "a.py");
        a.complexity = Some(100);
        a.loc = Some(500);
        g.add_node(a);

        // Module B: medium complexity
        let mut b = Node::module("b.py", "b.py");
        b.complexity = Some(50);
        b.loc = Some(200);
        g.add_node(b);

        // Module C: low complexity, no change data
        let mut c = Node::module("c.py", "c.py");
        c.complexity = Some(10);
        c.loc = Some(50);
        g.add_node(c);

        // Structural edges: A imports B, B imports C
        g.add_edge("a.py", "b.py", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("b.py", "c.py", EdgeType::Imports, 1.0).unwrap();

        // Co-change edge between A and B
        g.add_edge("a.py", "b.py", EdgeType::CoChanges, 0.7)
            .unwrap();

        // Change metrics
        g.change_metrics.insert(
            "a.py".to_string(),
            ChangeMetrics {
                change_freq: 30,
                churn_lines: 600,
                churn_rate: 20.0,
                hotspot_score: 0.9,
                sum_coupling: 0.7,
                ..Default::default()
            },
        );
        g.change_metrics.insert(
            "b.py".to_string(),
            ChangeMetrics {
                change_freq: 10,
                churn_lines: 100,
                churn_rate: 10.0,
                hotspot_score: 0.4,
                sum_coupling: 0.7,
                ..Default::default()
            },
        );

        g
    }

    #[test]
    fn test_change_load() {
        let g = make_test_graph();
        let maxes = collect_maxes(&g);

        // a.py: raw = 30*20 = 600, max = 600 → normalized = 1.0
        let load_a = compute_change_load(&g, "a.py", &maxes, 1.0);
        assert!((load_a - 1.0).abs() < 0.01);

        // b.py: raw = 10*10 = 100, max = 600 → normalized ≈ 0.167
        let load_b = compute_change_load(&g, "b.py", &maxes, 1.0);
        assert!((load_b - 100.0 / 600.0).abs() < 0.01);

        // c.py: no change data → 0
        let load_c = compute_change_load(&g, "c.py", &maxes, 1.0);
        assert!((load_c - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_capacity() {
        let g = make_test_graph();
        let maxes = collect_maxes(&g);

        // a.py: max complexity → burden high → capacity low
        let cap_a = compute_capacity(&g, "a.py", &maxes);
        // c.py: low complexity → burden low → capacity high
        let cap_c = compute_capacity(&g, "c.py", &maxes);
        assert!(cap_a < cap_c);
        assert!(cap_a >= MIN_CAPACITY);
        assert!(cap_c <= 1.0);
    }

    #[test]
    fn test_risk_field_ordering() {
        let g = make_test_graph();
        let config = Config::default();
        let field = compute_risk_field(&g, &config);

        // a.py should have lowest SF (highest risk): most change + most complex
        assert_eq!(field.nodes[0].node_id, "a.py");

        // Sorted by SF ascending
        for pair in field.nodes.windows(2) {
            assert!(pair[0].safety_factor <= pair[1].safety_factor);
        }
    }

    #[test]
    fn test_no_change_module_safe() {
        let g = make_test_graph();
        let config = Config::default();
        let field = compute_risk_field(&g, &config);

        // c.py has no change data → change_load = 0, risk ≈ propagated only
        let c = field.nodes.iter().find(|n| n.node_id == "c.py").unwrap();
        assert_eq!(c.change_load, 0.0);
        // Should be safer than a.py
        let a = field.nodes.iter().find(|n| n.node_id == "a.py").unwrap();
        assert!(c.safety_factor > a.safety_factor);
    }

    #[test]
    fn test_propagation_converges() {
        let g = make_test_graph();
        let config = Config::default();
        let field = compute_risk_field(&g, &config);

        assert!(field.converged);
        assert!(field.iterations > 0);
    }

    #[test]
    fn test_propagation_adds_risk() {
        let g = make_test_graph();
        let config = Config::default();
        let field = compute_risk_field(&g, &config);

        // b.py should have propagated risk from a.py (via CoChanges + Imports)
        let b = field.nodes.iter().find(|n| n.node_id == "b.py").unwrap();
        assert!(b.propagated_risk > 0.0);

        // c.py should have some propagated risk from b.py (via Imports)
        let c = field.nodes.iter().find(|n| n.node_id == "c.py").unwrap();
        assert!(c.propagated_risk > 0.0);
    }

    #[test]
    fn test_isolated_node() {
        let mut g = UnifiedGraph::new();
        let mut a = Node::module("a.py", "a.py");
        a.complexity = Some(50);
        a.loc = Some(100);
        g.add_node(a);

        g.change_metrics.insert(
            "a.py".to_string(),
            ChangeMetrics {
                change_freq: 10,
                churn_rate: 5.0,
                ..Default::default()
            },
        );

        let config = Config::default();
        let field = compute_risk_field(&g, &config);

        assert!(field.converged);
        assert_eq!(field.nodes.len(), 1);
        // Isolated node: change_load > 0, propagated = 0
        assert!(field.nodes[0].change_load > 0.0);
        assert!((field.nodes[0].propagated_risk - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_simulate_increases_risk() {
        let g = make_test_graph();
        let config = Config::default();

        let baseline = compute_risk_field(&g, &config);
        let load = LoadCase {
            name: "test".to_string(),
            loads: vec![LoadPoint {
                node_id: "a.py".to_string(),
                pressure: 3.0,
            }],
        };
        let loaded = simulate_load_case(&g, &config, &load);

        // a.py risk should increase under load
        let baseline_a = baseline.nodes.iter().find(|n| n.node_id == "a.py").unwrap();
        let loaded_a = loaded.nodes.iter().find(|n| n.node_id == "a.py").unwrap();
        assert!(loaded_a.risk_score > baseline_a.risk_score);
        // Capacity stays the same
        assert!((loaded_a.capacity - baseline_a.capacity).abs() < 0.001);
        // SF decreases
        assert!(loaded_a.safety_factor < baseline_a.safety_factor);
    }

    #[test]
    fn test_compare_risk_fields() {
        let g = make_test_graph();
        let config = Config::default();

        let before = compute_risk_field(&g, &config);
        let load = LoadCase {
            name: "test".to_string(),
            loads: vec![LoadPoint {
                node_id: "a.py".to_string(),
                pressure: 3.0,
            }],
        };
        let after = simulate_load_case(&g, &config, &load);
        let delta = compare_risk_fields(&before, &after);

        assert!(!delta.deltas.is_empty());
        // First delta should have the largest SF decrease
        let first = &delta.deltas[0];
        assert!(first.safety_factor_before >= first.safety_factor_after);
    }

    #[test]
    fn test_single_file_change_generator() {
        let g = make_test_graph();
        let load = single_file_change(&g, "a.py");

        assert_eq!(load.name, "single_file_change:a.py");
        assert!(
            load.loads
                .iter()
                .any(|lp| lp.node_id == "a.py" && lp.pressure == 2.0)
        );
        assert!(
            load.loads
                .iter()
                .any(|lp| lp.node_id == "b.py" && lp.pressure == 1.5)
        );
    }
}
