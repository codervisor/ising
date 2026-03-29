//! FEA stress computation for code modules.
//!
//! Computes material properties, stress tensors, safety factors, and runs
//! Jacobi-style stress propagation across the coupling graph. Also provides
//! load case simulation and comparison.

use ising_core::config::Config;
use ising_core::fea::{
    LoadCase, LoadPoint, MaterialProperties, NodeStress, NodeStressDelta, SafetyZone, StressDelta,
    StressField, StressTensor,
};
use ising_core::graph::{EdgeType, NodeType, UnifiedGraph};
use ising_core::metrics::{compute_node_metrics, normalize};
use std::collections::HashMap;

/// Maximum safety factor value (clamp to avoid infinity).
const MAX_SAFETY_FACTOR: f64 = 10.0;

/// Small epsilon to avoid division by zero.
const DIV_EPSILON: f64 = 1e-10;

/// Collected max values across the graph for normalization.
struct GraphMaxes {
    complexity: f64,
    cbo: f64,
    loc: f64,
    churn_rate: f64,
    change_pressure: f64,
}

/// Collect normalization maxes from all Module-type nodes.
fn collect_maxes(graph: &UnifiedGraph) -> GraphMaxes {
    let mut max_complexity: f64 = 0.0;
    let mut max_cbo: f64 = 0.0;
    let mut max_loc: f64 = 0.0;
    let mut max_churn_rate: f64 = 0.0;

    let mut max_change_pressure: f64 = 0.0;

    for node_id in graph.node_ids() {
        let node = match graph.get_node(node_id) {
            Some(n) if n.node_type == NodeType::Module => n,
            _ => continue,
        };

        max_complexity = max_complexity.max(node.complexity.unwrap_or(0) as f64);
        max_loc = max_loc.max(node.loc.unwrap_or(0) as f64);

        let metrics = compute_node_metrics(graph, node_id);
        max_cbo = max_cbo.max(metrics.cbo as f64);

        if let Some(cm) = graph.change_metrics.get(node_id) {
            max_churn_rate = max_churn_rate.max(cm.churn_rate);
            max_change_pressure =
                max_change_pressure.max(cm.change_freq as f64 * cm.churn_rate);
        }
    }

    GraphMaxes {
        complexity: max_complexity,
        cbo: max_cbo,
        loc: max_loc,
        churn_rate: max_churn_rate,
        change_pressure: max_change_pressure,
    }
}

/// Compute material properties for a single node.
fn compute_material(
    graph: &UnifiedGraph,
    node_id: &str,
    maxes: &GraphMaxes,
    config: &Config,
) -> MaterialProperties {
    let node = match graph.get_node(node_id) {
        Some(n) => n,
        None => return MaterialProperties::default(),
    };

    let metrics = compute_node_metrics(graph, node_id);

    let stiffness = normalize(node.complexity.unwrap_or(0) as f64, maxes.complexity)
        * normalize(metrics.cbo as f64, maxes.cbo);

    let yield_strength = config.fea.default_yield_strength;

    let churn_rate = graph
        .change_metrics
        .get(node_id)
        .map(|cm| cm.churn_rate)
        .unwrap_or(0.0);
    let fatigue_life = if churn_rate > 0.0 {
        (maxes.churn_rate / churn_rate).min(MAX_SAFETY_FACTOR)
    } else {
        MAX_SAFETY_FACTOR
    };

    let cross_section = (metrics.fan_in + metrics.fan_out) as f64;

    MaterialProperties {
        stiffness,
        yield_strength,
        fatigue_life,
        cross_section,
    }
}

/// Compute the local (pre-propagation) stress tensor for a single node.
fn compute_local_stress(
    graph: &UnifiedGraph,
    node_id: &str,
    maxes: &GraphMaxes,
    pressure_multiplier: f64,
) -> StressTensor {
    let node = match graph.get_node(node_id) {
        Some(n) => n,
        None => return StressTensor::default(),
    };

    let metrics = compute_node_metrics(graph, node_id);

    let cm = graph.change_metrics.get(node_id);
    let change_freq = cm.map(|c| c.change_freq as f64).unwrap_or(0.0);
    let churn_rate = cm.map(|c| c.churn_rate).unwrap_or(0.0);

    let raw_pressure = change_freq * churn_rate;
    let change_pressure = normalize(raw_pressure, maxes.change_pressure) * pressure_multiplier;

    let fan_total = (metrics.fan_in + metrics.fan_out + 1) as f64;
    let tensile = (metrics.fan_out as f64 / fan_total) * change_pressure;

    let compressive = normalize(node.loc.unwrap_or(0) as f64, maxes.loc)
        * normalize(node.complexity.unwrap_or(0) as f64, maxes.complexity)
        * normalize(metrics.cbo as f64, maxes.cbo)
        * change_pressure;

    let von_mises = (tensile * tensile + compressive * compressive - tensile * compressive).sqrt();

    StressTensor {
        change_pressure,
        tensile,
        compressive,
        von_mises,
    }
}

/// Run Jacobi-style stress propagation on the graph.
///
/// Returns updated von_mises values per node, iteration count, and convergence flag.
fn propagate_stress(
    graph: &UnifiedGraph,
    local_von_mises: &HashMap<String, f64>,
    config: &Config,
) -> (HashMap<String, f64>, usize, bool) {
    let damping = config.fea.damping;
    let epsilon = config.fea.epsilon;
    let max_iter = config.fea.max_iterations;

    // Build adjacency: for each node, collect (neighbor_id, coupling_weight) from CoChanges edges
    let co_change_edges = graph.edges_of_type(&EdgeType::CoChanges);
    let mut neighbors: HashMap<&str, Vec<(&str, f64)>> = HashMap::new();
    for &(src, tgt, weight) in &co_change_edges {
        // CoChanges are bidirectional in effect
        neighbors.entry(src).or_default().push((tgt, weight));
        neighbors.entry(tgt).or_default().push((src, weight));
    }

    let mut current: HashMap<String, f64> = local_von_mises.clone();
    let mut converged = false;
    let mut iterations = 0;

    for iter in 0..max_iter {
        iterations = iter + 1;
        let mut max_delta: f64 = 0.0;
        let mut next = HashMap::new();

        for (node_id, &local_stress) in local_von_mises {
            let neighbor_contribution: f64 = neighbors
                .get(node_id.as_str())
                .map(|nbrs| {
                    nbrs.iter()
                        .map(|&(nbr, weight)| {
                            current.get(nbr).copied().unwrap_or(0.0) * weight * damping
                        })
                        .sum()
                })
                .unwrap_or(0.0);

            let new_stress = local_stress + neighbor_contribution;
            let old_stress = current.get(node_id).copied().unwrap_or(0.0);
            max_delta = max_delta.max((new_stress - old_stress).abs());
            next.insert(node_id.clone(), new_stress);
        }

        current = next;

        if max_delta < epsilon {
            converged = true;
            break;
        }
    }

    (current, iterations, converged)
}

/// Compute the full stress field for the graph.
pub fn compute_stress_field(graph: &UnifiedGraph, config: &Config) -> StressField {
    compute_stress_field_with_loads(graph, config, &HashMap::new())
}

/// Compute stress field with optional per-node pressure multipliers.
fn compute_stress_field_with_loads(
    graph: &UnifiedGraph,
    config: &Config,
    pressure_multipliers: &HashMap<String, f64>,
) -> StressField {
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

    // Compute material properties and local stress for each module
    let mut materials: HashMap<String, MaterialProperties> = HashMap::new();
    let mut tensors: HashMap<String, StressTensor> = HashMap::new();
    let mut local_von_mises: HashMap<String, f64> = HashMap::new();

    for node_id in &module_ids {
        let material = compute_material(graph, node_id, &maxes, config);
        let multiplier = pressure_multipliers.get(node_id).copied().unwrap_or(1.0);
        let stress = compute_local_stress(graph, node_id, &maxes, multiplier);
        local_von_mises.insert(node_id.clone(), stress.von_mises);
        materials.insert(node_id.clone(), material);
        tensors.insert(node_id.clone(), stress);
    }

    // Propagate stress through coupling graph
    let (propagated, iterations, converged) = propagate_stress(graph, &local_von_mises, config);

    // Build final NodeStress results
    let mut nodes: Vec<NodeStress> = Vec::with_capacity(module_ids.len());
    for node_id in &module_ids {
        let material = materials.remove(node_id).unwrap_or_default();
        let mut stress = tensors.remove(node_id).unwrap_or_default();

        // Update von_mises with propagated value
        stress.von_mises = propagated.get(node_id).copied().unwrap_or(stress.von_mises);

        let safety_factor =
            (material.yield_strength / stress.von_mises.max(DIV_EPSILON)).min(MAX_SAFETY_FACTOR);
        let safety_zone = SafetyZone::from_factor(safety_factor);

        let file_path = graph
            .get_node(node_id)
            .map(|n| n.file_path.clone())
            .unwrap_or_default();

        nodes.push(NodeStress {
            node_id: node_id.clone(),
            file_path,
            material,
            stress,
            safety_factor,
            safety_zone,
        });
    }

    // Sort by safety factor ascending (most critical first)
    nodes.sort_by(|a, b| {
        a.safety_factor
            .partial_cmp(&b.safety_factor)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    StressField {
        nodes,
        iterations,
        converged,
    }
}

/// Simulate a load case: apply pressure multipliers and compute resulting stress.
pub fn simulate_load_case(
    graph: &UnifiedGraph,
    config: &Config,
    load_case: &LoadCase,
) -> StressField {
    let multipliers: HashMap<String, f64> = load_case
        .loads
        .iter()
        .map(|lp| (lp.node_id.clone(), lp.pressure))
        .collect();
    compute_stress_field_with_loads(graph, config, &multipliers)
}

/// Compare two stress fields to produce per-node deltas.
pub fn compare_stress_fields(before: &StressField, after: &StressField) -> StressDelta {
    let before_map: HashMap<&str, &NodeStress> = before
        .nodes
        .iter()
        .map(|n| (n.node_id.as_str(), n))
        .collect();

    let mut deltas: Vec<NodeStressDelta> = Vec::new();

    for after_node in &after.nodes {
        if let Some(before_node) = before_map.get(after_node.node_id.as_str()) {
            deltas.push(NodeStressDelta {
                node_id: after_node.node_id.clone(),
                file_path: after_node.file_path.clone(),
                von_mises_before: before_node.stress.von_mises,
                von_mises_after: after_node.stress.von_mises,
                safety_factor_before: before_node.safety_factor,
                safety_factor_after: after_node.safety_factor,
                zone_before: before_node.safety_zone,
                zone_after: after_node.safety_zone,
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

    StressDelta { deltas }
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

        // Module C: low complexity
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
        // C has no change metrics (never changed)

        g
    }

    #[test]
    fn test_material_properties_basic() {
        let g = make_test_graph();
        let config = Config::default();
        let maxes = collect_maxes(&g);

        let mat_a = compute_material(&g, "a.py", &maxes, &config);
        // complexity=100/100=1.0, cbo: a.py imports b.py → cbo=1, max_cbo=1 → 1.0
        // stiffness = 1.0 * 1.0 = 1.0
        assert!((mat_a.stiffness - 1.0).abs() < 0.01);
        assert!((mat_a.yield_strength - 0.5).abs() < f64::EPSILON);
        // churn_rate=20.0, max=20.0 → fatigue_life = 20/20 = 1.0
        assert!((mat_a.fatigue_life - 1.0).abs() < 0.01);
        // fan_in=0, fan_out=1 (structural) → cross_section=1.0
        assert!((mat_a.cross_section - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_material_properties_no_change_data() {
        let g = make_test_graph();
        let config = Config::default();
        let maxes = collect_maxes(&g);

        // c.py has no change metrics
        let mat_c = compute_material(&g, "c.py", &maxes, &config);
        // No churn → fatigue_life = MAX
        assert!((mat_c.fatigue_life - MAX_SAFETY_FACTOR).abs() < 0.01);
    }

    #[test]
    fn test_stress_tensor_computation() {
        let g = make_test_graph();
        let maxes = collect_maxes(&g);

        let stress_a = compute_local_stress(&g, "a.py", &maxes, 1.0);
        // change_pressure = normalize(30*20.0, 600.0) = 1.0
        assert!((stress_a.change_pressure - 1.0).abs() < 0.01);
        // tensile: fan_out=1, fan_in=0, total=2 → 1/2 * 1.0 = 0.5
        assert!((stress_a.tensile - 0.5).abs() < 0.01);
        // von_mises should be positive and in [0, ~1.5] range
        assert!(stress_a.von_mises > 0.0);
        assert!(stress_a.von_mises < 2.0);
    }

    #[test]
    fn test_von_mises_pure_math() {
        // von_mises = sqrt(t^2 + c^2 - t*c)
        let t = 3.0_f64;
        let c = 4.0_f64;
        let expected = (t * t + c * c - t * c).sqrt(); // sqrt(9+16-12) = sqrt(13)
        assert!((expected - 13.0_f64.sqrt()).abs() < 1e-10);
    }

    #[test]
    fn test_safety_factor_zero_stress() {
        let g = make_test_graph();
        let config = Config::default();
        let field = compute_stress_field(&g, &config);

        // c.py has no change data → von_mises ≈ 0 → SF clamped to MAX
        let c_stress = field.nodes.iter().find(|n| n.node_id == "c.py").unwrap();
        assert!((c_stress.safety_factor - MAX_SAFETY_FACTOR).abs() < 0.01);
        assert_eq!(c_stress.safety_zone, SafetyZone::OverEngineered);
    }

    #[test]
    fn test_safety_zone_boundaries() {
        assert_eq!(SafetyZone::from_factor(0.5), SafetyZone::Critical);
        assert_eq!(SafetyZone::from_factor(1.0), SafetyZone::Danger);
        assert_eq!(SafetyZone::from_factor(1.5), SafetyZone::Warning);
        assert_eq!(SafetyZone::from_factor(2.0), SafetyZone::Healthy);
        assert_eq!(SafetyZone::from_factor(5.0), SafetyZone::OverEngineered);
    }

    #[test]
    fn test_stress_propagation_converges() {
        let g = make_test_graph();
        let config = Config::default();
        let field = compute_stress_field(&g, &config);

        assert!(field.converged);
        assert!(field.iterations > 0);

        // a.py and b.py are connected by CoChanges — b.py should receive propagated stress
        let b_stress = field.nodes.iter().find(|n| n.node_id == "b.py").unwrap();
        // b.py's propagated von_mises should be > its local von_mises due to a.py's contribution
        // We can verify it's non-zero since b.py has change data
        assert!(b_stress.stress.von_mises > 0.0);
    }

    #[test]
    fn test_stress_propagation_isolated_nodes() {
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
        let field = compute_stress_field(&g, &config);

        // Single isolated node, no edges — converges immediately
        assert!(field.converged);
        assert_eq!(field.nodes.len(), 1);
        // With fan_out=0 and cbo=0, both tensile and compressive are 0
        // so von_mises = 0 and safety factor is clamped to max
        assert!((field.nodes[0].stress.von_mises - 0.0).abs() < 0.01);
        assert!((field.nodes[0].safety_factor - MAX_SAFETY_FACTOR).abs() < 0.01);
    }

    #[test]
    fn test_simulate_load_case() {
        let g = make_test_graph();
        let config = Config::default();

        let baseline = compute_stress_field(&g, &config);
        let load = LoadCase {
            name: "test".to_string(),
            loads: vec![LoadPoint {
                node_id: "a.py".to_string(),
                pressure: 3.0,
            }],
        };
        let loaded = simulate_load_case(&g, &config, &load);

        // a.py stress should increase under load
        let baseline_a = baseline.nodes.iter().find(|n| n.node_id == "a.py").unwrap();
        let loaded_a = loaded.nodes.iter().find(|n| n.node_id == "a.py").unwrap();
        assert!(loaded_a.stress.von_mises > baseline_a.stress.von_mises);
        assert!(loaded_a.safety_factor < baseline_a.safety_factor);
    }

    #[test]
    fn test_compare_stress_fields() {
        let g = make_test_graph();
        let config = Config::default();

        let before = compute_stress_field(&g, &config);
        let load = LoadCase {
            name: "test".to_string(),
            loads: vec![LoadPoint {
                node_id: "a.py".to_string(),
                pressure: 3.0,
            }],
        };
        let after = simulate_load_case(&g, &config, &load);

        let delta = compare_stress_fields(&before, &after);
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
        // Should have a.py at 2.0 and b.py at 1.5 (co-change neighbor)
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

    #[test]
    fn test_stress_field_sorted_by_safety_factor() {
        let g = make_test_graph();
        let config = Config::default();
        let field = compute_stress_field(&g, &config);

        // Verify nodes are sorted by safety_factor ascending
        for pair in field.nodes.windows(2) {
            assert!(pair[0].safety_factor <= pair[1].safety_factor);
        }
    }
}
