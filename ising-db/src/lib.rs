//! SQLite storage for the Ising code graph engine.
//!
//! Persists nodes, edges, change/defect metrics, and cross-layer signals
//! to a single SQLite file for fast CLI queries and MCP tool serving.

mod schema;
mod queries;
pub mod export;

use ising_core::graph::ChangeMetrics;
use rusqlite::Connection;

pub use export::{VizEdge, VizExport, VizMeta, VizNode, VizSignal};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A stored cross-layer signal.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredSignal {
    pub id: i64,
    pub signal_type: String,
    pub node_a: String,
    pub node_b: Option<String>,
    pub severity: f64,
    pub details: Option<serde_json::Value>,
    pub detected_at: String,
}

/// Result of an impact query.
#[derive(Debug, Default, serde::Serialize)]
pub struct ImpactResult {
    pub structural_deps: Vec<(String, String, f64)>,
    pub temporal_coupling: Vec<(String, f64)>,
    pub signals: Vec<StoredSignal>,
    pub change_metrics: Option<ChangeMetrics>,
}

/// Basic database statistics.
#[derive(Debug, serde::Serialize)]
pub struct DbStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub signal_count: usize,
    pub structural_edges: usize,
    pub change_edges: usize,
}

/// Database handle for Ising storage.
pub struct Database {
    pub(crate) conn: Connection,
}

impl Database {
    /// Open (or create) the database at the given path and initialize schema.
    pub fn open(path: &str) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ising_core::graph::{ChangeMetrics, EdgeType, Node, UnifiedGraph};

    #[test]
    fn test_create_and_query_db() {
        let db = Database::open_in_memory().unwrap();

        let mut graph = UnifiedGraph::new();
        graph.add_node(Node::module("a", "a.py"));
        graph.add_node(Node::module("b", "b.py"));
        graph.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        graph.add_edge("a", "b", EdgeType::CoChanges, 0.7).unwrap();
        graph.change_metrics.insert(
            "a".to_string(),
            ChangeMetrics {
                change_freq: 20,
                hotspot_score: 0.85,
                ..Default::default()
            },
        );

        db.store_graph(&graph).unwrap();

        let stats = db.get_stats().unwrap();
        assert_eq!(stats.node_count, 2);
        assert_eq!(stats.edge_count, 2);
        assert_eq!(stats.structural_edges, 1);
        assert_eq!(stats.change_edges, 1);
    }

    #[test]
    fn test_signals_storage_and_query() {
        let db = Database::open_in_memory().unwrap();

        // Insert nodes referenced by signals (FK constraint)
        let mut graph = UnifiedGraph::new();
        for id in &["a", "b", "c", "d", "e"] {
            graph.add_node(Node::module(*id, format!("{id}.py")));
        }
        db.store_graph(&graph).unwrap();

        db.store_signal("ghost_coupling", "a", Some("b"), 0.8, None)
            .unwrap();
        db.store_signal("ticking_bomb", "c", None, 0.9, None)
            .unwrap();
        db.store_signal("ghost_coupling", "d", Some("e"), 0.3, None)
            .unwrap();

        let all = db.get_signals(None, None).unwrap();
        assert_eq!(all.len(), 3);

        let ghost = db.get_signals(Some("ghost_coupling"), None).unwrap();
        assert_eq!(ghost.len(), 2);

        let high = db.get_signals(None, Some(0.5)).unwrap();
        assert_eq!(high.len(), 2);
    }

    #[test]
    fn test_impact_query() {
        let db = Database::open_in_memory().unwrap();

        let mut graph = UnifiedGraph::new();
        graph.add_node(Node::module("a", "a.py"));
        graph.add_node(Node::module("b", "b.py"));
        graph.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        graph.add_edge("a", "b", EdgeType::CoChanges, 0.7).unwrap();
        graph.change_metrics.insert(
            "a".to_string(),
            ChangeMetrics {
                change_freq: 20,
                hotspot_score: 0.85,
                ..Default::default()
            },
        );
        db.store_graph(&graph).unwrap();
        db.store_signal("ghost_coupling", "a", Some("b"), 0.8, None)
            .unwrap();

        let impact = db.get_impact("a").unwrap();
        assert_eq!(impact.structural_deps.len(), 1);
        assert_eq!(impact.temporal_coupling.len(), 1);
        assert_eq!(impact.signals.len(), 1);
        assert!(impact.change_metrics.is_some());
    }

    #[test]
    fn test_build_info() {
        let db = Database::open_in_memory().unwrap();
        db.set_build_info("last_build", "2025-01-01T00:00:00")
            .unwrap();
        assert_eq!(
            db.get_build_info("last_build").unwrap(),
            Some("2025-01-01T00:00:00".to_string())
        );
        assert_eq!(db.get_build_info("nonexistent").unwrap(), None);
    }

    #[test]
    fn test_hotspots_query() {
        let db = Database::open_in_memory().unwrap();

        let mut graph = UnifiedGraph::new();
        let mut node_a = Node::module("a", "a.py");
        node_a.complexity = Some(20);
        let mut node_b = Node::module("b", "b.py");
        node_b.complexity = Some(5);
        graph.add_node(node_a);
        graph.add_node(node_b);
        graph.change_metrics.insert(
            "a".to_string(),
            ChangeMetrics {
                change_freq: 10,
                hotspot_score: 0.9,
                ..Default::default()
            },
        );
        graph.change_metrics.insert(
            "b".to_string(),
            ChangeMetrics {
                change_freq: 3,
                hotspot_score: 0.3,
                ..Default::default()
            },
        );
        db.store_graph(&graph).unwrap();

        let hotspots = db.get_hotspots(10).unwrap();
        assert_eq!(hotspots.len(), 2);
        // "a" has highest score: (10/10) * (20/20) = 1.0
        assert_eq!(hotspots[0].0, "a");
        // "b" has lower score: (3/10) * (5/20) = 0.075
        assert_eq!(hotspots[1].0, "b");
    }
}
