//! Risk analysis types for codebase structural assessment.
//!
//! Defines risk scores, capacity, safety factors, load cases, and risk fields.
//! Replaces the earlier FEA-themed types with honest, direct metrics.

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
    /// Classification zone.
    pub zone: SafetyZone,
}

/// A complete risk field across the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskField {
    pub nodes: Vec<NodeRisk>,
    /// Number of propagation iterations to convergence.
    pub iterations: usize,
    /// Whether propagation converged within max_iterations.
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
        };
        let json = serde_json::to_string(&nr).unwrap();
        let restored: NodeRisk = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.node_id, "test.py");
        assert_eq!(restored.zone, SafetyZone::Critical);
    }
}
