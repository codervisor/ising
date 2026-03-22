//! Database schema initialization and management.

use crate::DbError;
use crate::Database;

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
}
