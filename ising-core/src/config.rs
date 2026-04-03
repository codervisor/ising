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
    #[serde(default)]
    pub fea: FeaConfig,
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
    /// Max coupling for unnecessary abstraction signal.
    #[serde(
        default = "default_unnecessary_abstraction",
        alias = "over_engineering_coupling"
    )]
    pub unnecessary_abstraction_coupling: f64,
    /// Minimum complexity for god module signal.
    #[serde(default = "default_god_module_complexity")]
    pub god_module_complexity: u32,
    /// Minimum LOC for god module signal.
    #[serde(default = "default_god_module_loc")]
    pub god_module_loc: u32,
    /// Minimum fan-out for god module signal.
    #[serde(default = "default_god_module_fan_out")]
    pub god_module_fan_out: usize,
    /// Minimum LOC for monolith detection (god module without CBO requirement).
    #[serde(default = "default_god_module_monolith_loc")]
    pub god_module_monolith_loc: u32,
    /// Minimum complexity for monolith detection (god module without CBO requirement).
    #[serde(default = "default_god_module_monolith_complexity")]
    pub god_module_monolith_complexity: u32,
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

/// Risk propagation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaConfig {
    /// Damping factor for risk propagation via co-change edges (0.0–1.0).
    #[serde(default = "default_cochange_damping")]
    pub cochange_damping: f64,
    /// Damping factor for risk propagation via structural (import) edges (0.0–1.0).
    #[serde(default = "default_structural_damping")]
    pub structural_damping: f64,
    /// Convergence epsilon for propagation iteration.
    #[serde(default = "default_epsilon")]
    pub epsilon: f64,
    /// Maximum propagation iterations.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    /// Attenuation factor for risk crossing module boundaries (0.0–1.0).
    /// Default 0.3: risk propagates at 30% when crossing a boundary.
    /// Clamped to [0.0, 1.0] to prevent breaking propagation convergence.
    #[serde(default = "default_boundary_attenuation")]
    pub boundary_attenuation: f64,
}

impl Default for FeaConfig {
    fn default() -> Self {
        Self {
            cochange_damping: default_cochange_damping(),
            structural_damping: default_structural_damping(),
            epsilon: default_epsilon(),
            max_iterations: default_max_iterations(),
            boundary_attenuation: default_boundary_attenuation(),
        }
    }
}

fn default_cochange_damping() -> f64 {
    0.3
}
fn default_structural_damping() -> f64 {
    0.15
}
fn default_epsilon() -> f64 {
    0.001
}
fn default_max_iterations() -> usize {
    100
}
fn default_boundary_attenuation() -> f64 {
    0.3
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
    3
}
fn default_min_coupling() -> f64 {
    0.15
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
fn default_unnecessary_abstraction() -> f64 {
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
fn default_god_module_monolith_loc() -> u32 {
    5000
}
fn default_god_module_monolith_complexity() -> u32 {
    200
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
            unnecessary_abstraction_coupling: default_unnecessary_abstraction(),
            god_module_complexity: default_god_module_complexity(),
            god_module_loc: default_god_module_loc(),
            god_module_fan_out: default_god_module_fan_out(),
            god_module_monolith_loc: default_god_module_monolith_loc(),
            god_module_monolith_complexity: default_god_module_monolith_complexity(),
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
        let mut config: Self = toml::from_str(&content)
            .map_err(|e| crate::IsingError::ConfigFile(format!("{path:?}: {e}")))?;
        config.fea.boundary_attenuation = config.fea.boundary_attenuation.clamp(0.0, 1.0);
        Ok(config)
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
        assert_eq!(config.thresholds.min_co_changes, 3);
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
        assert_eq!(config.thresholds.min_coupling, 0.15);
    }

    #[test]
    fn test_legacy_over_engineering_coupling_key() {
        let toml_str = r#"
[thresholds]
over_engineering_coupling = 0.1
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.thresholds.unnecessary_abstraction_coupling, 0.1);
    }
}
