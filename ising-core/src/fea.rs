//! Risk analysis types for codebase structural assessment.
//!
//! Defines risk scores, capacity, safety factors, load cases, and risk fields.
//! Uses auto-calibrated percentile-based risk tiers alongside legacy safety zones.

use serde::{Deserialize, Serialize};

/// Safety classification zones based on safety factor value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyZone {
    /// SF < 1.0 — risk exceeds capacity.
    Critical,
    /// SF 1.0–1.5 — little margin, next change may break it.
    Danger,
    /// SF 1.5–2.0 — caution needed.
    Warning,
    /// SF 2.0–3.0 — good margin.
    Healthy,
    /// SF > 3.0 — low risk, stable module.
    Stable,
}

impl SafetyZone {
    /// Classify a safety factor value into a zone.
    pub fn from_factor(sf: f64) -> Self {
        if sf < 1.0 {
            SafetyZone::Critical
        } else if sf < 1.5 {
            SafetyZone::Danger
        } else if sf < 2.0 {
            SafetyZone::Warning
        } else if sf <= 3.0 {
            SafetyZone::Healthy
        } else {
            SafetyZone::Stable
        }
    }

    /// Human-readable label for display.
    pub fn label(&self) -> &'static str {
        match self {
            SafetyZone::Critical => "CRITICAL",
            SafetyZone::Danger => "DANGER",
            SafetyZone::Warning => "WARNING",
            SafetyZone::Healthy => "HEALTHY",
            SafetyZone::Stable => "STABLE",
        }
    }
}

impl std::fmt::Display for SafetyZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Auto-calibrated risk tier based on percentile of direct risk score.
///
/// Unlike SafetyZone (which uses hard-coded thresholds that over-classify in dense graphs),
/// RiskTier is derived from the distribution of `direct_score = change_load / capacity`
/// within each specific graph. This makes it self-calibrating across languages, architectures,
/// and graph densities — like auto-exposure in a camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    /// Top 1% by direct risk — immediate attention needed.
    Critical,
    /// Top 1–5% — elevated risk, monitor closely.
    High,
    /// Top 5–15% — moderate risk.
    Medium,
    /// Bottom 85% — normal.
    Normal,
}

impl RiskTier {
    /// Human-readable label for display.
    pub fn label(&self) -> &'static str {
        match self {
            RiskTier::Critical => "CRITICAL",
            RiskTier::High => "HIGH",
            RiskTier::Medium => "MEDIUM",
            RiskTier::Normal => "NORMAL",
        }
    }
}

impl std::fmt::Display for RiskTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Risk assessment for a single code module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRisk {
    pub node_id: String,
    pub file_path: String,
    /// How much change pressure this module faces [0, 1+].
    pub change_load: f64,
    /// Structural weight: combined LOC, complexity, coupling score [0, 1].
    pub structural_weight: f64,
    /// Risk received from neighbors through propagation.
    pub propagated_risk: f64,
    /// Total risk: change_load + propagated_risk.
    pub risk_score: f64,
    /// Module's resilience to change [0.05, 1.0].
    pub capacity: f64,
    /// capacity / risk_score. High = safe, low = danger.
    pub safety_factor: f64,
    /// Legacy classification zone (hard-coded thresholds).
    pub zone: SafetyZone,
    /// Direct risk: change_load / capacity. Measures local risk without propagation.
    #[serde(default)]
    pub direct_score: f64,
    /// Auto-calibrated risk tier based on percentile of direct_score.
    #[serde(default)]
    pub risk_tier: RiskTier,
    /// Percentile rank within the graph (100 = highest risk, 0 = lowest).
    #[serde(default)]
    pub percentile: f64,
}

impl Default for RiskTier {
    fn default() -> Self {
        RiskTier::Normal
    }
}

/// A complete risk field across the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskField {
    pub nodes: Vec<NodeRisk>,
    /// Number of propagation iterations to convergence.
    pub iterations: usize,
    /// Whether propagation converged within max_iterations.
    pub converged: bool,
    /// Aggregate health index for the repository.
    #[serde(default)]
    pub health: Option<HealthIndex>,
}

/// Aggregate health index for a repository.
///
/// A composite score derived from three sub-scores:
/// 1. **Risk sub-score** — avg direct risk + concentration (the original formula, amplified)
/// 2. **Signal sub-score** — density of architectural signals (god modules, cycles, etc.)
/// 3. **Structural sub-score** — entanglement from cycles + unstable dependencies
///
/// Decomposing into sub-scores prevents bias by making it transparent what drives
/// the grade. Users can see whether a low grade comes from change risk, architectural
/// signals, or structural entanglement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthIndex {
    /// Overall health score [0.0, 1.0]. Higher = healthier.
    pub score: f64,
    /// Human-readable grade (A/B/C/D/F).
    pub grade: String,
    /// Number of modules actively changed in the time window.
    pub active_modules: usize,
    /// Total modules in the graph.
    pub total_modules: usize,
    /// Number of modules in the critical tier (top 1%).
    pub critical_count: usize,
    /// Number of modules in the high tier (top 1-5%).
    pub high_count: usize,
    /// Concentration: what fraction of total risk is in the top 10% of modules.
    /// High concentration (>0.8) = risk is localized (good). Low (<0.5) = systemic (bad).
    pub risk_concentration: f64,
    /// Average direct score across active modules.
    pub avg_direct_score: f64,

    // --- Signal density metrics (per-module, for cross-repo comparability) ---
    /// Total signals / total_modules. Higher = more architectural issues per module.
    #[serde(default)]
    pub signal_density: f64,
    /// God module count / total_modules.
    #[serde(default)]
    pub god_module_density: f64,
    /// Dependency cycle signal count / total_modules.
    #[serde(default)]
    pub cycle_density: f64,
    /// Unstable dependency signal count / total_modules.
    #[serde(default)]
    pub unstable_dep_density: f64,

    // --- Sub-scores for transparency [0.0, 1.0] each ---
    /// From avg_direct_score + concentration. Measures change-risk pressure.
    #[serde(default)]
    pub risk_sub_score: f64,
    /// From signal densities. Measures architectural health.
    #[serde(default)]
    pub signal_sub_score: f64,
    /// From cycles + unstable deps. Measures structural entanglement.
    #[serde(default)]
    pub structural_sub_score: f64,

    // --- Transparency ---
    /// Caveats about data quality or potential bias in this analysis.
    #[serde(default)]
    pub caveats: Vec<String>,
}

/// A single load point in a load case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadPoint {
    pub node_id: String,
    pub pressure: f64,
}

/// A load case: a set of hypothetical change pressures applied to nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadCase {
    pub name: String,
    pub loads: Vec<LoadPoint>,
}

/// Difference in risk for a single node between two risk fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRiskDelta {
    pub node_id: String,
    pub file_path: String,
    pub risk_before: f64,
    pub risk_after: f64,
    pub safety_factor_before: f64,
    pub safety_factor_after: f64,
    pub zone_before: SafetyZone,
    pub zone_after: SafetyZone,
}

/// Comparison between two risk fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDelta {
    pub deltas: Vec<NodeRiskDelta>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safety_zone_critical() {
        assert_eq!(SafetyZone::from_factor(0.0), SafetyZone::Critical);
        assert_eq!(SafetyZone::from_factor(0.5), SafetyZone::Critical);
        assert_eq!(SafetyZone::from_factor(0.99), SafetyZone::Critical);
    }

    #[test]
    fn test_safety_zone_danger() {
        assert_eq!(SafetyZone::from_factor(1.0), SafetyZone::Danger);
        assert_eq!(SafetyZone::from_factor(1.2), SafetyZone::Danger);
        assert_eq!(SafetyZone::from_factor(1.49), SafetyZone::Danger);
    }

    #[test]
    fn test_safety_zone_warning() {
        assert_eq!(SafetyZone::from_factor(1.5), SafetyZone::Warning);
        assert_eq!(SafetyZone::from_factor(1.99), SafetyZone::Warning);
    }

    #[test]
    fn test_safety_zone_healthy() {
        assert_eq!(SafetyZone::from_factor(2.0), SafetyZone::Healthy);
        assert_eq!(SafetyZone::from_factor(2.5), SafetyZone::Healthy);
        assert_eq!(SafetyZone::from_factor(3.0), SafetyZone::Healthy);
    }

    #[test]
    fn test_safety_zone_over_engineered() {
        assert_eq!(SafetyZone::from_factor(3.01), SafetyZone::Stable);
        assert_eq!(SafetyZone::from_factor(10.0), SafetyZone::Stable);
    }

    #[test]
    fn test_safety_zone_display() {
        assert_eq!(format!("{}", SafetyZone::Critical), "CRITICAL");
        assert_eq!(format!("{}", SafetyZone::Healthy), "HEALTHY");
        assert_eq!(format!("{}", SafetyZone::Stable), "STABLE");
    }

    #[test]
    fn test_serde_roundtrip() {
        let nr = NodeRisk {
            node_id: "test.py".to_string(),
            file_path: "test.py".to_string(),
            change_load: 0.8,
            structural_weight: 0.5,
            propagated_risk: 0.1,
            risk_score: 0.9,
            capacity: 0.3,
            safety_factor: 0.33,
            zone: SafetyZone::Critical,
            direct_score: 2.67,
            risk_tier: RiskTier::Critical,
            percentile: 99.5,
        };
        let json = serde_json::to_string(&nr).unwrap();
        let restored: NodeRisk = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.node_id, "test.py");
        assert_eq!(restored.zone, SafetyZone::Critical);
        assert_eq!(restored.risk_tier, RiskTier::Critical);
        assert!((restored.direct_score - 2.67).abs() < 0.01);
    }

    #[test]
    fn test_risk_tier_display() {
        assert_eq!(format!("{}", RiskTier::Critical), "CRITICAL");
        assert_eq!(format!("{}", RiskTier::High), "HIGH");
        assert_eq!(format!("{}", RiskTier::Medium), "MEDIUM");
        assert_eq!(format!("{}", RiskTier::Normal), "NORMAL");
    }

    #[test]
    fn test_risk_tier_default() {
        assert_eq!(RiskTier::default(), RiskTier::Normal);
    }
}
