//! Error types for ising-core.

use thiserror::Error;

/// Errors produced by ising-core operations.
#[derive(Debug, Error, PartialEq)]
pub enum IsingError {
    /// A referenced symbol does not exist in the graph.
    #[error("symbol not found: `{0}`")]
    SymbolNotFound(String),
}
