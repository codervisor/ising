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

    /// Query hotspots ranked by normalized(change_freq) × normalized(complexity).
    /// Falls back to normalized(change_freq) when complexity is unavailable.
    pub fn get_hotspots(&self, top_n: usize) -> Result<Vec<(String, f64, u32, f64)>, DbError> {
        let mut stmt = self.conn.prepare(
            "WITH maxvals AS (
                SELECT
                    MAX(cm.change_freq) as max_freq,
                    MAX(n.complexity) as max_complexity
                FROM nodes n
                LEFT JOIN change_metrics cm ON n.id = cm.node_id
                WHERE cm.change_freq > 0
            )
            SELECT
                n.id,
                (CAST(cm.change_freq AS REAL) / m.max_freq)
                    * (CAST(COALESCE(n.complexity, 1) AS REAL) / m.max_complexity) as score,
                COALESCE(n.complexity, 0),
                COALESCE(cm.change_freq, 0)
            FROM nodes n
            LEFT JOIN change_metrics cm ON n.id = cm.node_id
            CROSS JOIN maxvals m
            WHERE cm.change_freq > 0
            ORDER BY score DESC
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

    /// Export full graph state as viz-json for the SPA.
    pub fn get_viz_export(&self) -> Result<VizExport, DbError> {
        // Metadata
        let repo = self.get_build_info("repo_path")?.unwrap_or_default();
        let commit = self.get_build_info("commit")?.unwrap_or_default();
        let built_at = self.get_build_info("last_build")?.unwrap_or_default();
        let time_window = self.get_build_info("time_window")?.unwrap_or_default();

        // Compute fan-in/fan-out from edges
        let mut fan_in: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut fan_out: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT source, target FROM edges WHERE layer = 'structural'")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (src, tgt) = row?;
                *fan_out.entry(src).or_default() += 1;
                *fan_in.entry(tgt).or_default() += 1;
            }
        }

        // Derive module from file path: strip common prefix, take first dir component
        let all_paths: Vec<String> = {
            let mut stmt = self.conn.prepare("SELECT file_path FROM nodes")?;
            stmt.query_map([], |row| row.get::<_, String>(0))?
                .collect::<SqlResult<Vec<_>>>()?
        };
        let common_prefix = find_common_prefix(&all_paths);

        // Nodes
        let nodes: Vec<VizNode> = {
            let mut stmt = self.conn.prepare(
                "SELECT n.id, n.type, n.file_path, n.language, n.loc, n.complexity, n.nesting_depth,
                        COALESCE(cm.change_freq, 0), COALESCE(cm.churn_rate, 0.0),
                        COALESCE(cm.hotspot_score, 0.0), COALESCE(cm.sum_coupling, 0.0),
                        cm.last_changed,
                        COALESCE(dm.bug_count, 0), COALESCE(dm.defect_density, 0.0),
                        COALESCE(dm.fix_inducing_rate, 0.0)
                 FROM nodes n
                 LEFT JOIN change_metrics cm ON n.id = cm.node_id
                 LEFT JOIN defect_metrics dm ON n.id = dm.node_id",
            )?;
            stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let file_path: String = row.get(2)?;
                let module = derive_module(&file_path, &common_prefix);
                Ok(VizNode {
                    id: id.clone(),
                    node_type: row.get(1)?,
                    module,
                    language: row.get(3)?,
                    loc: row.get::<_, Option<u32>>(4)?.unwrap_or(0),
                    complexity: row.get::<_, Option<u32>>(5)?.unwrap_or(0),
                    nesting_depth: row.get::<_, Option<u32>>(6)?.unwrap_or(0),
                    fan_in: fan_in.get(&id).copied().unwrap_or(0),
                    fan_out: fan_out.get(&id).copied().unwrap_or(0),
                    change_freq: row.get(7)?,
                    churn_rate: row.get(8)?,
                    hotspot: row.get(9)?,
                    sum_coupling: row.get(10)?,
                    last_changed: row.get(11)?,
                    bug_count: row.get(12)?,
                    defect_density: row.get(13)?,
                    fix_inducing_rate: row.get(14)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?
        };

        // Edges
        let edges: Vec<VizEdge> = {
            let mut stmt = self.conn.prepare(
                "SELECT source, target, layer, edge_type, weight, metadata FROM edges",
            )?;
            stmt.query_map([], |row| {
                let metadata_str: Option<String> = row.get(5)?;
                let metadata = metadata_str
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok());
                Ok(VizEdge {
                    source: row.get(0)?,
                    target: row.get(1)?,
                    layer: row.get(2)?,
                    edge_type: row.get(3)?,
                    weight: row.get(4)?,
                    metadata,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?
        };

        // Signals
        let signals: Vec<VizSignal> = {
            let mut stmt = self.conn.prepare(
                "SELECT signal_type, node_a, node_b, severity, details FROM signals ORDER BY severity DESC",
            )?;
            stmt.query_map([], |row| {
                let details_str: Option<String> = row.get(4)?;
                let details: Option<serde_json::Value> = details_str
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok());
                let detail = details
                    .as_ref()
                    .and_then(|d| d.get("description").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();
                Ok(VizSignal {
                    signal_type: row.get(0)?,
                    node_a: row.get(1)?,
                    node_b: row.get(2)?,
                    severity: row.get(3)?,
                    detail,
                    evidence: details,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?
        };

        let file_count = nodes.len();
        let signal_count = signals.len();

        Ok(VizExport {
            meta: VizMeta {
                repo,
                commit,
                built_at,
                time_window,
                file_count,
                signal_count,
            },
            nodes,
            edges,
            signals,
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

/// A node in the viz-json export.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VizNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub module: String,
    pub language: Option<String>,
    pub loc: u32,
    pub complexity: u32,
    pub nesting_depth: u32,
    pub fan_in: u32,
    pub fan_out: u32,
    pub change_freq: u32,
    pub churn_rate: f64,
    pub hotspot: f64,
    pub bug_count: u32,
    pub fix_inducing_rate: f64,
    pub defect_density: f64,
    pub sum_coupling: f64,
    pub last_changed: Option<String>,
}

/// An edge in the viz-json export.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VizEdge {
    pub source: String,
    pub target: String,
    pub layer: String,
    #[serde(rename = "type")]
    pub edge_type: String,
    pub weight: f64,
    pub metadata: Option<serde_json::Value>,
}

/// A signal in the viz-json export.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VizSignal {
    #[serde(rename = "type")]
    pub signal_type: String,
    pub node_a: String,
    pub node_b: Option<String>,
    pub severity: f64,
    pub detail: String,
    pub evidence: Option<serde_json::Value>,
}

/// Metadata in the viz-json export.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VizMeta {
    pub repo: String,
    pub commit: String,
    pub built_at: String,
    pub time_window: String,
    pub file_count: usize,
    pub signal_count: usize,
}

/// Complete viz-json export.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VizExport {
    pub meta: VizMeta,
    pub nodes: Vec<VizNode>,
    pub edges: Vec<VizEdge>,
    pub signals: Vec<VizSignal>,
}

/// Find the longest common directory prefix among file paths.
fn find_common_prefix(paths: &[String]) -> String {
    if paths.is_empty() {
        return String::new();
    }
    let first = &paths[0];
    let parts: Vec<&str> = first.split('/').collect();
    let mut prefix_len = 0;
    for i in 0..parts.len().saturating_sub(1) {
        let candidate = &parts[..=i];
        let prefix = candidate.join("/");
        if paths.iter().all(|p| p.starts_with(&prefix) && p.as_bytes().get(prefix.len()) == Some(&b'/')) {
            prefix_len = i + 1;
        } else {
            break;
        }
    }
    if prefix_len == 0 {
        String::new()
    } else {
        parts[..prefix_len].join("/")
    }
}

/// Derive module name from a file path by stripping common prefix and taking first dir component.
fn derive_module(file_path: &str, common_prefix: &str) -> String {
    let stripped = if !common_prefix.is_empty() && file_path.starts_with(common_prefix) {
        &file_path[common_prefix.len()..].trim_start_matches('/')
    } else {
        file_path
    };
    match stripped.split('/').next() {
        Some(first) if stripped.contains('/') => first.to_string(),
        _ => "root".to_string(),
    }
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
