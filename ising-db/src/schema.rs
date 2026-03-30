//! Database schema initialization and management.

use crate::Database;
use crate::DbError;

impl Database {
    pub(crate) fn init_schema(&self) -> Result<(), DbError> {
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

            CREATE TABLE IF NOT EXISTS risk_data (
                node_id TEXT PRIMARY KEY,
                change_load REAL,
                structural_weight REAL,
                propagated_risk REAL,
                risk_score REAL,
                capacity REAL,
                safety_factor REAL,
                zone TEXT,
                direct_score REAL DEFAULT 0.0,
                risk_tier TEXT DEFAULT 'normal',
                percentile REAL DEFAULT 0.0,
                FOREIGN KEY (node_id) REFERENCES nodes(id)
            );

            CREATE TABLE IF NOT EXISTS health_index (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                score REAL,
                grade TEXT,
                active_modules INTEGER,
                total_modules INTEGER,
                critical_count INTEGER,
                high_count INTEGER,
                risk_concentration REAL,
                avg_direct_score REAL
            );

            CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);
            CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);
            CREATE INDEX IF NOT EXISTS idx_edges_layer ON edges(layer);
            CREATE INDEX IF NOT EXISTS idx_signals_type ON signals(signal_type);
            CREATE INDEX IF NOT EXISTS idx_signals_severity ON signals(severity DESC);
            CREATE INDEX IF NOT EXISTS idx_risk_safety ON risk_data(safety_factor ASC);
            CREATE INDEX IF NOT EXISTS idx_risk_direct ON risk_data(direct_score DESC);
            CREATE INDEX IF NOT EXISTS idx_risk_tier ON risk_data(risk_tier);

            -- Migration: drop old stress_data table from previous schema
            DROP TABLE IF EXISTS stress_data;
            ",
        )?;
        Ok(())
    }

    /// Clear all data (for rebuilds).
    pub fn clear(&self) -> Result<(), DbError> {
        self.conn.execute_batch(
            "
            DELETE FROM risk_data;
            DELETE FROM health_index;
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
}
