//! Heuristic analyzers that inspect factory events and produce insights.
//!
//! Each analyzer maintains internal state (sliding windows, counters) behind a
//! `Mutex` for thread safety. The `AnalyzerSet` holds all registered analyzers
//! and runs them on each incoming event.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use onsager::factory_event::{InsightKind, InsightScope, ShapingOutcome, VerdictSummary};
use onsager::protocol::Insight;
use onsager::FactoryEventKind;

/// A single heuristic that inspects a factory event and optionally produces an
/// insight.
#[async_trait]
pub trait Analyzer: Send + Sync {
    async fn analyze(&self, event: &FactoryEventKind) -> Option<Insight>;
}

/// Holds all registered analyzers and runs them on each event.
pub struct AnalyzerSet {
    analyzers: Vec<Box<dyn Analyzer>>,
}

impl Default for AnalyzerSet {
    fn default() -> Self {
        Self {
            analyzers: vec![
                Box::new(FailureRateAnalyzer::new()),
                Box::new(SlowSessionAnalyzer::new(120_000)), // 2 min threshold
                Box::new(GateDenyAnalyzer::new()),
            ],
        }
    }
}

impl AnalyzerSet {
    pub async fn analyze(&self, event: &FactoryEventKind) -> Vec<Insight> {
        let mut insights = Vec::new();
        for analyzer in &self.analyzers {
            if let Some(insight) = analyzer.analyze(event).await {
                insights.push(insight);
            }
        }
        insights
    }
}

// ---------------------------------------------------------------------------
// FailureRateAnalyzer
// ---------------------------------------------------------------------------

/// Tracks the last N `ForgeShapingReturned` events. If more than 40% have
/// `outcome != Completed`, emits an Insight with kind=Failure, scope=Global.
pub struct FailureRateAnalyzer {
    /// Ring buffer of outcomes: true = success, false = failure.
    window: Mutex<VecDeque<bool>>,
    window_size: usize,
    threshold: f64,
}

impl FailureRateAnalyzer {
    pub fn new() -> Self {
        Self {
            window: Mutex::new(VecDeque::new()),
            window_size: 20,
            threshold: 0.4,
        }
    }
}

#[async_trait]
impl Analyzer for FailureRateAnalyzer {
    async fn analyze(&self, event: &FactoryEventKind) -> Option<Insight> {
        if let FactoryEventKind::ForgeShapingReturned { outcome, .. } = event {
            let success = *outcome == ShapingOutcome::Completed;
            let mut window = self.window.lock().unwrap();
            window.push_back(success);
            if window.len() > self.window_size {
                window.pop_front();
            }

            // Need a minimum number of samples before firing.
            if window.len() >= 5 {
                let failure_count = window.iter().filter(|&&s| !s).count();
                let failure_rate = failure_count as f64 / window.len() as f64;

                if failure_rate > self.threshold {
                    let id = short_insight_id();
                    return Some(Insight {
                        insight_id: id,
                        kind: InsightKind::Failure,
                        scope: InsightScope::Global,
                        observation: format!(
                            "High shaping failure rate: {:.0}% over last {} attempts",
                            failure_rate * 100.0,
                            window.len()
                        ),
                        evidence: vec![],
                        suggested_action: None,
                        confidence: failure_rate,
                    });
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// SlowSessionAnalyzer
// ---------------------------------------------------------------------------

/// Watches `SessionCompleted` events. If `duration_ms` exceeds a threshold,
/// emits an Insight with kind=Waste.
pub struct SlowSessionAnalyzer {
    threshold_ms: u64,
}

impl SlowSessionAnalyzer {
    pub fn new(threshold_ms: u64) -> Self {
        Self { threshold_ms }
    }
}

#[async_trait]
impl Analyzer for SlowSessionAnalyzer {
    async fn analyze(&self, event: &FactoryEventKind) -> Option<Insight> {
        if let FactoryEventKind::SessionCompleted {
            session_id,
            duration_ms,
            ..
        } = event
        {
            if *duration_ms > self.threshold_ms {
                let id = short_insight_id();
                return Some(Insight {
                    insight_id: id,
                    kind: InsightKind::Waste,
                    scope: InsightScope::Global,
                    observation: format!(
                        "Slow session {}: took {}ms (threshold: {}ms)",
                        session_id, duration_ms, self.threshold_ms
                    ),
                    evidence: vec![],
                    suggested_action: None,
                    confidence: (*duration_ms as f64 / self.threshold_ms as f64).min(1.0),
                });
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// GateDenyAnalyzer
// ---------------------------------------------------------------------------

/// Watches `ForgeGateVerdict` events with `Deny` verdict. If the same artifact
/// is denied 3 or more times, emits an Insight with kind=Anomaly.
pub struct GateDenyAnalyzer {
    /// Maps artifact_id string -> denial count.
    denials: Mutex<HashMap<String, u32>>,
    threshold: u32,
}

impl GateDenyAnalyzer {
    pub fn new() -> Self {
        Self {
            denials: Mutex::new(HashMap::new()),
            threshold: 3,
        }
    }
}

#[async_trait]
impl Analyzer for GateDenyAnalyzer {
    async fn analyze(&self, event: &FactoryEventKind) -> Option<Insight> {
        if let FactoryEventKind::ForgeGateVerdict {
            artifact_id,
            verdict,
            ..
        } = event
        {
            if *verdict == VerdictSummary::Deny {
                let aid = artifact_id.to_string();
                let mut denials = self.denials.lock().unwrap();
                let count = denials.entry(aid.clone()).or_insert(0);
                *count += 1;

                if *count >= self.threshold {
                    let id = short_insight_id();
                    return Some(Insight {
                        insight_id: id,
                        kind: InsightKind::Anomaly,
                        scope: InsightScope::SpecificArtifact(artifact_id.clone()),
                        observation: format!("Artifact {} denied {} times at gate", aid, count),
                        evidence: vec![],
                        suggested_action: None,
                        confidence: (*count as f64 / 10.0).min(1.0),
                    });
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a short, prefixed insight ID.
fn short_insight_id() -> String {
    let uuid = uuid::Uuid::new_v4();
    format!("ins_{}", &uuid.to_string()[..8])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use onsager::artifact::ArtifactId;
    use onsager::factory_event::GatePoint;

    #[tokio::test]
    async fn failure_rate_below_threshold_no_insight() {
        let analyzer = FailureRateAnalyzer::new();

        // Feed 5 successful events -> no insight
        for _ in 0..5 {
            let event = FactoryEventKind::ForgeShapingReturned {
                request_id: "req_1".into(),
                artifact_id: ArtifactId::new("art_test0001"),
                outcome: ShapingOutcome::Completed,
            };
            let result = analyzer.analyze(&event).await;
            assert!(result.is_none());
        }
    }

    #[tokio::test]
    async fn failure_rate_above_threshold_emits_insight() {
        let analyzer = FailureRateAnalyzer::new();

        // Feed 5 failed events -> should trigger
        for _ in 0..5 {
            let event = FactoryEventKind::ForgeShapingReturned {
                request_id: "req_1".into(),
                artifact_id: ArtifactId::new("art_test0001"),
                outcome: ShapingOutcome::Failed,
            };
            let _ = analyzer.analyze(&event).await;
        }

        // The 5th one should have produced an insight; verify with one more
        let event = FactoryEventKind::ForgeShapingReturned {
            request_id: "req_1".into(),
            artifact_id: ArtifactId::new("art_test0001"),
            outcome: ShapingOutcome::Failed,
        };
        let result = analyzer.analyze(&event).await;
        assert!(result.is_some());
        let insight = result.unwrap();
        assert_eq!(insight.kind, InsightKind::Failure);
        assert_eq!(insight.scope, InsightScope::Global);
        assert!(insight.observation.contains("failure rate"));
        assert!(insight.insight_id.starts_with("ins_"));
    }

    #[tokio::test]
    async fn failure_rate_mixed_below_threshold() {
        let analyzer = FailureRateAnalyzer::new();

        // Feed 4 successes, 1 failure -> 20% failure rate, below 40% threshold
        for _ in 0..4 {
            let event = FactoryEventKind::ForgeShapingReturned {
                request_id: "req_1".into(),
                artifact_id: ArtifactId::new("art_test0001"),
                outcome: ShapingOutcome::Completed,
            };
            let _ = analyzer.analyze(&event).await;
        }
        let event = FactoryEventKind::ForgeShapingReturned {
            request_id: "req_1".into(),
            artifact_id: ArtifactId::new("art_test0001"),
            outcome: ShapingOutcome::Failed,
        };
        let result = analyzer.analyze(&event).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn slow_session_under_threshold_no_insight() {
        let analyzer = SlowSessionAnalyzer::new(120_000);
        let event = FactoryEventKind::SessionCompleted {
            session_id: "sess_1".into(),
            request_id: "req_1".into(),
            duration_ms: 60_000,
        };
        let result = analyzer.analyze(&event).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn slow_session_over_threshold_emits_insight() {
        let analyzer = SlowSessionAnalyzer::new(120_000);
        let event = FactoryEventKind::SessionCompleted {
            session_id: "sess_1".into(),
            request_id: "req_1".into(),
            duration_ms: 180_000,
        };
        let result = analyzer.analyze(&event).await;
        assert!(result.is_some());
        let insight = result.unwrap();
        assert_eq!(insight.kind, InsightKind::Waste);
        assert!(insight.observation.contains("sess_1"));
        assert!(insight.observation.contains("180000ms"));
    }

    #[tokio::test]
    async fn gate_deny_below_threshold_no_insight() {
        let analyzer = GateDenyAnalyzer::new();

        // Deny twice -> under threshold of 3
        for _ in 0..2 {
            let event = FactoryEventKind::ForgeGateVerdict {
                artifact_id: ArtifactId::new("art_deny0001"),
                gate_point: GatePoint::PreDispatch,
                verdict: VerdictSummary::Deny,
            };
            let result = analyzer.analyze(&event).await;
            assert!(result.is_none());
        }
    }

    #[tokio::test]
    async fn gate_deny_at_threshold_emits_insight() {
        let analyzer = GateDenyAnalyzer::new();

        // Deny 3 times -> at threshold
        for _ in 0..2 {
            let event = FactoryEventKind::ForgeGateVerdict {
                artifact_id: ArtifactId::new("art_deny0001"),
                gate_point: GatePoint::PreDispatch,
                verdict: VerdictSummary::Deny,
            };
            let _ = analyzer.analyze(&event).await;
        }
        let event = FactoryEventKind::ForgeGateVerdict {
            artifact_id: ArtifactId::new("art_deny0001"),
            gate_point: GatePoint::PreDispatch,
            verdict: VerdictSummary::Deny,
        };
        let result = analyzer.analyze(&event).await;
        assert!(result.is_some());
        let insight = result.unwrap();
        assert_eq!(insight.kind, InsightKind::Anomaly);
        assert!(insight.observation.contains("art_deny0001"));
        assert!(insight.observation.contains("3 times"));
    }

    #[tokio::test]
    async fn gate_deny_different_artifacts_tracked_separately() {
        let analyzer = GateDenyAnalyzer::new();

        // Deny artifact A twice, artifact B once -> neither should trigger
        for _ in 0..2 {
            let event = FactoryEventKind::ForgeGateVerdict {
                artifact_id: ArtifactId::new("art_aaaa0001"),
                gate_point: GatePoint::PreDispatch,
                verdict: VerdictSummary::Deny,
            };
            let _ = analyzer.analyze(&event).await;
        }
        let event = FactoryEventKind::ForgeGateVerdict {
            artifact_id: ArtifactId::new("art_bbbb0001"),
            gate_point: GatePoint::PreDispatch,
            verdict: VerdictSummary::Deny,
        };
        let result = analyzer.analyze(&event).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn gate_allow_does_not_count() {
        let analyzer = GateDenyAnalyzer::new();

        // 3 Allow verdicts should not trigger
        for _ in 0..3 {
            let event = FactoryEventKind::ForgeGateVerdict {
                artifact_id: ArtifactId::new("art_deny0001"),
                gate_point: GatePoint::PreDispatch,
                verdict: VerdictSummary::Allow,
            };
            let result = analyzer.analyze(&event).await;
            assert!(result.is_none());
        }
    }

    #[tokio::test]
    async fn unrelated_event_ignored_by_all_analyzers() {
        let set = AnalyzerSet::default();
        let event = FactoryEventKind::ForgeIdleTick;
        let insights = set.analyze(&event).await;
        assert!(insights.is_empty());
    }

    #[test]
    fn short_insight_id_has_correct_prefix() {
        let id = short_insight_id();
        assert!(id.starts_with("ins_"));
        assert_eq!(id.len(), 12); // "ins_" (4) + 8 hex chars
    }
}
