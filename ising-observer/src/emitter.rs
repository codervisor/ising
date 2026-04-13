//! Emits `InsightRecorded` factory events to the Onsager event spine.

use anyhow::Result;
use onsager::protocol::Insight;
use onsager::{EventMetadata, EventStore, FactoryEventKind};

/// Writes insight events back to the event spine so Forge and other observers
/// can react to them.
pub struct InsightEmitter {
    store: EventStore,
}

impl InsightEmitter {
    pub fn new(store: EventStore) -> Self {
        Self { store }
    }

    /// Serialize the insight as an `InsightRecorded` event and append it to the
    /// `events_ext` table under the `ising` namespace.
    pub async fn emit_insight(&self, insight: &Insight) -> Result<i64> {
        let event = FactoryEventKind::InsightRecorded {
            insight_id: insight.insight_id.clone(),
            kind: insight.kind,
            scope: insight.scope.clone(),
            observation: insight.observation.clone(),
            confidence: insight.confidence,
        };

        let data = serde_json::to_value(&event)?;
        let metadata = EventMetadata {
            actor: "ising-observer".into(),
            ..Default::default()
        };

        let id = self
            .store
            .append_ext(
                &insight.insight_id,
                "ising",
                event.event_type(),
                data,
                &metadata,
                None,
            )
            .await?;

        Ok(id)
    }
}
