//! Graph builders for the Ising three-layer code graph engine.
//!
//! - **Structural builder** (Layer 1): uses Tree-sitter to parse AST and extract
//!   modules, classes, functions, imports, and their relationships.
//! - **Change builder** (Layer 2): uses gix (gitoxide) to analyze git history
//!   and compute temporal coupling, hotspots, and churn metrics.

pub mod change;
pub mod structural;

use ising_core::config::Config;
use ising_core::graph::UnifiedGraph;
use std::path::Path;

/// Build the complete multi-layer graph for a repository.
pub fn build_all(repo_path: &Path, config: &Config) -> Result<UnifiedGraph, anyhow::Error> {
    tracing::info!("Building structural graph...");
    let structural = structural::build_structural_graph(repo_path)?;
    tracing::info!(
        "Structural graph: {} nodes, {} edges",
        structural.node_count(),
        structural.edge_count()
    );

    tracing::info!("Building change graph...");
    let change = change::build_change_graph(repo_path, config)?;
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

    Ok(graph)
}
