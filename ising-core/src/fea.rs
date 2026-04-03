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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    /// Top 1% by direct risk — immediate attention needed.
    Critical,
    /// Top 1–5% — elevated risk, monitor closely.
    High,
    /// Top 5–15% — moderate risk.
    Medium,
    /// Bottom 85% — normal.
    #[default]
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
/// Scoring is based on three components:
/// 1. **Zone score** — weighted fraction of modules in each safety zone (primary driver)
/// 2. **Coupling modifier** — λ_max from structural Import graph amplifies/dampens zone impact
/// 3. **Signal penalty** — architectural signals (god modules, cycles, etc.) as a modifier
///
/// The zone score directly measures what fraction of the codebase is in good shape.
/// λ_max determines whether bad zones matter (coupled: failures cascade) or are contained
/// (modular: failures stay local).
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

    // --- Zone fractions (fraction of active modules in each safety zone) ---
    /// Fraction of active modules in Stable zone (SF > 3.0).
    #[serde(default)]
    pub frac_stable: f64,
    /// Fraction of active modules in Healthy zone (SF 2.0-3.0).
    #[serde(default)]
    pub frac_healthy: f64,
    /// Fraction of active modules in Warning zone (SF 1.5-2.0).
    #[serde(default)]
    pub frac_warning: f64,
    /// Fraction of active modules in Danger zone (SF 1.0-1.5).
    #[serde(default)]
    pub frac_danger: f64,
    /// Fraction of active modules in Critical zone (SF < 1.0).
    #[serde(default)]
    pub frac_critical: f64,

    // --- Spectral coupling ---
    /// Spectral radius of structural Import graph (unit weights).
    /// λ < 1.0 = modular (failures local), λ ≥ 1.0 = coupled (failures cascade).
    #[serde(default)]
    pub lambda_max: f64,

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
    /// Zone-based score: weighted average of zone fractions.
    #[serde(default)]
    pub zone_sub_score: f64,
    /// Coupling modifier applied to zone score based on λ_max.
    #[serde(default)]
    pub coupling_modifier: f64,
    /// Signal penalty: reduction from architectural signals.
    #[serde(default)]
    pub signal_penalty: f64,

    // --- Legacy sub-scores (kept for backward compatibility) ---
    /// From avg_direct_score + concentration. Measures change-risk pressure.
    #[serde(default)]
    pub risk_sub_score: f64,
    /// From signal densities. Measures architectural health.
    #[serde(default)]
    pub signal_sub_score: f64,
    /// From cycles + unstable deps. Measures structural entanglement.
    #[serde(default)]
    pub structural_sub_score: f64,

    // --- Boundary health (spec 046) ---
    /// Boundary health score: weighted average containment ratio [0, 1].
    #[serde(default)]
    pub boundary_health_score: f64,

    // --- Expected Loss concentration risk (spec 047, Basel II-inspired) ---
    /// Maximum per-module Expected Loss (direct_score × fan_in_normalized).
    /// Identifies the single highest systemic-risk module.
    #[serde(default)]
    pub max_expected_loss: f64,
    /// Herfindahl-Hirschman Index over Expected Loss distribution [0, 1].
    /// High HHI = risk concentrated in few modules; low = well-diversified.
    #[serde(default)]
    pub el_hhi: f64,
    /// Whether the tail risk cap was triggered (max EL exceeded threshold).
    #[serde(default)]
    pub tail_risk_capped: bool,

    // --- Transparency ---
    /// Caveats about data quality or potential bias in this analysis.
    #[serde(default)]
    pub caveats: Vec<String>,
}

/// Per-module boundary health metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryHealth {
    /// Module identifier.
    /// Usually formatted as `package_id::module_id`; for the package root (`_root`),
    /// current producers emit just `package_id`.
    pub module_id: String,
    /// Number of files in this module.
    pub member_count: usize,
    /// Fraction of this module's change edges that stay within the module [0, 1].
    /// 1.0 = perfect containment, 0.0 = all changes leak out.
    pub containment_ratio: f64,
    /// Cross-boundary structural edges / total structural edges.
    pub coupling_ratio: f64,
    /// Fraction of internal nodes in Critical/Danger zone.
    pub internal_stress: f64,
    /// How much risk originating inside this module propagates out.
    pub risk_export: f64,
    /// How much external risk propagates into this module.
    pub risk_import: f64,
}

/// Aggregate boundary health report across all modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryHealthReport {
    /// Per-module health metrics.
    pub modules: Vec<BoundaryHealth>,
    /// Weighted average containment ratio across all modules.
    pub avg_containment: f64,
    /// Weighted average coupling ratio.
    pub avg_coupling_ratio: f64,
    /// Number of modules with containment < 0.5 (leaky boundaries).
    pub leaky_boundary_count: usize,
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
