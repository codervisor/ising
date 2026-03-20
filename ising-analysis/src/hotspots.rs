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
///
/// When complexity data is available, the score is:
///   normalized(change_freq) × normalized(complexity)
/// When complexity is unavailable, falls back to normalized(change_freq).
pub fn rank_hotspots(graph: &UnifiedGraph, top_n: usize) -> Vec<Hotspot> {
    // First pass: collect raw data and find max values for normalization
    let mut raw: Vec<(String, String, u32, Option<u32>, f64)> = graph
        .node_ids()
        .filter_map(|id| {
            let node = graph.get_node(id)?;
            let change = graph.change_metrics.get(id);
            let change_freq = change.map(|m| m.change_freq).unwrap_or(0);
            if change_freq == 0 {
                return None;
            }
            let churn_rate = change.map(|m| m.churn_rate).unwrap_or(0.0);
            Some((
                id.to_string(),
                node.file_path.clone(),
                change_freq,
                node.complexity,
                churn_rate,
            ))
        })
        .collect();

    let max_freq = raw.iter().map(|r| r.2).max().unwrap_or(1).max(1) as f64;
    let max_complexity = raw.iter().filter_map(|r| r.3).max().unwrap_or(1).max(1) as f64;

    let mut hotspots: Vec<Hotspot> = raw
        .drain(..)
        .map(
            |(node_id, file_path, change_freq, complexity, churn_rate)| {
                let norm_freq = change_freq as f64 / max_freq;
                let c = complexity.unwrap_or(1).max(1);
                let norm_complexity = c as f64 / max_complexity;
                let hotspot_score = norm_freq * norm_complexity;

                Hotspot {
                    node_id,
                    file_path,
                    hotspot_score,
                    change_freq,
                    complexity,
                    churn_rate,
                }
            },
        )
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
