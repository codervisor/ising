//! MCP server for the Ising code graph analysis engine.
//!
//! Exposes `ising_impact` and `ising_signals` as MCP tools via an HTTP/SSE
//! transport, enabling AI coding agents (Claude Code, Cursor, etc.) to query
//! the code graph before making changes.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use ising_db::Database;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Shared server state.
struct AppState {
    db: Mutex<Database>,
}

/// Query params for the impact endpoint.
#[derive(Deserialize)]
struct ImpactQuery {
    target: String,
}

/// Query params for the signals endpoint.
#[derive(Deserialize)]
struct SignalsQuery {
    #[serde(rename = "type")]
    signal_type: Option<String>,
    min_severity: Option<f64>,
}

/// MCP tool listing response.
#[derive(Serialize)]
struct ToolList {
    tools: Vec<ToolDefinition>,
}

#[derive(Serialize)]
struct ToolDefinition {
    name: String,
    description: String,
}

/// Start the MCP server.
pub async fn serve(db_path: &str, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::open(db_path)?;
    let state = Arc::new(AppState { db: Mutex::new(db) });

    let app = Router::new()
        .route("/tools", get(list_tools))
        .route("/impact", get(impact_handler))
        .route("/signals", get(signals_handler))
        .route("/health", get(health_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("MCP server listening on port {port}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn list_tools() -> Json<ToolList> {
    Json(ToolList {
        tools: vec![
            ToolDefinition {
                name: "ising_impact".to_string(),
                description: "Get blast radius, dependencies, and risk signals for a file or function before making changes".to_string(),
            },
            ToolDefinition {
                name: "ising_signals".to_string(),
                description: "Get active risk signals, optionally filtered by type or severity".to_string(),
            },
        ],
    })
}

async fn impact_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ImpactQuery>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let db = state
        .db
        .lock()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let impact = db
        .get_impact(&query.target)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let json =
        serde_json::to_value(&impact).map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json))
}

async fn signals_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SignalsQuery>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let db = state
        .db
        .lock()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let signals = db
        .get_signals(query.signal_type.as_deref(), query.min_severity)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let json = serde_json::to_value(&signals)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json))
}

async fn health_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let db = state
        .db
        .lock()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let stats = db
        .get_stats()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let json =
        serde_json::to_value(&stats).map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json))
}
