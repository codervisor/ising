//! Cross-layer signal detection for the Ising code graph engine.
//!
//! Detects anomalies that only appear when comparing across the structural,
//! change, and defect graph layers.

pub mod boundary_health;
pub mod hotspots;
pub mod signals;
pub mod stress;
