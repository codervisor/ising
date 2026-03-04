//! Graph model for representing codebase structure.
//!
//! Maps source symbols to a directed graph G = (V, E) where nodes are "spins"
//! representing code symbols and edges represent dependencies between them.

use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

/// A code symbol (function, class, module, etc.) acting as a "spin" in the lattice.
#[derive(Debug, Clone)]
pub struct Symbol {
    /// Fully qualified name of the symbol.
    pub name: String,
    /// File path where the symbol is defined.
    pub file: String,
    /// Kind of symbol (function, class, module, etc.).
    pub kind: SymbolKind,
}

/// Classification of code symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
    Module,
    Variable,
    Interface,
    Other(String),
}

/// The directed dependency graph of a codebase, modeled as a physical lattice.
///
/// Nodes are [`Symbol`]s and edges represent references (usages/dependencies)
/// between symbols.
#[derive(Debug)]
pub struct IsingGraph {
    /// The underlying directed graph.
    pub graph: DiGraph<Symbol, ()>,
    /// Lookup from symbol name to node index.
    index: HashMap<String, NodeIndex>,
}

impl IsingGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index: HashMap::new(),
        }
    }

    /// Add a symbol node to the graph. Returns the node index.
    pub fn add_symbol(&mut self, symbol: Symbol) -> NodeIndex {
        let name = symbol.name.clone();
        let idx = self.graph.add_node(symbol);
        self.index.insert(name, idx);
        idx
    }

    /// Add a directed edge (dependency) between two symbols by name.
    /// Returns `true` if both symbols exist and the edge was added.
    pub fn add_dependency(&mut self, from: &str, to: &str) -> bool {
        if let (Some(&from_idx), Some(&to_idx)) = (self.index.get(from), self.index.get(to)) {
            self.graph.add_edge(from_idx, to_idx, ());
            true
        } else {
            false
        }
    }

    /// Number of symbols (nodes) in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of dependencies (edges) in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Convert the graph to an adjacency matrix.
    pub fn to_adjacency_matrix(&self) -> ndarray::Array2<f64> {
        let n = self.graph.node_count();
        let mut matrix = ndarray::Array2::<f64>::zeros((n, n));

        for edge in self.graph.edge_indices() {
            if let Some((source, target)) = self.graph.edge_endpoints(edge) {
                matrix[[source.index(), target.index()]] = 1.0;
            }
        }

        matrix
    }
}

impl Default for IsingGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_symbol(name: &str) -> Symbol {
        Symbol {
            name: name.to_string(),
            file: "test.rs".to_string(),
            kind: SymbolKind::Function,
        }
    }

    #[test]
    fn test_add_symbol_and_dependency() {
        let mut g = IsingGraph::new();
        g.add_symbol(make_symbol("a"));
        g.add_symbol(make_symbol("b"));
        assert!(g.add_dependency("a", "b"));
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn test_adjacency_matrix() {
        let mut g = IsingGraph::new();
        g.add_symbol(make_symbol("a"));
        g.add_symbol(make_symbol("b"));
        g.add_dependency("a", "b");

        let m = g.to_adjacency_matrix();
        assert_eq!(m[[0, 1]], 1.0);
        assert_eq!(m[[1, 0]], 0.0);
    }
}
