//! Physics engine for spectral analysis of code graphs.
//!
//! Computes the "energy" of the system via eigenvalue analysis of the
//! adjacency matrix. The spectral radius (λ_max) determines whether
//! perturbations (bugs, refactors) amplify or decay.

use crate::graph::IsingGraph;
use ndarray::Array2;

/// The health assessment of a codebase based on spectral analysis.
#[derive(Debug, Clone, PartialEq)]
pub enum HealthScore {
    /// λ_max < 1.0: perturbations decay quickly. The system is stable.
    Stable(f64),
    /// λ_max ≥ 1.0: perturbations amplify, indicating architectural fragility.
    Critical(f64),
}

impl HealthScore {
    /// Returns the raw λ_max value.
    pub fn lambda(&self) -> f64 {
        match self {
            HealthScore::Stable(l) | HealthScore::Critical(l) => *l,
        }
    }

    /// Returns `true` if the system is in a critical state.
    pub fn is_critical(&self) -> bool {
        matches!(self, HealthScore::Critical(_))
    }
}

/// Detect the phase transition state of a codebase graph.
///
/// Computes the spectral radius (maximum eigenvalue) of the adjacency matrix.
/// If λ_max > 1.0, the system is in a critical state where perturbations amplify.
pub fn detect_phase_transition(graph: &IsingGraph) -> HealthScore {
    let adjacency_matrix = graph.to_adjacency_matrix();
    let lambda = calculate_max_eigenvalue(&adjacency_matrix);

    if lambda > 1.0 {
        HealthScore::Critical(lambda)
    } else {
        HealthScore::Stable(lambda)
    }
}

/// Calculate the maximum eigenvalue (spectral radius) of a matrix
/// using the power iteration method.
///
/// This avoids the need for LAPACK/BLAS and is sufficient for
/// symmetric-ish adjacency matrices.
fn calculate_max_eigenvalue(matrix: &Array2<f64>) -> f64 {
    let n = matrix.nrows();
    if n == 0 {
        return 0.0;
    }

    // Power iteration: repeatedly multiply by the matrix and normalize
    let mut v = ndarray::Array1::<f64>::ones(n);
    let norm = (v.dot(&v)).sqrt();
    v /= norm;

    let max_iterations = 100;
    let tolerance = 1e-10;
    let mut eigenvalue = 0.0;

    for _ in 0..max_iterations {
        let w = matrix.dot(&v);
        let new_eigenvalue = w.dot(&v);
        let norm = (w.dot(&w)).sqrt();

        if norm < tolerance {
            return 0.0;
        }

        v = w / norm;

        if (new_eigenvalue - eigenvalue).abs() < tolerance {
            return new_eigenvalue.abs();
        }

        eigenvalue = new_eigenvalue;
    }

    eigenvalue.abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Symbol, SymbolKind};

    fn make_symbol(name: &str) -> Symbol {
        Symbol {
            name: name.to_string(),
            file: "test.rs".to_string(),
            kind: SymbolKind::Function,
        }
    }

    #[test]
    fn test_empty_graph_is_stable() {
        let g = IsingGraph::new();
        let score = detect_phase_transition(&g);
        assert!(!score.is_critical());
        assert_eq!(score.lambda(), 0.0);
    }

    #[test]
    fn test_simple_chain_is_stable() {
        let mut g = IsingGraph::new();
        g.add_symbol(make_symbol("a"));
        g.add_symbol(make_symbol("b"));
        g.add_dependency("a", "b").unwrap();

        let score = detect_phase_transition(&g);
        assert!(!score.is_critical());
    }

    #[test]
    fn test_dense_graph_is_critical() {
        // A fully connected graph of 4 nodes should have λ_max > 1
        let mut g = IsingGraph::new();
        let names = ["a", "b", "c", "d"];
        for name in &names {
            g.add_symbol(make_symbol(name));
        }
        for from in &names {
            for to in &names {
                if from != to {
                    g.add_dependency(from, to).unwrap();
                }
            }
        }

        let score = detect_phase_transition(&g);
        assert!(score.is_critical(), "Dense graph should be critical, got λ={}", score.lambda());
    }
}
