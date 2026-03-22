//! Visualization export for the Ising database.

use crate::{Database, DbError};
use rusqlite::Result as SqlResult;

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

impl Database {
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
            let mut stmt = self
                .conn
                .prepare("SELECT source, target, layer, edge_type, weight, metadata FROM edges")?;
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
        if paths
            .iter()
            .all(|p| p.starts_with(&prefix) && p.as_bytes().get(prefix.len()) == Some(&b'/'))
        {
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
        file_path[common_prefix.len()..].trim_start_matches('/')
    } else {
        file_path
    };
    match stripped.split('/').next() {
        Some(first) if stripped.contains('/') => first.to_string(),
        _ => "root".to_string(),
    }
}
