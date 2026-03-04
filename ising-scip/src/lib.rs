use ising_core::graph::{IsingGraph, Symbol, SymbolKind};
use protobuf::Message;
use scip::types::{self, symbol_information, SymbolRole};
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
        (self.end_line - self.start_line, self.end_char - self.start_char)
    }
}

impl ScipLoader {
    pub fn load_from_file(path: &Path) -> Result<IsingGraph, ScipError> {
        let mut file = File::open(path)?;
        let index = types::Index::parse_from_reader(&mut file)?;
        Self::load_from_index(&index)
    }

    pub fn load_from_index(index: &types::Index) -> Result<IsingGraph, ScipError> {
        let mut graph = IsingGraph::new();
        let mut known_symbols = HashSet::new();
        let mut source_defs_by_document: Vec<Vec<SourceDef>> = Vec::with_capacity(index.documents.len());

        for document in &index.documents {
            for info in &document.symbols {
                if info.symbol.is_empty() {
                    return Err(ScipError::InvalidData(
                        "symbol information has empty symbol".to_string(),
                    ));
                }
                if known_symbols.insert(info.symbol.clone()) {
                    graph.add_symbol(Symbol {
                        name: info.symbol.clone(),
                        file: document.relative_path.clone(),
                        kind: map_kind(info.kind.enum_value_or_default()),
                    });
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
                    graph.add_symbol(Symbol {
                        name: occurrence.symbol.clone(),
                        file: document.relative_path.clone(),
                        kind: SymbolKind::Other("unknown".to_string()),
                    });
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
                    .add_dependency(from_symbol, &occurrence.symbol)
                    .map_err(|e| ScipError::InvalidData(e.to_string()))?;
            }
        }

        Ok(graph)
    }
}

fn has_role(symbol_roles: i32, role: SymbolRole) -> bool {
    symbol_roles & (role as i32) != 0
}

fn map_kind(kind: symbol_information::Kind) -> SymbolKind {
    match kind {
        symbol_information::Kind::Function
        | symbol_information::Kind::Method
        | symbol_information::Kind::Macro => SymbolKind::Function,
        symbol_information::Kind::Class
        | symbol_information::Kind::Enum
        | symbol_information::Kind::Struct => SymbolKind::Class,
        symbol_information::Kind::Package
        | symbol_information::Kind::Namespace
        | symbol_information::Kind::Module => SymbolKind::Module,
        symbol_information::Kind::Variable
        | symbol_information::Kind::Constant
        | symbol_information::Kind::Property => SymbolKind::Variable,
        symbol_information::Kind::Interface
        | symbol_information::Kind::Trait
        | symbol_information::Kind::Protocol => SymbolKind::Interface,
        other => SymbolKind::Other(format!("{other:?}")),
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
    fn malformed_reference_returns_invalid_data() {
        let index = Index {
            documents: vec![Document {
                relative_path: "src/lib.rs".to_string(),
                symbols: vec![
                    make_symbol("sym a", symbol_information::Kind::Function),
                    make_symbol("sym b", symbol_information::Kind::Function),
                ],
                occurrences: vec![
                    def_occ("sym a", vec![0, 0, 2, 0]),
                    ref_occ("sym b", vec![1, 2]),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let err = ScipLoader::load_from_index(&index).unwrap_err();
        assert!(matches!(err, ScipError::InvalidData(_)));
    }

    #[test]
    fn external_reference_is_skipped() {
        let index = Index {
            documents: vec![Document {
                relative_path: "src/lib.rs".to_string(),
                symbols: vec![make_symbol("sym a", symbol_information::Kind::Function)],
                occurrences: vec![
                    def_occ("sym a", vec![0, 0, 2, 0]),
                    ref_occ("external sym", vec![1, 0, 1, 1]),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let graph = ScipLoader::load_from_index(&index).unwrap();
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn symbol_kinds_map_to_ising_kinds() {
        let index = Index {
            documents: vec![Document {
                relative_path: "src/lib.rs".to_string(),
                symbols: vec![
                    make_symbol("fn", symbol_information::Kind::Function),
                    make_symbol("class", symbol_information::Kind::Class),
                    make_symbol("module", symbol_information::Kind::Namespace),
                    make_symbol("var", symbol_information::Kind::Variable),
                    make_symbol("iface", symbol_information::Kind::Interface),
                    make_symbol("other", symbol_information::Kind::Axiom),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let graph = ScipLoader::load_from_index(&index).unwrap();
        let kinds: Vec<_> = graph.graph.node_weights().map(|s| s.kind.clone()).collect();
        assert!(kinds.contains(&SymbolKind::Function));
        assert!(kinds.contains(&SymbolKind::Class));
        assert!(kinds.contains(&SymbolKind::Module));
        assert!(kinds.contains(&SymbolKind::Variable));
        assert!(kinds.contains(&SymbolKind::Interface));
        assert!(kinds.iter().any(|k| matches!(k, SymbolKind::Other(_))));
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
