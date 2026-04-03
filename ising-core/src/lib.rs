//! # Ising Core
//!
//! Core types and analysis for the Ising three-layer code graph engine.
//!
//! Provides the unified graph model that combines:
//! - **Layer 1 — Structural Graph**: static dependencies from code (AST)
//! - **Layer 2 — Change Graph**: temporal coupling from git history
//! - **Layer 3 — Defect Graph**: fault propagation from issue tracker + git blame
//!
//! The key innovation is cross-layer signal detection: anomalies that only
//! appear when comparing across layers.

pub mod boundary;
pub mod config;
pub mod error;
pub mod fea;
pub mod graph;
pub mod ignore;
pub mod metrics;
pub mod path_utils;

pub use error::IsingError;
