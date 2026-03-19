//! Error types for ising-core.

/// Errors produced by ising-core operations.
#[derive(Debug, thiserror::Error)]
pub enum IsingError {
    /// A referenced node does not exist in the graph.
    #[error("node not found: `{0}`")]
    NodeNotFound(String),

    /// A referenced edge does not exist.
    #[error("edge not found: `{from}` -> `{to}`")]
    EdgeNotFound { from: String, to: String },

    /// Invalid configuration value.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    /// Config file I/O or parse error.
    #[error("config file error: {0}")]
    ConfigFile(String),
}
