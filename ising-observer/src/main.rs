//! Ising factory observer — watches the Onsager event spine and emits insights.
//!
//! This is a long-running service that subscribes to factory events from Forge,
//! Stiglab, and Synodic via the Onsager event spine. Incoming events are passed
//! through a set of heuristic analyzers; any insights produced are emitted back
//! to the spine as `InsightRecorded` events under the `ising` namespace.

mod analyzer;
mod emitter;

use anyhow::Result;
use onsager::{EventHandler, EventNotification, EventStore, Listener, Namespace};
use tracing_subscriber::EnvFilter;

use crate::analyzer::AnalyzerSet;
use crate::emitter::InsightEmitter;

struct ObserverHandler {
    analyzers: AnalyzerSet,
    emitter: InsightEmitter,
    store: EventStore,
}

#[async_trait::async_trait]
impl EventHandler for ObserverHandler {
    async fn handle(&self, notification: EventNotification) -> anyhow::Result<()> {
        // Look up the full event record from the store using the notification's
        // stream_id.  We fetch from events_ext since factory events go there.
        let events = self
            .store
            .query_ext_events(Some(&notification.stream_id), None, 1)
            .await?;

        if let Some(event_record) = events.first() {
            // Try to deserialize the data payload as a typed FactoryEventKind.
            if let Ok(factory_event) =
                serde_json::from_value::<onsager::FactoryEventKind>(event_record.data.clone())
            {
                // Run all analyzers against this event.
                let insights = self.analyzers.analyze(&factory_event).await;

                // Emit any insights produced back to the spine.
                for insight in insights {
                    self.emitter.emit_insight(&insight).await?;
                    tracing::info!(
                        insight_id = %insight.insight_id,
                        kind = ?insight.kind,
                        "insight emitted"
                    );
                }
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("ising_observer=info,onsager=info")),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    tracing::info!("Ising observer starting");

    let store = EventStore::connect(&database_url).await?;
    let emitter = InsightEmitter::new(store.clone());
    let analyzers = AnalyzerSet::default();

    let handler = ObserverHandler {
        analyzers,
        emitter,
        store: store.clone(),
    };

    tracing::info!("Listening for factory events on forge, stiglab, synodic, ising namespaces");

    // Subscribe to all relevant factory-event namespaces.
    // The Listener filters on stream_id prefix: events with stream_id like
    // "forge:..." will match the "forge" namespace subscription.
    let listener = Listener::new(store)
        .subscribe(Namespace::new("forge").expect("valid namespace"))
        .subscribe(Namespace::stiglab())
        .subscribe(Namespace::synodic())
        .subscribe(Namespace::ising());

    listener.run(handler).await?;

    Ok(())
}
