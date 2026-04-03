//! CRUD queries for the Ising database.

use crate::{Database, DbError, DbStats, ImpactResult, StoredRisk, StoredSignal};
use ising_core::fea::RiskField;
use ising_core::graph::{ChangeMetrics, DefectMetrics, EdgeType, Node, NodeType, UnifiedGraph};
use rusqlite::{Result as SqlResult, params};

impl Database {
    /// Store a complete UnifiedGraph to the database.
    pub fn store_graph(&self, graph: &UnifiedGraph) -> Result<(), DbError> {
        let tx = self.conn.unchecked_transaction()?;

        // Insert nodes
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO nodes (id, type, file_path, line_start, line_end, language, loc, complexity, nesting_depth, deprecated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                    node.deprecated,
                ])?;
            }
        }

        // Ensure all change_metrics and defect_metrics node_ids exist as nodes
        // (change graph may reference files not in the structural graph)
        {
            let existing_nodes: std::collections::HashSet<String> = graph
                .graph
                .node_indices()
                .map(|idx| graph.graph[idx].id.clone())
                .collect();
            let mut missing_stmt = tx.prepare(
                "INSERT OR IGNORE INTO nodes (id, type, file_path) VALUES (?1, 'module', ?2)",
            )?;
            for node_id in graph.change_metrics.keys() {
                if !existing_nodes.contains(node_id) {
                    missing_stmt.execute(params![node_id, node_id])?;
                }
            }
            for node_id in graph.defect_metrics.keys() {
                if !existing_nodes.contains(node_id) {
                    missing_stmt.execute(params![node_id, node_id])?;
                }
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
                "INSERT OR REPLACE INTO change_metrics (node_id, change_freq, churn_lines, churn_rate, hotspot_score, sum_coupling, last_changed, defect_churn, feature_churn)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
                    cm.defect_churn,
                    cm.feature_churn,
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
        match self.conn.execute(
            "INSERT INTO signals (signal_type, node_a, node_b, severity, details, detected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![signal_type, node_a, node_b, severity, details_str, now],
        ) {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                // Skip signals referencing non-existent nodes (FK violation)
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
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

    /// Resolve a user-provided target string to a node ID.
    /// Tries: exact node ID match, then file_path match, then prefix match.
    fn resolve_node_id(&self, target: &str) -> Result<Option<String>, DbError> {
        // 1. Exact node ID match
        let exact: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM nodes WHERE id = ?1",
                params![target],
                |row| row.get(0),
            )
            .ok();
        if exact.is_some() {
            return Ok(exact);
        }

        // 2. File path match (return first module node for that file)
        let by_path: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM nodes WHERE file_path = ?1 AND type = 'module' LIMIT 1",
                params![target],
                |row| row.get(0),
            )
            .ok();
        if by_path.is_some() {
            return Ok(by_path);
        }

        // 3. Prefix match on node ID (e.g. "src/app.py" matches "src/app.py::AppClass")
        let by_prefix: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM nodes WHERE id LIKE ?1 || '%' AND type = 'module' LIMIT 1",
                params![target],
                |row| row.get(0),
            )
            .ok();
        Ok(by_prefix)
    }

    /// Get impact data for a node: its neighbors and related signals.
    /// Accepts either exact node ID or file path (tries exact match first,
    /// then file_path match, then prefix match on node ID).
    pub fn get_impact(&self, target: &str) -> Result<ImpactResult, DbError> {
        // Resolve target to a node ID: try exact match, then file_path, then prefix
        let node_id = self.resolve_node_id(target)?;
        let Some(node_id) = node_id else {
            return Ok(ImpactResult::default());
        };

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
                "SELECT change_freq, churn_lines, churn_rate, hotspot_score, sum_coupling, last_changed, defect_churn, feature_churn
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
                        defect_churn: row.get::<_, Option<u32>>(6)?.unwrap_or(0),
                        feature_churn: row.get::<_, Option<u32>>(7)?.unwrap_or(0),
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

    /// Store a risk field to the database.
    pub fn store_risk_field(&self, field: &RiskField) -> Result<(), DbError> {
        let tx = self.conn.unchecked_transaction()?;

        // Ensure all risk field node_ids exist in the nodes table
        // (function-level nodes may need to be added)
        {
            let mut ensure_stmt = tx
                .prepare("INSERT OR IGNORE INTO nodes (id, type, file_path) VALUES (?1, ?2, ?3)")?;
            for nr in &field.nodes {
                let node_type = if nr.node_id.contains("::") {
                    "function"
                } else {
                    "module"
                };
                ensure_stmt.execute(params![nr.node_id, node_type, nr.file_path])?;
            }
        }

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO risk_data
                 (node_id, change_load, structural_weight, propagated_risk,
                  risk_score, capacity, safety_factor, zone,
                  direct_score, risk_tier, percentile)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for nr in &field.nodes {
                let zone_str = serde_json::to_value(nr.zone)
                    .ok()
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "unknown".to_string());
                let tier_str = serde_json::to_value(nr.risk_tier)
                    .ok()
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "normal".to_string());
                stmt.execute(params![
                    nr.node_id,
                    nr.change_load,
                    nr.structural_weight,
                    nr.propagated_risk,
                    nr.risk_score,
                    nr.capacity,
                    nr.safety_factor,
                    zone_str,
                    nr.direct_score,
                    tier_str,
                    nr.percentile,
                ])?;
            }
        }

        // Store health index if present
        if let Some(health) = &field.health {
            let caveats_json = serde_json::to_string(&health.caveats).unwrap_or_default();
            tx.execute(
                "INSERT OR REPLACE INTO health_index
                 (id, score, grade, active_modules, total_modules,
                  critical_count, high_count, risk_concentration, avg_direct_score,
                  frac_stable, frac_healthy, frac_warning, frac_danger, frac_critical,
                  lambda_max,
                  signal_density, god_module_density, cycle_density, unstable_dep_density,
                  zone_sub_score, coupling_modifier, signal_penalty,
                  risk_sub_score, signal_sub_score, structural_sub_score,
                  boundary_health_score, caveats)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
                params![
                    health.score,
                    health.grade,
                    health.active_modules,
                    health.total_modules,
                    health.critical_count,
                    health.high_count,
                    health.risk_concentration,
                    health.avg_direct_score,
                    health.frac_stable,
                    health.frac_healthy,
                    health.frac_warning,
                    health.frac_danger,
                    health.frac_critical,
                    health.lambda_max,
                    health.signal_density,
                    health.god_module_density,
                    health.cycle_density,
                    health.unstable_dep_density,
                    health.zone_sub_score,
                    health.coupling_modifier,
                    health.signal_penalty,
                    health.risk_sub_score,
                    health.signal_sub_score,
                    health.structural_sub_score,
                    health.boundary_health_score,
                    caveats_json,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Query risk data ranked by direct score (highest risk first).
    pub fn get_safety_ranking(&self, top_n: usize) -> Result<Vec<StoredRisk>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT node_id, change_load, structural_weight, propagated_risk,
                    risk_score, capacity, safety_factor, zone,
                    direct_score, risk_tier, percentile
             FROM risk_data
             ORDER BY direct_score DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![top_n as i64], map_risk_row)?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    /// Query risk data for a specific node.
    pub fn get_node_risk(&self, node_id: &str) -> Result<Option<StoredRisk>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT node_id, change_load, structural_weight, propagated_risk,
                    risk_score, capacity, safety_factor, zone,
                    direct_score, risk_tier, percentile
             FROM risk_data WHERE node_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![node_id], map_risk_row)?;
        match rows.next() {
            Some(Ok(s)) => Ok(Some(s)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Query nodes by safety zone (legacy) or risk tier.
    pub fn get_nodes_by_zone(&self, zone: &str) -> Result<Vec<StoredRisk>, DbError> {
        // Support both legacy zone names and new tier names
        let mut stmt = self.conn.prepare(
            "SELECT node_id, change_load, structural_weight, propagated_risk,
                    risk_score, capacity, safety_factor, zone,
                    direct_score, risk_tier, percentile
             FROM risk_data WHERE zone = ?1 OR risk_tier = ?1
             ORDER BY direct_score DESC",
        )?;
        let rows = stmt
            .query_map(params![zone], map_risk_row)?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    /// Query the stored health index.
    pub fn get_health(&self) -> Result<Option<crate::StoredHealth>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT score, grade, active_modules, total_modules,
                    critical_count, high_count, risk_concentration, avg_direct_score,
                    frac_stable, frac_healthy, frac_warning, frac_danger, frac_critical,
                    lambda_max,
                    signal_density, god_module_density, cycle_density, unstable_dep_density,
                    zone_sub_score, coupling_modifier, signal_penalty,
                    risk_sub_score, signal_sub_score, structural_sub_score,
                    boundary_health_score, caveats
             FROM health_index WHERE id = 1",
        )?;
        let mut rows = stmt.query_map([], |row| {
            let caveats_str: String = row.get::<_, String>(25).unwrap_or_default();
            let caveats: Vec<String> = serde_json::from_str(&caveats_str).unwrap_or_default();
            Ok(crate::StoredHealth {
                score: row.get(0)?,
                grade: row.get(1)?,
                active_modules: row.get::<_, i64>(2)? as usize,
                total_modules: row.get::<_, i64>(3)? as usize,
                critical_count: row.get::<_, i64>(4)? as usize,
                high_count: row.get::<_, i64>(5)? as usize,
                risk_concentration: row.get(6)?,
                avg_direct_score: row.get(7)?,
                frac_stable: row.get(8)?,
                frac_healthy: row.get(9)?,
                frac_warning: row.get(10)?,
                frac_danger: row.get(11)?,
                frac_critical: row.get(12)?,
                lambda_max: row.get(13)?,
                signal_density: row.get(14)?,
                god_module_density: row.get(15)?,
                cycle_density: row.get(16)?,
                unstable_dep_density: row.get(17)?,
                zone_sub_score: row.get(18)?,
                coupling_modifier: row.get(19)?,
                signal_penalty: row.get(20)?,
                risk_sub_score: row.get(21)?,
                signal_sub_score: row.get(22)?,
                structural_sub_score: row.get(23)?,
                boundary_health_score: row.get(24)?,
                caveats,
            })
        })?;
        match rows.next() {
            Some(Ok(h)) => Ok(Some(h)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Reconstruct a UnifiedGraph from the database.
    pub fn load_graph(&self) -> Result<UnifiedGraph, DbError> {
        let mut graph = UnifiedGraph::new();

        // Load nodes
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, type, file_path, line_start, line_end, language, loc, complexity, nesting_depth, deprecated
                 FROM nodes",
            )?;
            let rows = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let type_str: String = row.get(1)?;
                let file_path: String = row.get(2)?;
                let node_type = match type_str.as_str() {
                    "module" => NodeType::Module,
                    "class" => NodeType::Class,
                    "function" => NodeType::Function,
                    _ => NodeType::Import,
                };
                let deprecated: bool = row.get::<_, Option<bool>>(9)?.unwrap_or(false);
                Ok(Node {
                    id,
                    node_type,
                    file_path,
                    language: row.get(5)?,
                    line_start: row.get(3)?,
                    line_end: row.get(4)?,
                    loc: row.get(6)?,
                    complexity: row.get(7)?,
                    nesting_depth: row.get(8)?,
                    deprecated,
                })
            })?;
            for node in rows {
                graph.add_node(node?);
            }
        }

        // Load edges
        {
            let mut stmt = self
                .conn
                .prepare("SELECT source, target, edge_type, weight FROM edges")?;
            let rows = stmt.query_map([], |row| {
                let source: String = row.get(0)?;
                let target: String = row.get(1)?;
                let edge_type_str: String = row.get(2)?;
                let weight: f64 = row.get(3)?;
                Ok((source, target, edge_type_str, weight))
            })?;
            for row in rows {
                let (source, target, edge_type_str, weight) = row?;
                let edge_type = match edge_type_str.as_str() {
                    "calls" => EdgeType::Calls,
                    "imports" => EdgeType::Imports,
                    "inherits" => EdgeType::Inherits,
                    "contains" => EdgeType::Contains,
                    "co_changes" => EdgeType::CoChanges,
                    "change_propagates" => EdgeType::ChangePropagates,
                    "fault_propagates" => EdgeType::FaultPropagates,
                    "co_fix" => EdgeType::CoFix,
                    _ => continue,
                };
                let _ = graph.add_edge(&source, &target, edge_type, weight);
            }
        }

        // Load change metrics
        {
            let mut stmt = self.conn.prepare(
                "SELECT node_id, change_freq, churn_lines, churn_rate, hotspot_score, sum_coupling, last_changed, defect_churn, feature_churn
                 FROM change_metrics",
            )?;
            let rows = stmt.query_map([], |row| {
                let node_id: String = row.get(0)?;
                Ok((
                    node_id,
                    ChangeMetrics {
                        change_freq: row.get(1)?,
                        churn_lines: row.get(2)?,
                        churn_rate: row.get(3)?,
                        hotspot_score: row.get(4)?,
                        sum_coupling: row.get(5)?,
                        last_changed: row.get(6)?,
                        defect_churn: row.get::<_, Option<u32>>(7)?.unwrap_or(0),
                        feature_churn: row.get::<_, Option<u32>>(8)?.unwrap_or(0),
                    },
                ))
            })?;
            for row in rows {
                let (node_id, cm) = row?;
                graph.change_metrics.insert(node_id, cm);
            }
        }

        // Load defect metrics
        {
            let mut stmt = self.conn.prepare(
                "SELECT node_id, bug_count, defect_density, fix_inducing_rate FROM defect_metrics",
            )?;
            let rows = stmt.query_map([], |row| {
                let node_id: String = row.get(0)?;
                Ok((
                    node_id,
                    DefectMetrics {
                        bug_count: row.get(1)?,
                        defect_density: row.get(2)?,
                        fix_inducing_rate: row.get(3)?,
                    },
                ))
            })?;
            for row in rows {
                let (node_id, dm) = row?;
                graph.defect_metrics.insert(node_id, dm);
            }
        }

        Ok(graph)
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

fn map_risk_row(row: &rusqlite::Row<'_>) -> SqlResult<StoredRisk> {
    Ok(StoredRisk {
        node_id: row.get(0)?,
        change_load: row.get(1)?,
        structural_weight: row.get(2)?,
        propagated_risk: row.get(3)?,
        risk_score: row.get(4)?,
        capacity: row.get(5)?,
        safety_factor: row.get(6)?,
        zone: row.get(7)?,
        direct_score: row.get(8)?,
        risk_tier: row.get(9)?,
        percentile: row.get(10)?,
    })
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
