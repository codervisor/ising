//! Risk computation for code modules.
//!
//! Computes change load, capacity, propagated risk, and safety factors.
//! Uses influence propagation along both co-change and structural edges.

use crate::signals::SignalSummary;
use ising_core::boundary::BoundaryStructure;
use ising_core::config::Config;
use ising_core::fea::{
    BoundaryHealthReport, HealthIndex, LoadCase, LoadPoint, NodeRisk, NodeRiskDelta, RiskDelta,
    RiskField, RiskTier, SafetyZone,
};
use ising_core::graph::{EdgeType, NodeType, UnifiedGraph};
use ising_core::metrics::{compute_node_metrics, compute_spectral_metrics, normalize};
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

/// Collect normalization maxes from nodes of a given type.
fn collect_maxes(graph: &UnifiedGraph) -> GraphMaxes {
    collect_maxes_for_type(graph, NodeType::Module)
}

/// Collect normalization maxes from all nodes of the specified type.
fn collect_maxes_for_type(graph: &UnifiedGraph, node_type: NodeType) -> GraphMaxes {
    let mut max_complexity: f64 = 0.0;
    let mut max_cbo: f64 = 0.0;
    let mut max_loc: f64 = 0.0;
    let mut max_change_pressure: f64 = 0.0;
    let mut max_coupling: f64 = 0.0;

    for node_id in graph.node_ids() {
        let node = match graph.get_node(node_id) {
            Some(n) if n.node_type == node_type => n,
            _ => continue,
        };

        max_complexity = max_complexity.max(node.complexity.unwrap_or(0) as f64);
        max_loc = max_loc.max(node.loc.unwrap_or(0) as f64);

        let metrics = compute_node_metrics(graph, node_id);
        max_cbo = max_cbo.max(metrics.cbo as f64);
        max_coupling = max_coupling.max((metrics.fan_in + metrics.fan_out) as f64);

        if let Some(cm) = graph.change_metrics.get(node_id) {
            // Spec 047: weight defect churn 3× higher than feature churn.
            // Falls back to change_freq * churn_rate when no classification data.
            let pressure = if cm.defect_churn > 0 || cm.feature_churn > 0 {
                cm.defect_churn as f64 * 3.0 + cm.feature_churn as f64
            } else {
                cm.change_freq as f64 * cm.churn_rate
            };
            max_change_pressure = max_change_pressure.max(pressure);
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
    // Spec 047: weight defect churn 3× higher than feature churn.
    // Falls back to change_freq * churn_rate (≈ churn_lines) when no classification data.
    let raw = if cm.defect_churn > 0 || cm.feature_churn > 0 {
        cm.defect_churn as f64 * 3.0 + cm.feature_churn as f64
    } else {
        cm.change_freq as f64 * cm.churn_rate
    };
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
///
/// When boundaries are provided, edges crossing module boundaries are
/// attenuated by `config.fea.boundary_attenuation` (default 0.3).
fn build_adjacency<'a>(
    graph: &'a UnifiedGraph,
    config: &Config,
    boundaries: Option<&BoundaryStructure>,
) -> HashMap<&'a str, Vec<(&'a str, f64)>> {
    let mut neighbors: HashMap<&str, Vec<(&str, f64)>> = HashMap::new();
    // Clamp attenuation to [0, 1] as defense-in-depth (also validated at config load).
    let attenuation = config.fea.boundary_attenuation.clamp(0.0, 1.0);

    // Co-change edges (bidirectional, higher damping)
    let co_change_edges = graph.edges_of_type(&EdgeType::CoChanges);
    for &(src, tgt, weight) in &co_change_edges {
        let boundary_factor = if let Some(bs) = boundaries {
            if bs.crosses_boundary(src, tgt) {
                attenuation
            } else {
                1.0
            }
        } else {
            1.0
        };
        let w = weight * config.fea.cochange_damping * boundary_factor;
        neighbors.entry(src).or_default().push((tgt, w));
        neighbors.entry(tgt).or_default().push((src, w));
    }

    // Structural import edges (bidirectional for risk propagation, lower damping)
    let import_edges = graph.edges_of_type(&EdgeType::Imports);
    for &(src, tgt, weight) in &import_edges {
        let boundary_factor = if let Some(bs) = boundaries {
            if bs.crosses_boundary(src, tgt) {
                attenuation
            } else {
                1.0
            }
        } else {
            1.0
        };
        let w = weight * config.fea.structural_damping * boundary_factor;
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
    boundaries: Option<&BoundaryStructure>,
) -> (HashMap<String, f64>, usize, bool) {
    let epsilon = config.fea.epsilon;
    let max_iter = config.fea.max_iterations;
    let raw_neighbors = build_adjacency(graph, config, boundaries);

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
///
/// If `signal_summary` is provided, it's incorporated into the health index
/// to produce a signal-aware grade. Without it, only change-risk is used.
///
/// If `boundary_health` is provided, the health index uses boundary-aware scoring.
pub fn compute_risk_field(
    graph: &UnifiedGraph,
    config: &Config,
    signal_summary: Option<&SignalSummary>,
    boundaries: Option<&BoundaryStructure>,
    boundary_health: Option<&BoundaryHealthReport>,
) -> RiskField {
    let mut field = compute_risk_field_with_loads(
        graph,
        config,
        &HashMap::new(),
        signal_summary,
        boundaries,
        boundary_health,
    );
    // Add function-level risk entries (separate normalization and tier assignment)
    compute_function_risks(graph, config, &mut field);
    // Apply tail risk cap (Moody's minimum function) on the full risk field
    // including function-level nodes — this catches e.g. TypeScript's createTypeChecker.
    apply_tail_risk_cap(&mut field, graph);
    field
}

/// Compute risk field with optional per-node pressure multipliers.
fn compute_risk_field_with_loads(
    graph: &UnifiedGraph,
    config: &Config,
    pressure_multipliers: &HashMap<String, f64>,
    signal_summary: Option<&SignalSummary>,
    boundaries: Option<&BoundaryStructure>,
    boundary_health: Option<&BoundaryHealthReport>,
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

    // Propagate risk through coupling graph (boundary-aware if boundaries provided)
    let (propagated, iterations, converged) =
        propagate_risk(graph, &local_loads, config, boundaries);

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

        // Direct score: local risk without propagation. This is the basis for
        // auto-calibrated tier classification.
        let direct_score = change_load / capacity.max(DIV_EPSILON);

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
            direct_score,
            risk_tier: RiskTier::Normal, // assigned below
            percentile: 0.0,             // assigned below
        });
    }

    // Auto-calibrate: assign risk tiers based on percentile of direct_score.
    // This is the "auto-exposure" step — thresholds derive from the data, not constants.
    assign_risk_tiers(&mut nodes);

    // Compute aggregate health index.
    let default_summary = SignalSummary::default();
    let summary = signal_summary.unwrap_or(&default_summary);
    let health = Some(compute_health_index(
        &nodes,
        summary,
        graph,
        boundary_health,
    ));

    // Sort by direct_score descending (highest risk first) for the primary ranking.
    // This replaces the old SF-ascending sort which was dominated by propagation.
    nodes.sort_by(|a, b| {
        b.direct_score
            .partial_cmp(&a.direct_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    RiskField {
        nodes,
        iterations,
        converged,
        health,
    }
}

/// Compute function-level risk and append to an existing (module-level) risk field.
///
/// Functions are assessed using the same framework as modules but with their own
/// normalization context (maxes computed from Function nodes only) and using
/// Calls edges for propagation instead of Imports/CoChanges.
pub fn compute_function_risks(graph: &UnifiedGraph, config: &Config, field: &mut RiskField) {
    let func_maxes = collect_maxes_for_type(graph, NodeType::Function);

    // If there are no functions with any data, skip entirely
    if func_maxes.change_pressure == 0.0 && func_maxes.complexity == 0.0 {
        return;
    }

    // Collect function node IDs
    let func_ids: Vec<String> = graph
        .node_ids()
        .filter(|id| {
            graph
                .get_node(id)
                .is_some_and(|n| n.node_type == NodeType::Function)
        })
        .map(|s| s.to_string())
        .collect();

    if func_ids.is_empty() {
        return;
    }

    // Compute per-function values
    let mut capacities: HashMap<String, f64> = HashMap::new();
    let mut local_loads: HashMap<String, f64> = HashMap::new();

    for node_id in &func_ids {
        let change_load = compute_change_load(graph, node_id, &func_maxes, 1.0);
        let capacity = compute_capacity(graph, node_id, &func_maxes);

        local_loads.insert(node_id.clone(), change_load);
        capacities.insert(node_id.clone(), capacity);
    }

    // Propagate risk through Calls edges
    let func_neighbors = build_calls_adjacency(graph, config);
    let (propagated, _, _) = propagate_risk_with_adjacency(&local_loads, &func_neighbors, config);

    // Build NodeRisk entries for functions
    let mut func_nodes: Vec<NodeRisk> = Vec::with_capacity(func_ids.len());
    for node_id in &func_ids {
        let change_load = local_loads.get(node_id).copied().unwrap_or(0.0);
        let capacity = capacities.get(node_id).copied().unwrap_or(1.0);
        let raw_total = propagated
            .get(node_id.as_str())
            .copied()
            .unwrap_or(change_load);
        let propagated_risk = (raw_total - change_load).max(0.0);
        let total_risk = change_load + propagated_risk;

        let safety_factor = (capacity / total_risk.max(DIV_EPSILON)).min(MAX_SAFETY_FACTOR);
        let zone = SafetyZone::from_factor(safety_factor);
        let direct_score = change_load / capacity.max(DIV_EPSILON);

        let file_path = graph
            .get_node(node_id)
            .map(|n| n.file_path.clone())
            .unwrap_or_default();

        let structural_weight = compute_structural_weight(graph, node_id, &func_maxes);

        func_nodes.push(NodeRisk {
            node_id: node_id.clone(),
            file_path,
            change_load,
            structural_weight,
            propagated_risk,
            risk_score: total_risk,
            capacity,
            safety_factor,
            zone,
            direct_score,
            risk_tier: RiskTier::Normal,
            percentile: 0.0,
        });
    }

    // Assign risk tiers among functions only (separate from module tiers)
    assign_risk_tiers(&mut func_nodes);

    field.nodes.extend(func_nodes);
}

/// Build adjacency list from Calls edges (for function-level propagation).
fn build_calls_adjacency<'a>(
    graph: &'a UnifiedGraph,
    config: &Config,
) -> HashMap<&'a str, Vec<(&'a str, f64)>> {
    let mut neighbors: HashMap<&str, Vec<(&str, f64)>> = HashMap::new();

    let calls_edges = graph.edges_of_type(&EdgeType::Calls);
    for &(src, tgt, weight) in &calls_edges {
        // Caller -> callee: structural damping (similar to imports)
        let w = weight * config.fea.structural_damping;
        neighbors.entry(src).or_default().push((tgt, w));
        // Reverse: callee risk propagates back to caller (at lower weight)
        neighbors.entry(tgt).or_default().push((src, w * 0.5));
    }

    neighbors
}

/// Run risk propagation with a pre-built adjacency list.
fn propagate_risk_with_adjacency<'a>(
    local_loads: &HashMap<String, f64>,
    raw_neighbors: &HashMap<&'a str, Vec<(&'a str, f64)>>,
    config: &Config,
) -> (HashMap<String, f64>, usize, bool) {
    let epsilon = config.fea.epsilon;
    let max_iter = config.fea.max_iterations;

    // Normalize weights per node
    const MAX_SPECTRAL_RADIUS: f64 = 0.95;
    let neighbors: HashMap<&str, Vec<(&str, f64)>> = raw_neighbors
        .iter()
        .map(|(&node, nbrs)| {
            let total_weight: f64 = nbrs.iter().map(|&(_, w)| w).sum();
            if total_weight > MAX_SPECTRAL_RADIUS {
                let scale = MAX_SPECTRAL_RADIUS / total_weight;
                let normalized: Vec<(&str, f64)> =
                    nbrs.iter().map(|&(n, w)| (n, w * scale)).collect();
                (node, normalized)
            } else {
                (node, nbrs.clone())
            }
        })
        .collect();

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
    compute_risk_field_with_loads(graph, config, &multipliers, None, None, None)
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

/// Assign auto-calibrated risk tiers based on percentile of direct_score.
///
/// Like auto-exposure in a camera: instead of fixed thresholds, we measure the
/// distribution and set tiers relative to it. Top 1% = Critical, top 5% = High,
/// top 15% = Medium, rest = Normal.
///
/// Only modules with change_load > 0 (i.e., actually changed in the time window)
/// are eligible for Critical/High tiers. Unchanged modules are always Normal.
fn assign_risk_tiers(nodes: &mut [NodeRisk]) {
    if nodes.is_empty() {
        return;
    }

    // Collect direct scores of active modules (those with actual changes)
    let mut active_scores: Vec<(usize, f64)> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.change_load > 0.0)
        .map(|(i, n)| (i, n.direct_score))
        .collect();

    // Sort descending by direct score
    active_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let n = active_scores.len();
    if n == 0 {
        return;
    }

    // Assign tiers based on rank position among active modules
    let critical_cutoff = (n as f64 * 0.01).ceil() as usize; // top 1%
    let high_cutoff = (n as f64 * 0.05).ceil() as usize; // top 5%
    let medium_cutoff = (n as f64 * 0.15).ceil() as usize; // top 15%

    for (rank, &(node_idx, _score)) in active_scores.iter().enumerate() {
        let tier = if rank < critical_cutoff {
            RiskTier::Critical
        } else if rank < high_cutoff {
            RiskTier::High
        } else if rank < medium_cutoff {
            RiskTier::Medium
        } else {
            RiskTier::Normal
        };
        let percentile = 100.0 * (1.0 - rank as f64 / n as f64);
        nodes[node_idx].risk_tier = tier;
        nodes[node_idx].percentile = percentile;
    }
}

/// Compute aggregate health index for the repository.
///
/// Spec 047 formula (fully multiplicative):
///   score = zone_sub_score × coupling_modifier × containment_modifier × signal_factor
///
/// Components:
/// - **zone_sub_score** [0.15, 1.0]: weighted average of safety zone fractions (primary driver)
/// - **coupling_modifier** [0.85, 1.05]: f(λ_max/√N), penalties/bonuses from structural coupling
/// - **containment_modifier** [0.70, 1.05]: boundary health bonus/penalty
/// - **signal_factor** [0.70, 1.0]: 1.0 - signal_penalty, architectural signal discount
///
/// Signal penalty uses an adaptive piecewise curve:
///   x ≤ 5: 0.25 × x/(x+3)  (gentle sigmoid)
///   x > 5: 0.156 + 0.094 × log₂(x/5)  (log growth, cap 0.30)
///
/// Tail risk (Basel II / Moody's-inspired):
/// - Expected Loss per module: EL = direct_score × (1 + fan_in / max_fan_in)
/// - HHI over EL distribution measures concentration risk
/// - If max(EL) > 3.0, score is capped at 0.84 (B ceiling) — Moody's "minimum function"
///
/// Legacy sub-scores (risk, signal, structural) still computed for backward compatibility.
fn compute_health_index(
    nodes: &[NodeRisk],
    signals: &SignalSummary,
    graph: &UnifiedGraph,
    boundary_health: Option<&BoundaryHealthReport>,
) -> HealthIndex {
    let total_modules = nodes.len();
    let active: Vec<&NodeRisk> = nodes.iter().filter(|n| n.change_load > 0.0).collect();
    let active_modules = active.len();
    let total_f = (total_modules as f64).max(1.0);

    // Compute structural spectral radius (unit weights on Import edges).
    let spectral = compute_spectral_metrics(graph);
    let lambda_max = spectral.lambda_max;

    if active_modules == 0 {
        let caveats =
            vec!["No modules have change history; score reflects structure only".to_string()];
        return HealthIndex {
            score: 1.0,
            grade: "A".to_string(),
            active_modules: 0,
            total_modules,
            critical_count: 0,
            high_count: 0,
            risk_concentration: 1.0,
            avg_direct_score: 0.0,
            frac_stable: 0.0,
            frac_healthy: 0.0,
            frac_warning: 0.0,
            frac_danger: 0.0,
            frac_critical: 0.0,
            lambda_max,
            signal_density: signals.total_signals as f64 / total_f,
            god_module_density: signals.god_module_count as f64 / total_f,
            cycle_density: signals.cycle_count as f64 / total_f,
            unstable_dep_density: signals.unstable_dep_count as f64 / total_f,
            zone_sub_score: 1.0,
            coupling_modifier: 1.0,
            signal_penalty: 0.0,
            risk_sub_score: 1.0,
            signal_sub_score: 1.0,
            structural_sub_score: 1.0,
            boundary_health_score: 1.0,
            max_expected_loss: 0.0,
            el_hhi: 0.0,
            tail_risk_capped: false,
            caveats,
        };
    }

    let critical_count = nodes
        .iter()
        .filter(|n| n.risk_tier == RiskTier::Critical)
        .count();
    let high_count = nodes
        .iter()
        .filter(|n| n.risk_tier == RiskTier::High)
        .count();

    // === Zone fractions ===
    // Count active modules in each safety zone. This directly measures what fraction
    // of the codebase is in each structural health state.
    let active_f = active_modules as f64;
    let mut zone_counts = [0usize; 5]; // [stable, healthy, warning, danger, critical]
    for n in &active {
        match n.zone {
            SafetyZone::Stable => zone_counts[0] += 1,
            SafetyZone::Healthy => zone_counts[1] += 1,
            SafetyZone::Warning => zone_counts[2] += 1,
            SafetyZone::Danger => zone_counts[3] += 1,
            SafetyZone::Critical => zone_counts[4] += 1,
        }
    }
    let frac_stable = zone_counts[0] as f64 / active_f;
    let frac_healthy = zone_counts[1] as f64 / active_f;
    let frac_warning = zone_counts[2] as f64 / active_f;
    let frac_danger = zone_counts[3] as f64 / active_f;
    let frac_critical = zone_counts[4] as f64 / active_f;

    // === Zone sub-score ===
    // Weighted average: each zone contributes proportionally to its health.
    // Weights: Stable=1.0, Healthy=0.90, Warning=0.65, Danger=0.35, Critical=0.15
    //
    // Critical gets 0.15 (not 0.0) because SF<1.0 doesn't mean "broken" — it means
    // the module is under change pressure relative to its complexity. For actively
    // maintained frameworks (flask, gin), core modules are always high-churn and thus
    // tend to land in Critical. Giving them some weight prevents small frameworks
    // from being unfairly penalized.
    let raw_zone_score = frac_stable * 1.0
        + frac_healthy * 0.90
        + frac_warning * 0.65
        + frac_danger * 0.35
        + frac_critical * 0.15;

    // Small-sample adjustment: when few modules are active (<50), zone fractions
    // are noisy — one module changing zones shifts the score by 1/N. We blend the
    // raw zone score toward a neutral prior (0.75) proportionally to sample size.
    // At 50+ active modules, no adjustment. At 10, blend 40% toward prior.
    let sample_blend = (active_f / 50.0).min(1.0);
    let zone_sub_score =
        (raw_zone_score * sample_blend + 0.75 * (1.0 - sample_blend)).clamp(0.0, 1.0);

    // === Coupling modifier (λ_max) ===
    // λ_max is the spectral radius of the structural Import graph with unit weights.
    // For real codebases, λ_max is always >>1 because hub modules import many others
    // (a module importing k files creates degree k, giving λ ≥ sqrt(k)).
    //
    // To make λ_max comparable across codebases of different sizes, we normalize:
    //   normalized_lambda = λ_max / sqrt(total_modules)
    // This measures coupling *density*: how connected the graph is relative to its size.
    //   - Complete graph K_n: λ = n-1, normalized = sqrt(n) → grows with size
    //   - Star graph:        λ = sqrt(k), normalized = sqrt(k/n) → small for large n
    //   - Random graph:      λ ≈ avg_degree, normalized = avg_degree/sqrt(n)
    //
    // Typical ranges for normalized_lambda:
    //   < 0.5: loosely coupled
    //   0.5-2.0: moderate coupling (most real codebases)
    //   > 2.0: tightly coupled
    //
    // The modifier applies a gentle penalty/bonus based on normalized coupling:
    //   - normalized < 1.0: slight bonus (up to +5%)
    //   - normalized > 1.0: gentle penalty (up to -10%)
    let normalized_lambda = if total_f > 1.0 {
        lambda_max / total_f.sqrt()
    } else {
        0.0
    };
    // Phase 2 (spec 047): Widened range [0.85, 1.05] gives coupling a 20% total swing,
    // enough to shift a grade. Bonus/penalty coefficients scaled up accordingly.
    let coupling_bonus = if normalized_lambda < 1.0 {
        (1.0 - normalized_lambda) * 0.05
    } else {
        0.0
    };
    let coupling_penalty = if normalized_lambda > 1.0 {
        // Log penalty: log2(norm_λ) * 0.05, capped at 15%.
        (normalized_lambda.log2() * 0.05).min(0.15)
    } else {
        0.0
    };
    let coupling_modifier = (1.0 + coupling_bonus - coupling_penalty).clamp(0.85, 1.05);

    // === Signal penalty ===
    // Signals are architectural problems detected across layers. Instead of a separate
    // sub-score that gates grades, signals act as a penalty on the zone-based score.
    //
    // This ensures that a codebase with perfect zone distribution but many god modules
    // still gets penalized, while a codebase with zero signals doesn't get a free A.
    let sqrt_n = total_f.sqrt();
    let weighted_signal_score = (signals.god_module_count as f64 / sqrt_n) * 3.0
        + (signals.cycle_count as f64 / sqrt_n) * 4.0
        + (signals.ticking_bomb_count as f64 / sqrt_n) * 3.0
        + (signals.fragile_boundary_count as f64 / sqrt_n) * 2.0
        + (signals.shotgun_surgery_count as f64 / sqrt_n) * 1.5
        + (signals.unstable_dep_count as f64 / sqrt_n) * 2.0
        + (signals.ghost_coupling_count as f64 / sqrt_n) * 1.0
        + (signals.systemic_complexity_count as f64) * 2.5;
    // Phase 5 (spec 047): Adaptive piecewise penalty curve.
    // Below x=5: gentle sigmoid (same as before). Above x=5: log growth for better
    // discrimination between signal-heavy repos (e.g., kubernetes vs prometheus).
    // Cap raised from 0.25 to 0.30.
    let signal_penalty = if weighted_signal_score <= 5.0 {
        (0.25 * weighted_signal_score / (weighted_signal_score + 3.0)).clamp(0.0, 0.25)
    } else {
        // 0.156 is the sigmoid value at x=5: 0.25 * 5/(5+3)
        (0.15625 + 0.094 * (weighted_signal_score / 5.0).log2()).clamp(0.0, 0.30)
    };

    // === Boundary health score ===
    // When boundary health is available, incorporate containment into scoring.
    // avg_containment measures how well risk is contained within module boundaries
    // (1.0 = perfect, 0.0 = all leaks). When not computed, default to 1.0 (neutral).
    let boundary_health_score = boundary_health.map(|bh| bh.avg_containment).unwrap_or(1.0);

    // === Expected Loss metrics (Basel II-inspired, for transparency) ===
    // EL = direct_score × (1 + fan_in_normalized) per module.
    // Computed here for the HealthIndex record; the actual tail risk cap is applied
    // post-hoc in apply_tail_risk_cap() after function-level nodes are added.
    let max_fan_in = active
        .iter()
        .map(|n| compute_node_metrics(graph, &n.node_id).fan_in)
        .max()
        .unwrap_or(0) as f64;

    let expected_losses: Vec<f64> = active
        .iter()
        .map(|n| {
            let fan_in = compute_node_metrics(graph, &n.node_id).fan_in as f64;
            let fan_in_norm = if max_fan_in > 0.0 {
                fan_in / max_fan_in
            } else {
                0.0
            };
            n.direct_score * (1.0 + fan_in_norm)
        })
        .collect();

    let total_el: f64 = expected_losses.iter().sum();
    let max_el = expected_losses.iter().copied().fold(0.0_f64, f64::max);
    let el_hhi = if total_el > 0.0 {
        expected_losses
            .iter()
            .map(|el| {
                let share = el / total_el;
                share * share
            })
            .sum::<f64>()
    } else {
        0.0
    };

    // === Final score ===
    // Fully multiplicative formula (spec 047 Phase 1).
    // Tail risk cap is applied separately after function-level nodes are added
    // (see apply_tail_risk_cap).
    let signal_factor = 1.0 - signal_penalty;
    let score = if let Some(bh) = boundary_health {
        let containment_modifier = (0.70 + 0.35 * bh.avg_containment).clamp(0.70, 1.05);
        (zone_sub_score * coupling_modifier * containment_modifier * signal_factor).clamp(0.0, 1.0)
    } else {
        (zone_sub_score * coupling_modifier * signal_factor).clamp(0.0, 1.0)
    };

    let grade = if score >= 0.85 {
        "A"
    } else if score >= 0.70 {
        "B"
    } else if score >= 0.55 {
        "C"
    } else if score >= 0.40 {
        "D"
    } else {
        "F"
    }
    .to_string();

    // === Legacy sub-scores (for backward compatibility) ===
    let total_direct: f64 = active.iter().map(|n| n.direct_score).sum();
    let avg_direct_score = total_direct / active_f;

    let mut scores: Vec<f64> = active.iter().map(|n| n.direct_score).collect();
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_direct_score = if scores.len().is_multiple_of(2) && scores.len() >= 2 {
        (scores[scores.len() / 2 - 1] + scores[scores.len() / 2]) / 2.0
    } else {
        scores[scores.len() / 2]
    };
    let p75_idx = ((scores.len() as f64 * 0.75).floor() as usize).min(scores.len() - 1);
    let p75_direct_score = scores[p75_idx];
    let representative_score = median_direct_score * 0.75 + p75_direct_score * 0.25;

    scores.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let top_10_count = (scores.len() as f64 * 0.10).ceil() as usize;
    let top_10_sum: f64 = scores.iter().take(top_10_count).sum();
    let risk_concentration = if total_direct > 0.0 {
        top_10_sum / total_direct
    } else {
        1.0
    };

    let amplification = if active_modules < 20 {
        let normalized = (active_f - 1.0) / 19.0;
        2.0 + 3.0 * normalized
    } else {
        5.0
    };
    let base_health = 1.0 / (1.0 + representative_score * amplification);
    let concentration_factor = 0.8 + 0.2 * risk_concentration;
    let risk_sub_score = (base_health * concentration_factor).clamp(0.0, 1.0);

    let signal_sub_score = (1.0 / (1.0 + weighted_signal_score * 0.3)).clamp(0.0, 1.0);

    let entanglement_score = (signals.cycle_count + signals.unstable_dep_count) as f64 / sqrt_n;
    let structural_sub_score = (1.0 / (1.0 + entanglement_score * 0.5)).clamp(0.0, 1.0);

    // Signal densities for display.
    let signal_density = signals.total_signals as f64 / total_f;
    let god_module_density = signals.god_module_count as f64 / total_f;
    let cycle_density = signals.cycle_count as f64 / total_f;
    let unstable_dep_density = signals.unstable_dep_count as f64 / total_f;

    // --- Caveats ---
    let mut caveats = Vec::new();
    if active_modules < total_modules / 20 {
        caveats.push(format!(
            "Only {:.1}% of modules have change history; risk scores reflect recent activity only",
            active_modules as f64 / total_modules as f64 * 100.0
        ));
    }
    if signals.total_signals == 0 && total_modules > 50 {
        caveats.push(
            "No architectural signals detected; verify analysis included sufficient git history"
                .to_string(),
        );
    }
    if signals.ticking_bomb_count == 0 && total_modules > 100 {
        caveats.push(
            "No ticking bombs detected; this may indicate missing defect/bug-fix data".to_string(),
        );
    }
    if normalized_lambda > 2.0 {
        caveats.push(format!(
            "High structural coupling: λ/√N={:.2} (λ={:.1}), failures likely cascade across modules",
            normalized_lambda, lambda_max
        ));
    }
    if frac_critical > 0.10 {
        caveats.push(format!(
            "{:.0}% of active modules in critical zone (SF<1.0)",
            frac_critical * 100.0
        ));
    }
    // Note: tail_risk_capped is initially false here; it gets set by
    // apply_tail_risk_cap() which runs after function-level nodes are added.

    HealthIndex {
        score,
        grade,
        active_modules,
        total_modules,
        critical_count,
        high_count,
        risk_concentration,
        avg_direct_score,
        frac_stable,
        frac_healthy,
        frac_warning,
        frac_danger,
        frac_critical,
        lambda_max,
        signal_density,
        god_module_density,
        cycle_density,
        unstable_dep_density,
        zone_sub_score,
        coupling_modifier,
        signal_penalty,
        risk_sub_score,
        signal_sub_score,
        structural_sub_score,
        boundary_health_score,
        max_expected_loss: max_el,
        el_hhi,
        tail_risk_capped: false, // Set by apply_tail_risk_cap() post-hoc
        caveats,
    }
}

/// Apply tail risk cap (Moody's "minimum function") to the health index.
///
/// Computed on the FULL risk field including function-level nodes, so that
/// function-level systemic risks (e.g., TypeScript's `createTypeChecker`) are caught.
///
/// Expected Loss = direct_score × (1 + fan_in / max_fan_in).
/// - Test files are excluded (high churn but zero blast radius).
/// - If max(EL) among non-test nodes exceeds the threshold, score is capped at B (0.84).
fn apply_tail_risk_cap(field: &mut RiskField, graph: &UnifiedGraph) {
    use ising_core::path_utils::is_test_file;

    let health = match &field.health {
        Some(h) => h,
        None => return,
    };

    // Only consider active (changed) nodes that are NOT test files.
    let active_non_test: Vec<&NodeRisk> = field
        .nodes
        .iter()
        .filter(|n| n.change_load > 0.0 && !is_test_file(&n.file_path))
        .collect();

    if active_non_test.is_empty() {
        return;
    }

    let max_fan_in = active_non_test
        .iter()
        .map(|n| compute_node_metrics(graph, &n.node_id).fan_in)
        .max()
        .unwrap_or(0) as f64;

    // Compute EL for each non-test active node.
    let mut max_el = 0.0_f64;
    let mut max_el_node = String::new();
    for n in &active_non_test {
        let fan_in = compute_node_metrics(graph, &n.node_id).fan_in as f64;
        let fan_in_norm = if max_fan_in > 0.0 {
            fan_in / max_fan_in
        } else {
            0.0
        };
        let el = n.direct_score * (1.0 + fan_in_norm);
        if el > max_el {
            max_el = el;
            max_el_node = n.node_id.clone();
        }
    }

    // Threshold calibrated against benchmark: 5.0 catches true systemic risks
    // (TypeScript createTypeChecker EL≈40, svelte compiler EL≈13) while avoiding
    // false positives from normal high-churn production files (typical EL < 3).
    const TAIL_RISK_EL_THRESHOLD: f64 = 5.0;

    if max_el > TAIL_RISK_EL_THRESHOLD && health.score > 0.84 {
        let mut updated = health.clone();
        updated.score = updated.score.min(0.84);
        updated.grade = "B".to_string();
        updated.tail_risk_capped = true;
        updated.max_expected_loss = max_el;
        updated.caveats.push(format!(
            "Tail risk cap: {} has Expected Loss {:.1} (>{:.0} threshold), grade capped at B",
            max_el_node.rsplit('/').next().unwrap_or(&max_el_node),
            max_el,
            TAIL_RISK_EL_THRESHOLD
        ));
        field.health = Some(updated);
    } else if let Some(h) = &mut field.health {
        h.max_expected_loss = max_el;
    }
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
        let field = compute_risk_field(&g, &config, None, None, None);

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
        let field = compute_risk_field(&g, &config, None, None, None);

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
        let field = compute_risk_field(&g, &config, None, None, None);

        assert!(field.converged);
        assert!(field.iterations > 0);
    }

    #[test]
    fn test_propagation_adds_risk() {
        let g = make_test_graph();
        let config = Config::default();
        let field = compute_risk_field(&g, &config, None, None, None);

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
        let field = compute_risk_field(&g, &config, None, None, None);

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

        let baseline = compute_risk_field(&g, &config, None, None, None);
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

        let before = compute_risk_field(&g, &config, None, None, None);
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

    #[test]
    fn test_function_level_risk_computation() {
        let mut g = UnifiedGraph::new();

        // Module with functions
        let mut m = Node::module("app.py", "app.py");
        m.complexity = Some(50);
        m.loc = Some(200);
        g.add_node(m);

        let mut f1 = Node::function("app.py::hot_func", "app.py", 1, 50);
        f1.complexity = Some(20);
        f1.loc = Some(50);
        g.add_node(f1);

        let mut f2 = Node::function("app.py::cool_func", "app.py", 60, 80);
        f2.complexity = Some(5);
        f2.loc = Some(20);
        g.add_node(f2);

        g.add_edge("app.py", "app.py::hot_func", EdgeType::Contains, 1.0)
            .unwrap();
        g.add_edge("app.py", "app.py::cool_func", EdgeType::Contains, 1.0)
            .unwrap();
        g.add_edge(
            "app.py::hot_func",
            "app.py::cool_func",
            EdgeType::Calls,
            1.0,
        )
        .unwrap();

        // Function-level change metrics
        g.change_metrics.insert(
            "app.py::hot_func".to_string(),
            ChangeMetrics {
                change_freq: 20,
                churn_lines: 200,
                churn_rate: 10.0,
                hotspot_score: 0.8,
                ..Default::default()
            },
        );
        g.change_metrics.insert(
            "app.py::cool_func".to_string(),
            ChangeMetrics {
                change_freq: 2,
                churn_lines: 10,
                churn_rate: 5.0,
                hotspot_score: 0.1,
                ..Default::default()
            },
        );

        // Module-level metrics for the module part of the risk field
        g.change_metrics.insert(
            "app.py".to_string(),
            ChangeMetrics {
                change_freq: 22,
                churn_lines: 210,
                churn_rate: 9.5,
                hotspot_score: 0.7,
                ..Default::default()
            },
        );

        let config = Config::default();
        let field = compute_risk_field(&g, &config, None, None, None);

        // Should have 1 module + 2 function nodes
        assert_eq!(field.nodes.len(), 3, "Should have 1 module + 2 functions");

        // Find function nodes
        let hot = field
            .nodes
            .iter()
            .find(|n| n.node_id == "app.py::hot_func")
            .expect("hot_func should be in risk field");
        let cool = field
            .nodes
            .iter()
            .find(|n| n.node_id == "app.py::cool_func")
            .expect("cool_func should be in risk field");

        // hot_func should have higher risk than cool_func
        assert!(
            hot.direct_score > cool.direct_score,
            "hot_func ({}) should have higher direct_score than cool_func ({})",
            hot.direct_score,
            cool.direct_score,
        );

        // Both should have capacity > 0
        assert!(hot.capacity > 0.0);
        assert!(cool.capacity > 0.0);
    }

    #[test]
    fn test_function_risks_not_added_without_functions() {
        let g = make_test_graph();
        let config = Config::default();
        let field = compute_risk_field(&g, &config, None, None, None);

        // make_test_graph only has modules, no functions
        assert_eq!(field.nodes.len(), 3, "Should only have 3 module nodes");
        // All nodes should be module-level (no "::" in ID)
        for n in &field.nodes {
            assert!(!n.node_id.contains("::"), "No function nodes expected");
        }
    }
}
