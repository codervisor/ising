//! Boundary health metrics computation.
//!
//! Computes per-module boundary health: containment ratio, coupling ratio,
//! internal stress, and risk import/export. Used by the health index to
//! produce boundary-aware grades.

use ising_core::boundary::BoundaryStructure;
use ising_core::fea::{BoundaryHealth, BoundaryHealthReport, NodeRisk, SafetyZone};
use ising_core::graph::{EdgeType, UnifiedGraph};
use std::collections::HashMap;

/// Compute boundary health metrics for all detected modules.
///
/// Uses a single-pass edge pre-indexing strategy to avoid O(modules × edges).
pub fn compute_boundary_health(
    graph: &UnifiedGraph,
    boundaries: &BoundaryStructure,
    risk_nodes: &[NodeRisk],
) -> BoundaryHealthReport {
    // Build risk lookup
    let risk_map: HashMap<&str, &NodeRisk> =
        risk_nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();

    // Collect all unique modules and their members
    let mut module_members: HashMap<(String, String), Vec<String>> = HashMap::new();
    for (node_id, (pkg, module)) in &boundaries.assignments {
        module_members
            .entry((pkg.clone(), module.clone()))
            .or_default()
            .push(node_id.clone());
    }

    // Build node → module key lookup for O(1) module membership checks
    let node_module: HashMap<&str, (&str, &str)> = boundaries
        .assignments
        .iter()
        .map(|(id, (pkg, m))| (id.as_str(), (pkg.as_str(), m.as_str())))
        .collect();

    // Pre-index edge counts per module in a single pass over edges.
    // For each module: (total_change_edges, internal_change_edges, total_struct, cross_struct)
    type ModKey = (String, String);
    let mut change_counts: HashMap<ModKey, (usize, usize)> = HashMap::new();
    let mut struct_counts: HashMap<ModKey, (usize, usize)> = HashMap::new();

    let co_change_edges = graph.edges_of_type(&EdgeType::CoChanges);
    for &(a, b, _) in &co_change_edges {
        let mod_a = node_module.get(a);
        let mod_b = node_module.get(b);
        let same_mod = matches!((mod_a, mod_b), (Some(ma), Some(mb)) if ma == mb);

        if let Some(&(pkg, m)) = mod_a {
            let entry = change_counts
                .entry((pkg.to_string(), m.to_string()))
                .or_insert((0, 0));
            entry.0 += 1;
            if same_mod {
                entry.1 += 1;
            }
        }
        // Only count for b's module if a and b are in different modules
        // (if same module, already counted above)
        if let Some(&(pkg, m)) = mod_b
            && !same_mod
        {
            let entry = change_counts
                .entry((pkg.to_string(), m.to_string()))
                .or_insert((0, 0));
            entry.0 += 1;
        }
    }

    let import_edges = graph.edges_of_type(&EdgeType::Imports);
    for &(a, b, _) in &import_edges {
        let mod_a = node_module.get(a);
        let mod_b = node_module.get(b);
        let same_mod = matches!((mod_a, mod_b), (Some(ma), Some(mb)) if ma == mb);

        if let Some(&(pkg, m)) = mod_a {
            let entry = struct_counts
                .entry((pkg.to_string(), m.to_string()))
                .or_insert((0, 0));
            entry.0 += 1;
            if !same_mod {
                entry.1 += 1;
            }
        }
        if let Some(&(pkg, m)) = mod_b
            && !same_mod
        {
            let entry = struct_counts
                .entry((pkg.to_string(), m.to_string()))
                .or_insert((0, 0));
            entry.0 += 1;
            entry.1 += 1;
        }
    }

    let mut modules = Vec::new();

    for ((pkg, module), members) in &module_members {
        let key = (pkg.clone(), module.clone());

        // --- Containment ratio ---
        let (total_change_edges, internal_change_edges) =
            change_counts.get(&key).copied().unwrap_or((0, 0));
        let containment_ratio = if total_change_edges > 0 {
            internal_change_edges as f64 / total_change_edges as f64
        } else {
            1.0 // No change edges = perfectly contained (no leaks)
        };

        // --- Coupling ratio ---
        let (total_structural, cross_structural) =
            struct_counts.get(&key).copied().unwrap_or((0, 0));
        let coupling_ratio = if total_structural > 0 {
            cross_structural as f64 / total_structural as f64
        } else {
            0.0
        };

        // --- Internal stress ---
        // Fraction of module's nodes in Critical/Danger zone
        let mut stress_count = 0usize;
        let mut active_count = 0usize;

        for member in members {
            if let Some(risk) = risk_map.get(member.as_str())
                && risk.change_load > 0.0
            {
                active_count += 1;
                if matches!(risk.zone, SafetyZone::Critical | SafetyZone::Danger) {
                    stress_count += 1;
                }
            }
        }

        let internal_stress = if active_count > 0 {
            stress_count as f64 / active_count as f64
        } else {
            0.0
        };

        // --- Risk export/import ---
        // How much propagated risk crosses the boundary
        let mut risk_export = 0.0f64;
        let mut risk_import = 0.0f64;

        for member in members {
            if let Some(risk) = risk_map.get(member.as_str()) {
                // A module with high propagated risk but low change_load is a risk importer
                if risk.change_load > 0.01 && risk.propagated_risk > 0.0 {
                    // Approximate: risk that originates here and goes out
                    risk_export += risk.change_load * coupling_ratio;
                }
                if risk.propagated_risk > risk.change_load * 0.5 {
                    risk_import += risk.propagated_risk;
                }
            }
        }

        let module_id = if module == "_root" {
            pkg.clone()
        } else {
            format!("{}::{}", pkg, module)
        };

        modules.push(BoundaryHealth {
            module_id,
            member_count: members.len(),
            containment_ratio,
            coupling_ratio,
            internal_stress,
            risk_export,
            risk_import,
        });
    }

    // Sort by containment (worst first)
    modules.sort_by(|a, b| {
        a.containment_ratio
            .partial_cmp(&b.containment_ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Compute aggregates (weighted by member count)
    let total_members: usize = modules.iter().map(|m| m.member_count).sum();
    let total_f = total_members.max(1) as f64;

    let avg_containment = modules
        .iter()
        .map(|m| m.containment_ratio * m.member_count as f64)
        .sum::<f64>()
        / total_f;

    let avg_coupling_ratio = modules
        .iter()
        .map(|m| m.coupling_ratio * m.member_count as f64)
        .sum::<f64>()
        / total_f;

    let leaky_boundary_count = modules
        .iter()
        .filter(|m| m.containment_ratio < 0.5 && m.member_count >= 2)
        .count();

    BoundaryHealthReport {
        modules,
        avg_containment,
        avg_coupling_ratio,
        leaky_boundary_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ising_core::boundary::BoundaryStructure;
    use ising_core::fea::{NodeRisk, RiskTier};
    use ising_core::graph::{ChangeMetrics, Node, UnifiedGraph};

    fn make_test_graph_with_boundaries() -> (UnifiedGraph, BoundaryStructure) {
        let mut g = UnifiedGraph::new();

        // Module A in package "auth"
        let mut a = Node::module("auth/login.py", "auth/login.py");
        a.complexity = Some(20);
        a.loc = Some(100);
        g.add_node(a);

        let mut b = Node::module("auth/register.py", "auth/register.py");
        b.complexity = Some(15);
        b.loc = Some(80);
        g.add_node(b);

        // Module C in package "api"
        let mut c = Node::module("api/routes.py", "api/routes.py");
        c.complexity = Some(30);
        c.loc = Some(200);
        g.add_node(c);

        // Structural edges
        g.add_edge("api/routes.py", "auth/login.py", EdgeType::Imports, 1.0)
            .unwrap();

        // Co-change: auth files co-change (internal), api+auth co-change (cross-boundary)
        g.add_edge(
            "auth/login.py",
            "auth/register.py",
            EdgeType::CoChanges,
            0.8,
        )
        .unwrap();
        g.add_edge("auth/login.py", "api/routes.py", EdgeType::CoChanges, 0.6)
            .unwrap();

        // Change metrics
        g.change_metrics.insert(
            "auth/login.py".to_string(),
            ChangeMetrics {
                change_freq: 20,
                churn_rate: 10.0,
                ..Default::default()
            },
        );
        g.change_metrics.insert(
            "auth/register.py".to_string(),
            ChangeMetrics {
                change_freq: 15,
                churn_rate: 8.0,
                ..Default::default()
            },
        );
        g.change_metrics.insert(
            "api/routes.py".to_string(),
            ChangeMetrics {
                change_freq: 25,
                churn_rate: 12.0,
                ..Default::default()
            },
        );

        let node_ids = &["auth/login.py", "auth/register.py", "api/routes.py"];
        let bs = BoundaryStructure::detect(std::path::Path::new("/nonexistent"), node_ids);

        (g, bs)
    }

    #[test]
    fn test_boundary_health_computation() {
        let (g, bs) = make_test_graph_with_boundaries();

        // Create mock risk nodes
        let risk_nodes = vec![
            NodeRisk {
                node_id: "auth/login.py".to_string(),
                file_path: "auth/login.py".to_string(),
                change_load: 0.5,
                structural_weight: 0.3,
                propagated_risk: 0.2,
                risk_score: 0.7,
                capacity: 0.6,
                safety_factor: 0.86,
                zone: SafetyZone::Critical,
                direct_score: 0.83,
                risk_tier: RiskTier::Critical,
                percentile: 99.0,
            },
            NodeRisk {
                node_id: "auth/register.py".to_string(),
                file_path: "auth/register.py".to_string(),
                change_load: 0.3,
                structural_weight: 0.2,
                propagated_risk: 0.1,
                risk_score: 0.4,
                capacity: 0.7,
                safety_factor: 1.75,
                zone: SafetyZone::Warning,
                direct_score: 0.43,
                risk_tier: RiskTier::Normal,
                percentile: 50.0,
            },
            NodeRisk {
                node_id: "api/routes.py".to_string(),
                file_path: "api/routes.py".to_string(),
                change_load: 0.6,
                structural_weight: 0.4,
                propagated_risk: 0.3,
                risk_score: 0.9,
                capacity: 0.5,
                safety_factor: 0.56,
                zone: SafetyZone::Critical,
                direct_score: 1.2,
                risk_tier: RiskTier::Critical,
                percentile: 100.0,
            },
        ];

        let report = compute_boundary_health(&g, &bs, &risk_nodes);

        assert!(!report.modules.is_empty());
        assert!(report.avg_containment >= 0.0 && report.avg_containment <= 1.0);
    }
}
