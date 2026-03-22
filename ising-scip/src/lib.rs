//! SCIP index loader for the Ising code graph engine.
//!
//! Converts SCIP (Source Code Intelligence Protocol) protobuf indexes
//! into the Ising unified graph model.
//!
//! # Deprecated
//!
//! This crate is deprecated in favor of Tree-sitter based structural analysis
//! (see spec 019). Tree-sitter provides sufficient signal accuracy without
//! requiring external indexers. This crate will be removed in a future release.
#![deprecated(
    note = "ising-scip is deprecated. Use Tree-sitter structural analysis instead (spec 019)."
)]

use ising_core::graph::{EdgeType, Node, NodeType, UnifiedGraph};
use protobuf::Message;
use scip::types::{self, SymbolRole, symbol_information};
use std::collections::HashSet;
use std::fs::File;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ScipError {
    #[error("failed to read SCIP file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse SCIP protobuf: {0}")]
    Parse(#[from] protobuf::Error),
    #[error("invalid SCIP data: {0}")]
    InvalidData(String),
}

pub struct ScipLoader;

#[derive(Clone)]
struct SourceDef {
    symbol: String,
    range: Range,
}

#[derive(Clone, Copy)]
struct Range {
    start_line: i32,
    start_char: i32,
    end_line: i32,
    end_char: i32,
}

impl Range {
    fn parse(raw: &[i32]) -> Result<Self, ScipError> {
        match raw {
            [start_line, start_char, end_char] => Ok(Self {
                start_line: *start_line,
                start_char: *start_char,
                end_line: *start_line,
                end_char: *end_char,
            }),
            [start_line, start_char, end_line, end_char] => Ok(Self {
                start_line: *start_line,
                start_char: *start_char,
                end_line: *end_line,
                end_char: *end_char,
            }),
            _ => Err(ScipError::InvalidData(format!(
                "occurrence range must have 3 or 4 elements, got {}",
                raw.len()
            ))),
        }
    }

    fn contains(&self, other: &Self) -> bool {
        let start_ok = (other.start_line, other.start_char) >= (self.start_line, self.start_char);
        let end_ok = (other.end_line, other.end_char) <= (self.end_line, self.end_char);
        start_ok && end_ok
    }

    fn span_size(&self) -> (i32, i32) {
        (
            self.end_line - self.start_line,
            self.end_char - self.start_char,
        )
    }
}

impl ScipLoader {
    pub fn load_from_file(path: &Path) -> Result<UnifiedGraph, ScipError> {
        let mut file = File::open(path)?;
        let index = types::Index::parse_from_reader(&mut file)?;
        Self::load_from_index(&index)
    }

    pub fn load_from_index(index: &types::Index) -> Result<UnifiedGraph, ScipError> {
        let mut graph = UnifiedGraph::new();
        let mut known_symbols = HashSet::new();
        let mut source_defs_by_document: Vec<Vec<SourceDef>> =
            Vec::with_capacity(index.documents.len());

        for document in &index.documents {
            for info in &document.symbols {
                if info.symbol.is_empty() {
                    return Err(ScipError::InvalidData(
                        "symbol information has empty symbol".to_string(),
                    ));
                }
                if known_symbols.insert(info.symbol.clone()) {
                    let node_type = map_kind_to_node_type(info.kind.enum_value_or_default());
                    let mut node = Node::module(&info.symbol, &document.relative_path);
                    node.node_type = node_type;
                    graph.add_node(node);
                }
            }

            let mut source_defs = Vec::new();
            for occurrence in &document.occurrences {
                if !has_role(occurrence.symbol_roles, SymbolRole::Definition) {
                    continue;
                }
                if occurrence.symbol.is_empty() {
                    return Err(ScipError::InvalidData(
                        "definition occurrence has empty symbol".to_string(),
                    ));
                }
                let range = Range::parse(&occurrence.range)?;
                source_defs.push(SourceDef {
                    symbol: occurrence.symbol.clone(),
                    range,
                });

                if known_symbols.insert(occurrence.symbol.clone()) {
                    let node = Node::module(&occurrence.symbol, &document.relative_path);
                    graph.add_node(node);
                }
            }
            source_defs_by_document.push(source_defs);
        }

        for (document, source_defs) in index.documents.iter().zip(source_defs_by_document.iter()) {
            for occurrence in &document.occurrences {
                if has_role(occurrence.symbol_roles, SymbolRole::Definition) {
                    continue;
                }
                if occurrence.symbol.is_empty() {
                    return Err(ScipError::InvalidData(
                        "reference occurrence has empty symbol".to_string(),
                    ));
                }

                if !known_symbols.contains(&occurrence.symbol) {
                    continue;
                }

                let range = Range::parse(&occurrence.range)?;
                let from_symbol = source_defs
                    .iter()
                    .filter(|candidate| candidate.range.contains(&range))
                    .min_by_key(|candidate| candidate.range.span_size())
                    .map(|candidate| candidate.symbol.as_str())
                    .ok_or_else(|| {
                        ScipError::InvalidData(format!(
                            "unable to resolve referencing symbol for `{}`",
                            occurrence.symbol
                        ))
                    })?;

                graph
                    .add_edge(from_symbol, &occurrence.symbol, EdgeType::Calls, 1.0)
                    .map_err(|e| ScipError::InvalidData(e.to_string()))?;
            }
        }

        Ok(graph)
    }
}

fn has_role(symbol_roles: i32, role: SymbolRole) -> bool {
    symbol_roles & (role as i32) != 0
}

fn map_kind_to_node_type(kind: symbol_information::Kind) -> NodeType {
    match kind {
        symbol_information::Kind::Function
        | symbol_information::Kind::Method
        | symbol_information::Kind::Macro => NodeType::Function,
        symbol_information::Kind::Class
        | symbol_information::Kind::Enum
        | symbol_information::Kind::Struct
        | symbol_information::Kind::Interface
        | symbol_information::Kind::Trait
        | symbol_information::Kind::Protocol => NodeType::Class,
        symbol_information::Kind::Package
        | symbol_information::Kind::Namespace
        | symbol_information::Kind::Module => NodeType::Module,
        _ => NodeType::Module,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protobuf::EnumOrUnknown;
    use scip::types::{Document, Index, Occurrence, SymbolInformation};
    use tempfile::NamedTempFile;

    fn make_symbol(symbol: &str, kind: symbol_information::Kind) -> SymbolInformation {
        SymbolInformation {
            symbol: symbol.to_string(),
            kind: EnumOrUnknown::new(kind),
            ..Default::default()
        }
    }

    fn def_occ(symbol: &str, range: Vec<i32>) -> Occurrence {
        Occurrence {
            symbol: symbol.to_string(),
            symbol_roles: SymbolRole::Definition as i32,
            range,
            ..Default::default()
        }
    }

    fn ref_occ(symbol: &str, range: Vec<i32>) -> Occurrence {
        Occurrence {
            symbol: symbol.to_string(),
            symbol_roles: SymbolRole::ReadAccess as i32,
            range,
            ..Default::default()
        }
    }

    #[test]
    fn parse_minimal_index_with_reference() {
        let index = Index {
            documents: vec![Document {
                relative_path: "src/lib.rs".to_string(),
                symbols: vec![
                    make_symbol("pkg a", symbol_information::Kind::Function),
                    make_symbol("pkg b", symbol_information::Kind::Function),
                ],
                occurrences: vec![
                    def_occ("pkg a", vec![0, 0, 3, 0]),
                    def_occ("pkg b", vec![4, 0, 6, 0]),
                    ref_occ("pkg b", vec![1, 2, 1, 3]),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let graph = ScipLoader::load_from_index(&index).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn cross_file_reference_creates_edge() {
        let index = Index {
            documents: vec![
                Document {
                    relative_path: "src/a.rs".to_string(),
                    symbols: vec![make_symbol("sym a", symbol_information::Kind::Function)],
                    occurrences: vec![
                        def_occ("sym a", vec![0, 0, 2, 0]),
                        ref_occ("sym b", vec![1, 0, 1, 1]),
                    ],
                    ..Default::default()
                },
                Document {
                    relative_path: "src/b.rs".to_string(),
                    symbols: vec![make_symbol("sym b", symbol_information::Kind::Function)],
                    occurrences: vec![def_occ("sym b", vec![0, 0, 1, 0])],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let graph = ScipLoader::load_from_index(&index).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn empty_index_returns_empty_graph() {
        let graph = ScipLoader::load_from_index(&Index::default()).unwrap();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn load_from_file_roundtrip() {
        let index = Index {
            documents: vec![Document {
                relative_path: "src/lib.rs".to_string(),
                symbols: vec![
                    make_symbol("sym a", symbol_information::Kind::Function),
                    make_symbol("sym b", symbol_information::Kind::Function),
                ],
                occurrences: vec![
                    def_occ("sym a", vec![0, 0, 2, 0]),
                    def_occ("sym b", vec![3, 0, 5, 0]),
                    ref_occ("sym b", vec![1, 0, 1, 1]),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let mut tmp = NamedTempFile::new().unwrap();
        index.write_to_writer(&mut tmp).unwrap();

        let graph = ScipLoader::load_from_file(tmp.path()).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
    }
}
