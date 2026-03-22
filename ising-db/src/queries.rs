//! CRUD queries for the Ising database.

use crate::{Database, DbError, DbStats, ImpactResult, StoredSignal};
use ising_core::graph::{ChangeMetrics, NodeType, UnifiedGraph};
use rusqlite::{Result as SqlResult, params};

impl Database {
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
                        .map(serde_json::to_string)
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
                    node_id,
                    dm.bug_count,
                    dm.defect_density,
                    dm.fix_inducing_rate,
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
        let details_str = details.map(serde_json::to_string).transpose()?;
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
