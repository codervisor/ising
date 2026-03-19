//! Hotspot ranking following Tornhill's model.
//!
//! Hotspot = normalized(change_frequency) × normalized(complexity)

use ising_core::graph::UnifiedGraph;
use serde::Serialize;

/// A ranked hotspot entry.
#[derive(Debug, Clone, Serialize)]
pub struct Hotspot {
    pub node_id: String,
    pub file_path: String,
    pub hotspot_score: f64,
    pub change_freq: u32,
    pub complexity: Option<u32>,
    pub churn_rate: f64,
}

/// Rank nodes by hotspot score (change frequency × complexity).
pub fn rank_hotspots(graph: &UnifiedGraph, top_n: usize) -> Vec<Hotspot> {
    let mut hotspots: Vec<Hotspot> = graph
        .node_ids()
        .filter_map(|id| {
            let node = graph.get_node(id)?;
            let change = graph.change_metrics.get(id);
            let hotspot_score = change.map(|m| m.hotspot_score).unwrap_or(0.0);
            let change_freq = change.map(|m| m.change_freq).unwrap_or(0);

            // Only include nodes that have been changed at least once
            if change_freq == 0 {
                return None;
            }

            Some(Hotspot {
                node_id: id.to_string(),
                file_path: node.file_path.clone(),
                hotspot_score,
                change_freq,
                complexity: node.complexity,
                churn_rate: change.map(|m| m.churn_rate).unwrap_or(0.0),
            })
        })
        .collect();

    hotspots.sort_by(|a, b| {
        b.hotspot_score
            .partial_cmp(&a.hotspot_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hotspots.truncate(top_n);
    hotspots
}

#[cfg(test)]
mod tests {
    use super::*;
    use ising_core::graph::{ChangeMetrics, Node};

    #[test]
    fn test_rank_hotspots() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_node(Node::module("c", "c.py"));

        g.change_metrics.insert(
            "a".to_string(),
            ChangeMetrics {
                change_freq: 30,
                hotspot_score: 0.9,
                ..Default::default()
            },
        );
        g.change_metrics.insert(
            "b".to_string(),
            ChangeMetrics {
                change_freq: 5,
                hotspot_score: 0.2,
                ..Default::default()
            },
        );
        // c has no change metrics

        let hotspots = rank_hotspots(&g, 10);
        assert_eq!(hotspots.len(), 2); // c excluded (no changes)
        assert_eq!(hotspots[0].node_id, "a");
        assert_eq!(hotspots[1].node_id, "b");
    }
}
