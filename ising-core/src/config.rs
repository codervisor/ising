//! Configuration for the Ising analysis engine.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level configuration for Ising.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub thresholds: ThresholdConfig,
    #[serde(default)]
    pub percentiles: PercentileConfig,
}

/// Build configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Time window for git history analysis (e.g., "6 months ago").
    #[serde(default = "default_time_window")]
    pub time_window: String,
    /// Database file path.
    #[serde(default = "default_db_path")]
    pub db_path: String,
    /// Maximum number of commits to analyze (0 = unlimited).
    #[serde(default = "default_max_commits")]
    pub max_commits: u32,
    /// Skip commits that touch more than this many files (noisy bulk changes).
    #[serde(default = "default_max_files_per_commit")]
    pub max_files_per_commit: u32,
}

/// Threshold values for signal detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Minimum number of co-changes before considering a pair.
    #[serde(default = "default_min_co_changes")]
    pub min_co_changes: u32,
    /// Minimum temporal coupling score to create an edge.
    #[serde(default = "default_min_coupling")]
    pub min_coupling: f64,
    /// Coupling threshold for ghost coupling signal.
    #[serde(default = "default_ghost_coupling")]
    pub ghost_coupling_threshold: f64,
    /// Coupling threshold for fragile boundary signal.
    #[serde(default = "default_fragile_coupling")]
    pub fragile_boundary_coupling: f64,
    /// Fault propagation threshold for fragile boundary.
    #[serde(default = "default_fragile_fault")]
    pub fragile_boundary_fault_prop: f64,
    /// Max coupling for over-engineering signal.
    #[serde(default = "default_over_engineering")]
    pub over_engineering_coupling: f64,
    /// Minimum complexity for god module signal.
    #[serde(default = "default_god_module_complexity")]
    pub god_module_complexity: u32,
    /// Minimum LOC for god module signal.
    #[serde(default = "default_god_module_loc")]
    pub god_module_loc: u32,
    /// Minimum fan-out for god module signal.
    #[serde(default = "default_god_module_fan_out")]
    pub god_module_fan_out: usize,
    /// Minimum number of co-changing files for shotgun surgery signal.
    #[serde(default = "default_shotgun_surgery_breadth")]
    pub shotgun_surgery_breadth: usize,
    /// Instability gap for unstable dependency signal (stable - unstable).
    #[serde(default = "default_unstable_dep_gap")]
    pub unstable_dep_gap: f64,
}

/// Percentile thresholds for node-level signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PercentileConfig {
    /// Change frequency percentile for stable core (bottom N%).
    #[serde(default = "default_p10")]
    pub stable_core_freq: u32,
    /// Fan-in percentile for stable core (top N%).
    #[serde(default = "default_p80")]
    pub stable_core_fan_in: u32,
    /// Hotspot percentile for ticking bomb (top N%).
    #[serde(default = "default_p90")]
    pub ticking_bomb_hotspot: u32,
    /// Defect density percentile for ticking bomb (top N%).
    #[serde(default = "default_p90")]
    pub ticking_bomb_defect: u32,
    /// Coupling percentile for ticking bomb (top N%).
    #[serde(default = "default_p80")]
    pub ticking_bomb_coupling: u32,
}

fn default_time_window() -> String {
    "6 months ago".to_string()
}
fn default_db_path() -> String {
    "ising.db".to_string()
}
fn default_max_commits() -> u32 {
    5000
}
fn default_max_files_per_commit() -> u32 {
    50
}
fn default_min_co_changes() -> u32 {
    5
}
fn default_min_coupling() -> f64 {
    0.3
}
fn default_ghost_coupling() -> f64 {
    0.5
}
fn default_fragile_coupling() -> f64 {
    0.3
}
fn default_fragile_fault() -> f64 {
    0.1
}
fn default_over_engineering() -> f64 {
    0.05
}
fn default_god_module_complexity() -> u32 {
    50
}
fn default_god_module_loc() -> u32 {
    500
}
fn default_god_module_fan_out() -> usize {
    15
}
fn default_shotgun_surgery_breadth() -> usize {
    8
}
fn default_unstable_dep_gap() -> f64 {
    0.4
}
fn default_p10() -> u32 {
    10
}
fn default_p80() -> u32 {
    80
}
fn default_p90() -> u32 {
    90
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            time_window: default_time_window(),
            db_path: default_db_path(),
            max_commits: default_max_commits(),
            max_files_per_commit: default_max_files_per_commit(),
        }
    }
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            min_co_changes: default_min_co_changes(),
            min_coupling: default_min_coupling(),
            ghost_coupling_threshold: default_ghost_coupling(),
            fragile_boundary_coupling: default_fragile_coupling(),
            fragile_boundary_fault_prop: default_fragile_fault(),
            over_engineering_coupling: default_over_engineering(),
            god_module_complexity: default_god_module_complexity(),
            god_module_loc: default_god_module_loc(),
            god_module_fan_out: default_god_module_fan_out(),
            shotgun_surgery_breadth: default_shotgun_surgery_breadth(),
            unstable_dep_gap: default_unstable_dep_gap(),
        }
    }
}

impl Default for PercentileConfig {
    fn default() -> Self {
        Self {
            stable_core_freq: default_p10(),
            stable_core_fan_in: default_p80(),
            ticking_bomb_hotspot: default_p90(),
            ticking_bomb_defect: default_p90(),
            ticking_bomb_coupling: default_p80(),
        }
    }
}

impl Config {
    /// Load config from a TOML file, falling back to defaults for missing fields.
    pub fn load(path: &Path) -> Result<Self, crate::IsingError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::IsingError::ConfigFile(format!("{path:?}: {e}")))?;
        toml::from_str(&content)
            .map_err(|e| crate::IsingError::ConfigFile(format!("{path:?}: {e}")))
    }

    /// Load config from a path if it exists, otherwise return defaults.
    pub fn load_or_default(path: &Path) -> Self {
        if path.is_file() {
            Self::load(path).unwrap_or_default()
        } else {
            Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.thresholds.min_co_changes, 5);
        assert_eq!(config.thresholds.ghost_coupling_threshold, 0.5);
        assert_eq!(config.build.time_window, "6 months ago");
    }

    #[test]
    fn test_parse_partial_toml() {
        let toml_str = r#"
[build]
time_window = "3 months ago"

[thresholds]
min_co_changes = 10
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.build.time_window, "3 months ago");
        assert_eq!(config.thresholds.min_co_changes, 10);
        // Defaults for unspecified fields
        assert_eq!(config.thresholds.min_coupling, 0.3);
    }
}
