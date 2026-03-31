//! Graph builders for the Ising three-layer code graph engine.
//!
//! - **Structural builder** (Layer 1): uses Tree-sitter to parse AST and extract
//!   modules, classes, functions, imports, and their relationships.
//! - **Change builder** (Layer 2): uses gix (gitoxide) to analyze git history
//!   and compute temporal coupling, hotspots, and churn metrics.

pub mod change;
pub mod common;
pub mod languages;
pub mod structural;

use ising_core::config::Config;
use ising_core::graph::UnifiedGraph;
use ising_core::ignore::IgnoreRules;
use std::path::Path;

/// Build the complete multi-layer graph for a repository.
pub fn build_all(repo_path: &Path, config: &Config) -> Result<UnifiedGraph, anyhow::Error> {
    let ignore = IgnoreRules::load(repo_path);
    if ignore.has_user_rules() {
        tracing::info!("Loaded .isingignore rules");
    }

    tracing::info!("Building structural graph...");
    let structural = structural::build_structural_graph(repo_path, &ignore)?;
    tracing::info!(
        "Structural graph: {} nodes, {} edges",
        structural.node_count(),
        structural.edge_count()
    );

    tracing::info!("Building change graph...");
    let change = change::build_change_graph(repo_path, config, &ignore)?;
    tracing::info!(
        "Change graph: {} nodes, {} edges",
        change.node_count(),
        change.edge_count()
    );

    let mut graph = structural;
    graph.merge(change);
    tracing::info!(
        "Merged graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    // Attribute file-level change metrics to function nodes proportionally
    attribute_changes_to_functions(&mut graph);

    Ok(graph)
}

/// Attribute file-level change metrics to contained function/class nodes.
///
/// Uses line range proportional attribution: if a function spans 30% of a file's
/// lines, it gets ~30% of the file's change metrics. This is a simple heuristic
/// that's more accurate than nothing but less accurate than hunk-level attribution.
fn attribute_changes_to_functions(graph: &mut UnifiedGraph) {
    use ising_core::graph::{ChangeMetrics, EdgeType, NodeType};

    // Collect module -> children with their line ranges
    let contains_edges = graph.edges_of_type(&EdgeType::Contains);
    let mut module_children: std::collections::HashMap<String, Vec<(String, u32, u32)>> =
        std::collections::HashMap::new();
    for (module_id, child_id, _) in &contains_edges {
        if let Some(child) = graph.get_node(child_id) {
            if child.node_type == NodeType::Function || child.node_type == NodeType::Class {
                if let (Some(start), Some(end)) = (child.line_start, child.line_end) {
                    module_children
                        .entry(module_id.to_string())
                        .or_default()
                        .push((child_id.to_string(), start, end));
                }
            }
        }
    }

    // For each module with change metrics, distribute to children
    let mut func_metrics: Vec<(String, ChangeMetrics)> = Vec::new();

    for (module_id, children) in &module_children {
        let metrics = match graph.change_metrics.get(module_id.as_str()) {
            Some(m) if m.change_freq > 0 => m.clone(),
            _ => continue,
        };

        let module_loc = graph
            .get_node(module_id)
            .and_then(|n| n.loc)
            .unwrap_or(1) as f64;

        for (child_id, start, end) in children {
            let child_lines = (end.saturating_sub(*start) + 1) as f64;
            let proportion = (child_lines / module_loc).min(1.0);

            // Attribute proportionally
            let child_change_freq = (metrics.change_freq as f64 * proportion).ceil() as u32;
            let child_churn = (metrics.churn_lines as f64 * proportion).ceil() as u32;
            let child_churn_rate = if child_change_freq > 0 {
                child_churn as f64 / child_change_freq as f64
            } else {
                0.0
            };

            func_metrics.push((
                child_id.clone(),
                ChangeMetrics {
                    change_freq: child_change_freq,
                    churn_lines: child_churn,
                    churn_rate: child_churn_rate,
                    hotspot_score: metrics.hotspot_score * proportion,
                    sum_coupling: 0.0, // Functions don't have co-change coupling yet
                    last_changed: metrics.last_changed.clone(),
                },
            ));
        }
    }

    // Insert the attributed metrics
    let count = func_metrics.len();
    for (id, metrics) in func_metrics {
        graph.change_metrics.insert(id, metrics);
    }
    if count > 0 {
        tracing::info!("Attributed change metrics to {} function/class nodes", count);
    }
}
