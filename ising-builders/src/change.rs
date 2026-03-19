//! Layer 2 — Change Graph Builder
//!
//! Uses gix (gitoxide) to analyze git history and compute:
//! - Temporal coupling (co-change frequency between file pairs)
//! - Change frequency per file
//! - Code churn (lines added + deleted)
//! - Hotspot scores (change frequency × complexity)

use gix::bstr::ByteSlice;
use ising_core::config::Config;
use ising_core::graph::{ChangeMetrics, EdgeType, Node, UnifiedGraph};
use std::collections::HashMap;
use std::path::Path;

/// Build the change graph from git history.
pub fn build_change_graph(
    repo_path: &Path,
    config: &Config,
) -> Result<UnifiedGraph, anyhow::Error> {
    let mut graph = UnifiedGraph::new();

    let repo = gix::open(repo_path)?;
    let head = repo.head_commit()?;

    let min_co_changes = config.thresholds.min_co_changes;
    let min_coupling = config.thresholds.min_coupling;

    // Collect changed files per commit by walking the commit graph
    let mut file_changes: HashMap<String, u32> = HashMap::new();
    let mut co_changes: HashMap<(String, String), u32> = HashMap::new();
    let mut total_commits: u32 = 0;

    // Walk commit ancestry
    let mut commit_id = head.id;
    let mut seen = std::collections::HashSet::new();

    // Simple linear walk — follow first parent chain
    loop {
        if !seen.insert(commit_id) {
            break;
        }

        let commit = match repo.find_commit(commit_id) {
            Ok(c) => c,
            Err(_) => break,
        };

        // Get changed files by diffing against parent
        let changed_files = get_changed_files(&repo, &commit)?;

        if !changed_files.is_empty() {
            total_commits += 1;

            for f in &changed_files {
                *file_changes.entry(f.clone()).or_default() += 1;
            }

            // All unique pairs
            let files_vec: Vec<&String> = changed_files.iter().collect();
            for i in 0..files_vec.len() {
                for j in (i + 1)..files_vec.len() {
                    let key = ordered_pair(files_vec[i], files_vec[j]);
                    *co_changes.entry(key).or_default() += 1;
                }
            }
        }

        // Move to first parent
        match commit.parent_ids().next() {
            Some(parent_id) => commit_id = parent_id.detach(),
            None => break,
        }
    }

    tracing::info!(
        "Analyzed {} commits, {} unique files",
        total_commits,
        file_changes.len()
    );

    // Add module nodes for all files seen in git history
    for file in file_changes.keys() {
        graph.add_node(Node::module(file, file));
    }

    // Compute coupling scores and add co-change edges
    for ((a, b), count) in &co_changes {
        if *count < min_co_changes {
            continue;
        }
        let freq_a = file_changes[a] as f64;
        let freq_b = file_changes[b] as f64;
        let denom = freq_a.min(freq_b);
        if denom == 0.0 {
            continue;
        }
        let coupling = *count as f64 / denom;
        if coupling >= min_coupling {
            let _ = graph.add_edge(a, b, EdgeType::CoChanges, coupling);
        }
    }

    // Compute per-file change metrics
    let max_freq = file_changes.values().copied().max().unwrap_or(1) as f64;
    for (file, freq) in &file_changes {
        let normalized_freq = *freq as f64 / max_freq;
        // Hotspot = normalized frequency (complexity will be merged from structural graph later)
        let hotspot = normalized_freq;

        // Sum of coupling for this file
        let sum_coupling: f64 = co_changes
            .iter()
            .filter(|((a, b), count)| {
                (a == file || b == file) && *count >= &min_co_changes
            })
            .map(|((a, b), count)| {
                let freq_a = file_changes[a] as f64;
                let freq_b = file_changes[b] as f64;
                let denom = freq_a.min(freq_b);
                if denom > 0.0 {
                    *count as f64 / denom
                } else {
                    0.0
                }
            })
            .filter(|c| *c >= min_coupling)
            .sum();

        graph.change_metrics.insert(
            file.clone(),
            ChangeMetrics {
                change_freq: *freq,
                churn_lines: 0, // Would require per-file diff analysis
                churn_rate: 0.0,
                hotspot_score: hotspot,
                sum_coupling,
                last_changed: None,
            },
        );
    }

    Ok(graph)
}

/// Get the list of changed files in a commit (compared to its first parent).
fn get_changed_files(
    repo: &gix::Repository,
    commit: &gix::Commit<'_>,
) -> Result<std::collections::HashSet<String>, anyhow::Error> {
    let mut changed = std::collections::HashSet::new();

    let tree = commit.tree()?;

    // If there's a parent, diff against it; otherwise this is the root commit
    let parent_tree = commit
        .parent_ids()
        .next()
        .and_then(|pid| repo.find_commit(pid.detach()).ok())
        .and_then(|pc| pc.tree().ok());

    match parent_tree {
        Some(ptree) => {
            ptree
                .changes()?
                .for_each_to_obtain_tree(&tree, |change| {
                    if let Ok(path) = change.location().to_str() {
                        changed.insert(path.to_string());
                    }
                    Ok::<_, std::convert::Infallible>(gix::object::tree::diff::Action::Continue)
                })?;
        }
        None => {
            // Root commit: diff empty tree → commit tree
            let empty_tree = repo.empty_tree();
            empty_tree
                .changes()?
                .for_each_to_obtain_tree(&tree, |change| {
                    if let Ok(path) = change.location().to_str() {
                        changed.insert(path.to_string());
                    }
                    Ok::<_, std::convert::Infallible>(gix::object::tree::diff::Action::Continue)
                })?;
        }
    }

    Ok(changed)
}

/// Create an ordered pair (for consistent HashMap keys).
fn ordered_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ordered_pair() {
        assert_eq!(
            ordered_pair("b.py", "a.py"),
            ("a.py".to_string(), "b.py".to_string())
        );
        assert_eq!(
            ordered_pair("a.py", "b.py"),
            ("a.py".to_string(), "b.py".to_string())
        );
    }
}
