//! Finite Element Analysis (FEA) types for code stress analysis.
//!
//! Defines material properties, stress tensors, safety factors, load cases,
//! and stress fields — the data model for treating codebases as physical
//! structures under load.

use serde::{Deserialize, Serialize};

/// Material properties for a code module (analogous to physical material constants).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaterialProperties {
    /// Resistance to change: normalize(complexity) × normalize(CBO).
    pub stiffness: f64,
    /// Capacity before breaking: baseline × (1 + test_coverage_ratio).
    /// Defaults to 0.5 when no coverage data is available.
    pub yield_strength: f64,
    /// Remaining endurance: max_churn_rate / actual_churn_rate.
    pub fatigue_life: f64,
    /// API surface area: fan_in + fan_out.
    pub cross_section: f64,
}

/// Stress tensor for a code module.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StressTensor {
    /// Applied load: change_freq × churn_rate.
    pub change_pressure: f64,
    /// Fan-out strain: module pulled in many directions by consumers.
    pub tensile: f64,
    /// Responsibility overload: high LOC × complexity × CBO under change.
    pub compressive: f64,
    /// Combined scalar: sqrt(tensile² + compressive² - tensile × compressive).
    pub von_mises: f64,
}

/// Safety classification zones based on safety factor value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyZone {
    /// SF < 1.0 — already past yield, failing under current load.
    Critical,
    /// SF 1.0–1.5 — little margin, next change may break it.
    Danger,
    /// SF 1.5–2.0 — caution needed.
    Warning,
    /// SF 2.0–3.0 — good architectural margin.
    Healthy,
    /// SF > 3.0 — more stable than necessary.
    OverEngineered,
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
            SafetyZone::OverEngineered
        }
    }

    /// Human-readable label for display.
    pub fn label(&self) -> &'static str {
        match self {
            SafetyZone::Critical => "CRITICAL",
            SafetyZone::Danger => "DANGER",
            SafetyZone::Warning => "WARNING",
            SafetyZone::Healthy => "HEALTHY",
            SafetyZone::OverEngineered => "OVER-ENG",
        }
    }
}

impl std::fmt::Display for SafetyZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Complete FEA result for a single node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStress {
    pub node_id: String,
    pub file_path: String,
    pub material: MaterialProperties,
    pub stress: StressTensor,
    pub safety_factor: f64,
    pub safety_zone: SafetyZone,
}

/// A complete stress field across the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressField {
    pub nodes: Vec<NodeStress>,
    /// Number of Jacobi iterations to convergence.
    pub iterations: usize,
    /// Whether the propagation converged within max_iterations.
    pub converged: bool,
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

/// Difference in stress for a single node between two stress fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStressDelta {
    pub node_id: String,
    pub file_path: String,
    pub von_mises_before: f64,
    pub von_mises_after: f64,
    pub safety_factor_before: f64,
    pub safety_factor_after: f64,
    pub zone_before: SafetyZone,
    pub zone_after: SafetyZone,
}

/// Comparison between two stress fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressDelta {
    pub deltas: Vec<NodeStressDelta>,
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
        assert_eq!(SafetyZone::from_factor(3.01), SafetyZone::OverEngineered);
        assert_eq!(SafetyZone::from_factor(10.0), SafetyZone::OverEngineered);
    }

    #[test]
    fn test_safety_zone_display() {
        assert_eq!(format!("{}", SafetyZone::Critical), "CRITICAL");
        assert_eq!(format!("{}", SafetyZone::Healthy), "HEALTHY");
    }

    #[test]
    fn test_serde_roundtrip() {
        let ns = NodeStress {
            node_id: "test.py".to_string(),
            file_path: "test.py".to_string(),
            material: MaterialProperties {
                stiffness: 0.5,
                yield_strength: 0.5,
                fatigue_life: 2.0,
                cross_section: 5.0,
            },
            stress: StressTensor {
                change_pressure: 10.0,
                tensile: 3.0,
                compressive: 4.0,
                von_mises: 3.6,
            },
            safety_factor: 0.14,
            safety_zone: SafetyZone::Critical,
        };
        let json = serde_json::to_string(&ns).unwrap();
        let restored: NodeStress = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.node_id, "test.py");
        assert_eq!(restored.safety_zone, SafetyZone::Critical);
    }
}
