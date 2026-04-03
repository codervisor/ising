//! SQLite storage for the Ising code graph engine.
//!
//! Persists nodes, edges, change/defect metrics, and cross-layer signals
//! to a single SQLite file for fast CLI queries and MCP tool serving.

pub mod export;
mod queries;
mod schema;

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

/// Stored risk data for a node.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredRisk {
    pub node_id: String,
    pub change_load: f64,
    pub structural_weight: f64,
    pub propagated_risk: f64,
    pub risk_score: f64,
    pub capacity: f64,
    pub safety_factor: f64,
    pub zone: String,
    pub direct_score: f64,
    pub risk_tier: String,
    pub percentile: f64,
}

/// Stored health index for the repository.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredHealth {
    pub score: f64,
    pub grade: String,
    pub active_modules: usize,
    pub total_modules: usize,
    pub critical_count: usize,
    pub high_count: usize,
    pub risk_concentration: f64,
    pub avg_direct_score: f64,
    pub frac_stable: f64,
    pub frac_healthy: f64,
    pub frac_warning: f64,
    pub frac_danger: f64,
    pub frac_critical: f64,
    pub lambda_max: f64,
    pub signal_density: f64,
    pub god_module_density: f64,
    pub cycle_density: f64,
    pub unstable_dep_density: f64,
    pub zone_sub_score: f64,
    pub coupling_modifier: f64,
    pub signal_penalty: f64,
    pub risk_sub_score: f64,
    pub signal_sub_score: f64,
    pub structural_sub_score: f64,
    pub boundary_health_score: f64,
    pub caveats: Vec<String>,
}

/// Database handle for Ising storage.
pub struct Database {
    pub(crate) conn: Connection,
}

impl Database {
    /// Open (or create) the database at the given path and initialize schema.
    pub fn open(path: &str) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ising_core::fea::{NodeRisk, RiskField, RiskTier, SafetyZone};
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

    #[test]
    fn test_store_and_query_risk() {
        let db = Database::open_in_memory().unwrap();

        // Insert nodes first (risk_data references nodes via FK)
        let mut graph = UnifiedGraph::new();
        graph.add_node(Node::module("a", "a.py"));
        graph.add_node(Node::module("b", "b.py"));
        db.store_graph(&graph).unwrap();

        let field = RiskField {
            nodes: vec![
                NodeRisk {
                    node_id: "a".to_string(),
                    file_path: "a.py".to_string(),
                    change_load: 0.9,
                    structural_weight: 0.7,
                    propagated_risk: 0.1,
                    risk_score: 1.0,
                    capacity: 0.3,
                    safety_factor: 0.3,
                    zone: SafetyZone::Critical,
                    direct_score: 3.0,
                    risk_tier: RiskTier::Critical,
                    percentile: 100.0,
                },
                NodeRisk {
                    node_id: "b".to_string(),
                    file_path: "b.py".to_string(),
                    change_load: 0.0,
                    structural_weight: 0.1,
                    propagated_risk: 0.0,
                    risk_score: 0.0,
                    capacity: 0.9,
                    safety_factor: 10.0,
                    zone: SafetyZone::Stable,
                    direct_score: 0.0,
                    risk_tier: RiskTier::Normal,
                    percentile: 0.0,
                },
            ],
            iterations: 5,
            converged: true,
            health: None,
        };

        db.store_risk_field(&field).unwrap();

        // Query ranking — "a" should be first (highest direct_score)
        let ranking = db.get_safety_ranking(10).unwrap();
        assert_eq!(ranking.len(), 2);
        assert_eq!(ranking[0].node_id, "a");
        assert!(ranking[0].direct_score > ranking[1].direct_score);

        // Query by node
        let risk_a = db.get_node_risk("a").unwrap().unwrap();
        assert_eq!(risk_a.zone, "critical");
        assert!((risk_a.risk_score - 1.0).abs() < 0.01);

        // Query by zone
        let critical = db.get_nodes_by_zone("critical").unwrap();
        assert_eq!(critical.len(), 1);
        assert_eq!(critical[0].node_id, "a");
    }

    #[test]
    fn test_load_graph_roundtrip() {
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
                churn_lines: 100,
                churn_rate: 5.0,
                hotspot_score: 0.85,
                sum_coupling: 0.7,
                ..Default::default()
            },
        );
        db.store_graph(&graph).unwrap();

        let loaded = db.load_graph().unwrap();
        assert_eq!(loaded.node_count(), 2);
        assert_eq!(loaded.edge_count(), 2);
        assert!(loaded.get_node("a").is_some());
        assert!(loaded.get_node("b").is_some());
        assert!(loaded.has_structural_edge("a", "b"));

        let cm = loaded.change_metrics.get("a").unwrap();
        assert_eq!(cm.change_freq, 20);
        assert!((cm.churn_rate - 5.0).abs() < f64::EPSILON);
    }
}
