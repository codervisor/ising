//! SQLite storage for the Ising code graph engine.
//!
//! Persists nodes, edges, change/defect metrics, and cross-layer signals
//! to a single SQLite file for fast CLI queries and MCP tool serving.

use ising_core::graph::{ChangeMetrics, NodeType, UnifiedGraph};
use rusqlite::{params, Connection, Result as SqlResult};

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

/// Database handle for Ising storage.
pub struct Database {
    conn: Connection,
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

    fn init_schema(&self) -> Result<(), DbError> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS nodes (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                file_path TEXT NOT NULL,
                line_start INTEGER,
                line_end INTEGER,
                language TEXT,
                loc INTEGER,
                complexity INTEGER,
                nesting_depth INTEGER
            );

            CREATE TABLE IF NOT EXISTS edges (
                source TEXT NOT NULL,
                target TEXT NOT NULL,
                layer TEXT NOT NULL,
                edge_type TEXT NOT NULL,
                weight REAL DEFAULT 1.0,
                metadata JSON,
                FOREIGN KEY (source) REFERENCES nodes(id),
                FOREIGN KEY (target) REFERENCES nodes(id)
            );

            CREATE TABLE IF NOT EXISTS change_metrics (
                node_id TEXT PRIMARY KEY,
                change_freq INTEGER,
                churn_lines INTEGER,
                churn_rate REAL,
                hotspot_score REAL,
                sum_coupling REAL,
                last_changed TEXT,
                FOREIGN KEY (node_id) REFERENCES nodes(id)
            );

            CREATE TABLE IF NOT EXISTS defect_metrics (
                node_id TEXT PRIMARY KEY,
                bug_count INTEGER,
                defect_density REAL,
                fix_inducing_rate REAL,
                FOREIGN KEY (node_id) REFERENCES nodes(id)
            );

            CREATE TABLE IF NOT EXISTS signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                signal_type TEXT NOT NULL,
                node_a TEXT NOT NULL,
                node_b TEXT,
                severity REAL NOT NULL,
                details JSON,
                detected_at TEXT NOT NULL,
                FOREIGN KEY (node_a) REFERENCES nodes(id)
            );

            CREATE TABLE IF NOT EXISTS build_info (
                key TEXT PRIMARY KEY,
                value TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);
            CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);
            CREATE INDEX IF NOT EXISTS idx_edges_layer ON edges(layer);
            CREATE INDEX IF NOT EXISTS idx_signals_type ON signals(signal_type);
            CREATE INDEX IF NOT EXISTS idx_signals_severity ON signals(severity DESC);
            ",
        )?;
        Ok(())
    }

    /// Clear all data (for rebuilds).
    pub fn clear(&self) -> Result<(), DbError> {
        self.conn.execute_batch(
            "
            DELETE FROM signals;
            DELETE FROM change_metrics;
            DELETE FROM defect_metrics;
            DELETE FROM edges;
            DELETE FROM nodes;
            DELETE FROM build_info;
            ",
        )?;
        Ok(())
    }

    /// Store a complete UnifiedGraph to the database.
    pub fn store_graph(&self, graph: &UnifiedGraph) -> Result<(), DbError> {
        let tx = self.conn.unchecked_transaction()?;

        // Insert nodes
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO nodes (id, type, file_path, line_start, line_end, language, loc, complexity, nesting_depth)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for idx in graph.graph.node_indices() {
                let node = &graph.graph[idx];
                let node_type = match &node.node_type {
                    NodeType::Module => "module",
                    NodeType::Class => "class",
                    NodeType::Function => "function",
                    NodeType::Import => "import",
                };
                stmt.execute(params![
                    node.id,
                    node_type,
                    node.file_path,
                    node.line_start,
                    node.line_end,
                    node.language,
                    node.loc,
                    node.complexity,
                    node.nesting_depth,
                ])?;
            }
        }

        // Insert edges
        {
            let mut stmt = tx.prepare(
                "INSERT INTO edges (source, target, layer, edge_type, weight, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for ei in graph.graph.edge_indices() {
                let edge = &graph.graph[ei];
                if let Some((src, tgt)) = graph.graph.edge_endpoints(ei) {
                    let src_id = &graph.graph[src].id;
                    let tgt_id = &graph.graph[tgt].id;
                    let layer = format!("{:?}", edge.edge_type.layer()).to_lowercase();
                    let edge_type = serde_json::to_value(&edge.edge_type)?;
                    let metadata = edge
                        .metadata
                        .as_ref()
                        .map(|m| serde_json::to_string(m))
                        .transpose()?;
                    stmt.execute(params![
                        src_id,
                        tgt_id,
                        layer,
                        edge_type.as_str().unwrap_or("unknown"),
                        edge.weight,
                        metadata,
                    ])?;
                }
            }
        }

        // Insert change metrics
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO change_metrics (node_id, change_freq, churn_lines, churn_rate, hotspot_score, sum_coupling, last_changed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (node_id, cm) in &graph.change_metrics {
                stmt.execute(params![
                    node_id,
                    cm.change_freq,
                    cm.churn_lines,
                    cm.churn_rate,
                    cm.hotspot_score,
                    cm.sum_coupling,
                    cm.last_changed,
                ])?;
            }
        }

        // Insert defect metrics
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO defect_metrics (node_id, bug_count, defect_density, fix_inducing_rate)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (node_id, dm) in &graph.defect_metrics {
                stmt.execute(params![
                    node_id, dm.bug_count, dm.defect_density, dm.fix_inducing_rate,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Store a signal.
    pub fn store_signal(
        &self,
        signal_type: &str,
        node_a: &str,
        node_b: Option<&str>,
        severity: f64,
        details: Option<&serde_json::Value>,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let details_str = details
            .map(|d| serde_json::to_string(d))
            .transpose()?;
        self.conn.execute(
            "INSERT INTO signals (signal_type, node_a, node_b, severity, details, detected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![signal_type, node_a, node_b, severity, details_str, now],
        )?;
        Ok(())
    }

    /// Store build metadata.
    pub fn set_build_info(&self, key: &str, value: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO build_info (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Retrieve build metadata.
    pub fn get_build_info(&self, key: &str) -> Result<Option<String>, DbError> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM build_info WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Query hotspots ranked by hotspot_score.
    pub fn get_hotspots(&self, top_n: usize) -> Result<Vec<(String, f64, u32, f64)>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, COALESCE(cm.hotspot_score, 0), COALESCE(n.complexity, 0), COALESCE(cm.change_freq, 0)
             FROM nodes n
             LEFT JOIN change_metrics cm ON n.id = cm.node_id
             ORDER BY cm.hotspot_score DESC NULLS LAST
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![top_n as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    /// Query signals, optionally filtered by type and minimum severity.
    pub fn get_signals(
        &self,
        signal_type: Option<&str>,
        min_severity: Option<f64>,
    ) -> Result<Vec<StoredSignal>, DbError> {
        let mut sql = String::from(
            "SELECT id, signal_type, node_a, node_b, severity, details, detected_at FROM signals WHERE 1=1",
        );
        if signal_type.is_some() {
            sql.push_str(" AND signal_type = ?1");
        }
        if min_severity.is_some() {
            sql.push_str(if signal_type.is_some() {
                " AND severity >= ?2"
            } else {
                " AND severity >= ?1"
            });
        }
        sql.push_str(" ORDER BY severity DESC");

        let mut stmt = self.conn.prepare(&sql)?;

        let rows: Vec<StoredSignal> = match (signal_type, min_severity) {
            (Some(st), Some(ms)) => stmt
                .query_map(params![st, ms], map_signal_row)?
                .collect::<SqlResult<Vec<_>>>()?,
            (Some(st), None) => stmt
                .query_map(params![st], map_signal_row)?
                .collect::<SqlResult<Vec<_>>>()?,
            (None, Some(ms)) => stmt
                .query_map(params![ms], map_signal_row)?
                .collect::<SqlResult<Vec<_>>>()?,
            (None, None) => stmt
                .query_map([], map_signal_row)?
                .collect::<SqlResult<Vec<_>>>()?,
        };
        Ok(rows)
    }

    /// Get impact data for a node: its neighbors and related signals.
    pub fn get_impact(&self, node_id: &str) -> Result<ImpactResult, DbError> {
        // Get node info
        let node_exists: bool = self.conn.query_row(
            "SELECT COUNT(*) FROM nodes WHERE id = ?1",
            params![node_id],
            |row| row.get::<_, i64>(0),
        )? > 0;

        if !node_exists {
            return Ok(ImpactResult::default());
        }

        // Structural dependencies (outgoing)
        let mut stmt = self.conn.prepare(
            "SELECT target, edge_type, weight FROM edges WHERE source = ?1 AND layer = 'structural'",
        )?;
        let structural_deps: Vec<(String, String, f64)> = stmt
            .query_map(params![node_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })?
            .collect::<SqlResult<Vec<_>>>()?;

        // Temporal coupling
        let mut stmt = self.conn.prepare(
            "SELECT target, weight FROM edges WHERE source = ?1 AND edge_type = 'co_changes'
             UNION
             SELECT source, weight FROM edges WHERE target = ?1 AND edge_type = 'co_changes'
             ORDER BY weight DESC",
        )?;
        let temporal_coupling: Vec<(String, f64)> = stmt
            .query_map(params![node_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?
            .collect::<SqlResult<Vec<_>>>()?;

        // Signals involving this node
        let mut stmt = self.conn.prepare(
            "SELECT id, signal_type, node_a, node_b, severity, details, detected_at
             FROM signals WHERE node_a = ?1 OR node_b = ?1
             ORDER BY severity DESC",
        )?;
        let signals: Vec<StoredSignal> = stmt
            .query_map(params![node_id], map_signal_row)?
            .collect::<SqlResult<Vec<_>>>()?;

        // Change metrics
        let change_metrics = self
            .conn
            .query_row(
                "SELECT change_freq, churn_lines, churn_rate, hotspot_score, sum_coupling, last_changed
                 FROM change_metrics WHERE node_id = ?1",
                params![node_id],
                |row| {
                    Ok(ChangeMetrics {
                        change_freq: row.get(0)?,
                        churn_lines: row.get(1)?,
                        churn_rate: row.get(2)?,
                        hotspot_score: row.get(3)?,
                        sum_coupling: row.get(4)?,
                        last_changed: row.get(5)?,
                    })
                },
            )
            .ok();

        Ok(ImpactResult {
            structural_deps,
            temporal_coupling,
            signals,
            change_metrics,
        })
    }

    /// Get basic stats about the stored graph.
    pub fn get_stats(&self) -> Result<DbStats, DbError> {
        let node_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
        let edge_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
        let signal_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM signals", [], |r| r.get(0))?;
        let structural_edges: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE layer = 'structural'",
            [],
            |r| r.get(0),
        )?;
        let change_edges: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE layer = 'change'",
            [],
            |r| r.get(0),
        )?;

        Ok(DbStats {
            node_count: node_count as usize,
            edge_count: edge_count as usize,
            signal_count: signal_count as usize,
            structural_edges: structural_edges as usize,
            change_edges: change_edges as usize,
        })
    }
}

fn map_signal_row(row: &rusqlite::Row<'_>) -> SqlResult<StoredSignal> {
    let details_str: Option<String> = row.get(5)?;
    let details = details_str
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .unwrap_or(None);
    Ok(StoredSignal {
        id: row.get(0)?,
        signal_type: row.get(1)?,
        node_a: row.get(2)?,
        node_b: row.get(3)?,
        severity: row.get(4)?,
        details,
        detected_at: row.get(6)?,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use ising_core::graph::{EdgeType, Node};

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
        graph.add_node(Node::module("a", "a.py"));
        graph.add_node(Node::module("b", "b.py"));
        graph.change_metrics.insert(
            "a".to_string(),
            ChangeMetrics {
                hotspot_score: 0.9,
                ..Default::default()
            },
        );
        graph.change_metrics.insert(
            "b".to_string(),
            ChangeMetrics {
                hotspot_score: 0.3,
                ..Default::default()
            },
        );
        db.store_graph(&graph).unwrap();

        let hotspots = db.get_hotspots(10).unwrap();
        assert_eq!(hotspots.len(), 2);
        assert_eq!(hotspots[0].0, "a");
    }
}
