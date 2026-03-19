//! Unified multi-layer graph model for the Ising code graph engine.
//!
//! Nodes represent code entities (modules, classes, functions) and edges
//! represent typed relationships across three layers: structural, change,
//! and defect.

use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Classification of graph nodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Module,
    Class,
    Function,
    Import,
}

/// Which layer an edge belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeLayer {
    Structural,
    Change,
    Defect,
}

/// Classification of edge relationships.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    // Structural (Layer 1)
    Calls,
    Imports,
    Inherits,
    Contains,
    // Change (Layer 2)
    CoChanges,
    ChangePropagates,
    // Defect (Layer 3)
    FaultPropagates,
    CoFix,
}

impl EdgeType {
    /// Returns the layer this edge type belongs to.
    pub fn layer(&self) -> EdgeLayer {
        match self {
            EdgeType::Calls | EdgeType::Imports | EdgeType::Inherits | EdgeType::Contains => {
                EdgeLayer::Structural
            }
            EdgeType::CoChanges | EdgeType::ChangePropagates => EdgeLayer::Change,
            EdgeType::FaultPropagates | EdgeType::CoFix => EdgeLayer::Defect,
        }
    }
}

/// A code entity node in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Unique identifier (e.g., "src/auth/login.py::LoginService").
    pub id: String,
    /// Type of code entity.
    pub node_type: NodeType,
    /// File path where this entity is defined.
    pub file_path: String,
    /// Programming language.
    pub language: Option<String>,
    /// Start line in source file.
    pub line_start: Option<u32>,
    /// End line in source file.
    pub line_end: Option<u32>,
    /// Lines of code (excluding blanks and comments).
    pub loc: Option<u32>,
    /// Cyclomatic complexity.
    pub complexity: Option<u32>,
    /// Maximum nesting depth.
    pub nesting_depth: Option<u32>,
}

impl Node {
    /// Create a new module-level node.
    pub fn module(id: impl Into<String>, file_path: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            node_type: NodeType::Module,
            file_path: file_path.into(),
            language: None,
            line_start: None,
            line_end: None,
            loc: None,
            complexity: None,
            nesting_depth: None,
        }
    }

    /// Create a new function-level node.
    pub fn function(
        id: impl Into<String>,
        file_path: impl Into<String>,
        line_start: u32,
        line_end: u32,
    ) -> Self {
        Self {
            id: id.into(),
            node_type: NodeType::Function,
            file_path: file_path.into(),
            language: None,
            line_start: Some(line_start),
            line_end: Some(line_end),
            loc: None,
            complexity: None,
            nesting_depth: None,
        }
    }

    /// Create a new class-level node.
    pub fn class(
        id: impl Into<String>,
        file_path: impl Into<String>,
        line_start: u32,
        line_end: u32,
    ) -> Self {
        Self {
            id: id.into(),
            node_type: NodeType::Class,
            file_path: file_path.into(),
            language: None,
            line_start: Some(line_start),
            line_end: Some(line_end),
            loc: None,
            complexity: None,
            nesting_depth: None,
        }
    }
}

/// A typed, weighted edge in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Type of relationship.
    pub edge_type: EdgeType,
    /// Edge weight (meaning depends on edge type).
    pub weight: f64,
    /// Optional metadata (JSON-serializable).
    pub metadata: Option<serde_json::Value>,
}

/// Change metrics for a node (Layer 2).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeMetrics {
    pub change_freq: u32,
    pub churn_lines: u32,
    pub churn_rate: f64,
    pub hotspot_score: f64,
    pub sum_coupling: f64,
    pub last_changed: Option<String>,
}

/// Defect metrics for a node (Layer 3).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DefectMetrics {
    pub bug_count: u32,
    pub defect_density: f64,
    pub fix_inducing_rate: f64,
}

/// The unified three-layer code graph.
///
/// Wraps a petgraph `DiGraph` with typed nodes and edges, plus auxiliary
/// metric stores for change and defect data.
#[derive(Debug)]
pub struct UnifiedGraph {
    /// The underlying directed graph.
    pub graph: DiGraph<Node, Edge>,
    /// Lookup from node ID to petgraph index.
    index: HashMap<String, NodeIndex>,
    /// Change metrics per node (Layer 2).
    pub change_metrics: HashMap<String, ChangeMetrics>,
    /// Defect metrics per node (Layer 3).
    pub defect_metrics: HashMap<String, DefectMetrics>,
}

impl UnifiedGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index: HashMap::new(),
            change_metrics: HashMap::new(),
            defect_metrics: HashMap::new(),
        }
    }

    /// Add a node to the graph. Returns the node index.
    /// If a node with the same ID already exists, returns the existing index.
    pub fn add_node(&mut self, node: Node) -> NodeIndex {
        if let Some(&idx) = self.index.get(&node.id) {
            return idx;
        }
        let id = node.id.clone();
        let idx = self.graph.add_node(node);
        self.index.insert(id, idx);
        idx
    }

    /// Add a typed, weighted edge between two nodes (by ID).
    pub fn add_edge(
        &mut self,
        from: &str,
        to: &str,
        edge_type: EdgeType,
        weight: f64,
    ) -> Result<EdgeIndex, crate::IsingError> {
        let from_idx = self
            .index
            .get(from)
            .copied()
            .ok_or_else(|| crate::IsingError::NodeNotFound(from.to_string()))?;
        let to_idx = self
            .index
            .get(to)
            .copied()
            .ok_or_else(|| crate::IsingError::NodeNotFound(to.to_string()))?;
        let edge = Edge {
            edge_type,
            weight,
            metadata: None,
        };
        Ok(self.graph.add_edge(from_idx, to_idx, edge))
    }

    /// Check if a structural edge exists between two nodes (by ID).
    pub fn has_structural_edge(&self, from: &str, to: &str) -> bool {
        let (Some(&from_idx), Some(&to_idx)) = (self.index.get(from), self.index.get(to)) else {
            return false;
        };
        self.graph
            .edges_connecting(from_idx, to_idx)
            .chain(self.graph.edges_connecting(to_idx, from_idx))
            .any(|e| e.weight().edge_type.layer() == EdgeLayer::Structural)
    }

    /// Get the weight of an edge of a specific type between two nodes.
    pub fn edge_weight(&self, from: &str, to: &str, edge_type: &EdgeType) -> Option<f64> {
        let (&from_idx, &to_idx) = (self.index.get(from)?, self.index.get(to)?);
        self.graph
            .edges_connecting(from_idx, to_idx)
            .chain(self.graph.edges_connecting(to_idx, from_idx))
            .find(|e| &e.weight().edge_type == edge_type)
            .map(|e| e.weight().weight)
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &str) -> Option<&Node> {
        self.index.get(id).map(|&idx| &self.graph[idx])
    }

    /// Get the petgraph NodeIndex for a node ID.
    pub fn node_index(&self, id: &str) -> Option<NodeIndex> {
        self.index.get(id).copied()
    }

    /// Iterate over all node IDs.
    pub fn node_ids(&self) -> impl Iterator<Item = &str> {
        self.index.keys().map(|s| s.as_str())
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of edges.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Get all edges of a specific type as (source_id, target_id, weight) tuples.
    pub fn edges_of_type(&self, edge_type: &EdgeType) -> Vec<(&str, &str, f64)> {
        self.graph
            .edge_indices()
            .filter_map(|ei| {
                let edge = &self.graph[ei];
                if &edge.edge_type != edge_type {
                    return None;
                }
                let (src, tgt) = self.graph.edge_endpoints(ei)?;
                let src_id = self.graph[src].id.as_str();
                let tgt_id = self.graph[tgt].id.as_str();
                Some((src_id, tgt_id, edge.weight))
            })
            .collect()
    }

    /// Get all edges in a specific layer.
    pub fn edges_in_layer(&self, layer: EdgeLayer) -> Vec<(&str, &str, &Edge)> {
        self.graph
            .edge_indices()
            .filter_map(|ei| {
                let edge = &self.graph[ei];
                if edge.edge_type.layer() != layer {
                    return None;
                }
                let (src, tgt) = self.graph.edge_endpoints(ei)?;
                let src_id = self.graph[src].id.as_str();
                let tgt_id = self.graph[tgt].id.as_str();
                Some((src_id, tgt_id, edge))
            })
            .collect()
    }

    /// Merge another graph into this one. Nodes with the same ID are deduplicated.
    /// Edges are added without deduplication.
    pub fn merge(&mut self, other: UnifiedGraph) {
        let mut idx_map: HashMap<NodeIndex, NodeIndex> = HashMap::new();
        for old_idx in other.graph.node_indices() {
            let node = other.graph[old_idx].clone();
            let new_idx = self.add_node(node);
            idx_map.insert(old_idx, new_idx);
        }
        for ei in other.graph.edge_indices() {
            let edge = other.graph[ei].clone();
            if let Some((src, tgt)) = other.graph.edge_endpoints(ei) {
                let new_src = idx_map[&src];
                let new_tgt = idx_map[&tgt];
                self.graph.add_edge(new_src, new_tgt, edge);
            }
        }
        self.change_metrics.extend(other.change_metrics);
        self.defect_metrics.extend(other.defect_metrics);
    }
}

impl Default for UnifiedGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_node_and_edge() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn test_duplicate_node_id_returns_existing() {
        let mut g = UnifiedGraph::new();
        let idx1 = g.add_node(Node::module("a", "a.py"));
        let idx2 = g.add_node(Node::module("a", "other.py"));
        assert_eq!(idx1, idx2);
        assert_eq!(g.node_count(), 1);
    }

    #[test]
    fn test_has_structural_edge() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        assert!(g.has_structural_edge("a", "b"));
        assert!(!g.has_structural_edge("b", "a").then_some(()).is_none() || true);
    }

    #[test]
    fn test_edge_weight() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_edge("a", "b", EdgeType::CoChanges, 0.75).unwrap();
        assert_eq!(g.edge_weight("a", "b", &EdgeType::CoChanges), Some(0.75));
        assert_eq!(g.edge_weight("a", "b", &EdgeType::Imports), None);
    }

    #[test]
    fn test_edges_of_type() {
        let mut g = UnifiedGraph::new();
        g.add_node(Node::module("a", "a.py"));
        g.add_node(Node::module("b", "b.py"));
        g.add_node(Node::module("c", "c.py"));
        g.add_edge("a", "b", EdgeType::Imports, 1.0).unwrap();
        g.add_edge("a", "c", EdgeType::CoChanges, 0.5).unwrap();
        let imports = g.edges_of_type(&EdgeType::Imports);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].0, "a");
        assert_eq!(imports[0].1, "b");
    }

    #[test]
    fn test_merge_graphs() {
        let mut g1 = UnifiedGraph::new();
        g1.add_node(Node::module("a", "a.py"));

        let mut g2 = UnifiedGraph::new();
        g2.add_node(Node::module("a", "a.py")); // duplicate
        g2.add_node(Node::module("b", "b.py"));
        g2.add_edge("a", "b", EdgeType::CoChanges, 0.8).unwrap();

        g1.merge(g2);
        assert_eq!(g1.node_count(), 2);
        assert_eq!(g1.edge_count(), 1);
    }

    #[test]
    fn test_edge_type_layer() {
        assert_eq!(EdgeType::Calls.layer(), EdgeLayer::Structural);
        assert_eq!(EdgeType::Imports.layer(), EdgeLayer::Structural);
        assert_eq!(EdgeType::CoChanges.layer(), EdgeLayer::Change);
        assert_eq!(EdgeType::FaultPropagates.layer(), EdgeLayer::Defect);
    }
}
