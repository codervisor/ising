//! Layer 2 — Change Graph Builder
//!
//! Uses gix (gitoxide) to analyze git history and compute:
//! - Temporal coupling (co-change frequency between file pairs)
//! - Change frequency per file
//! - Code churn (lines added + deleted)
//! - Hotspot scores (change frequency × complexity)

use crate::common::Language;
use gix::bstr::ByteSlice;
use ising_core::config::Config;
use ising_core::graph::{ChangeMetrics, EdgeType, Node, UnifiedGraph};
use ising_core::ignore::IgnoreRules;
use std::collections::HashMap;
use std::path::Path;

/// Parse a time window string (e.g., "6 months ago") into a Unix timestamp cutoff.
fn parse_time_window(window: &str) -> Option<i64> {
    let parts: Vec<&str> = window.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let amount: i64 = parts[0].parse().ok()?;
    let unit = parts[1].trim_end_matches('s'); // "months" -> "month"
    let seconds = match unit {
        "day" => amount * 86_400,
        "week" => amount * 7 * 86_400,
        "month" => amount * 30 * 86_400,
        "year" => amount * 365 * 86_400,
        _ => return None,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some(now - seconds)
}

/// Build the change graph from git history.
pub fn build_change_graph(
    repo_path: &Path,
    config: &Config,
    ignore: &IgnoreRules,
) -> Result<UnifiedGraph, anyhow::Error> {
    let mut graph = UnifiedGraph::new();

    let repo = gix::open(repo_path)?;
    let head = repo.head_commit()?;

    let min_co_changes = config.thresholds.min_co_changes;
    let min_coupling = config.thresholds.min_coupling;
    let max_commits = config.build.max_commits;
    let max_files_per_commit = config.build.max_files_per_commit as usize;

    // Parse time window into a cutoff timestamp
    let cutoff_timestamp = parse_time_window(&config.build.time_window);
    if let Some(ts) = cutoff_timestamp {
        tracing::info!("Time window cutoff: {} (unix)", ts);
    }

    // Collect changed files per commit by walking the commit graph
    let mut file_changes: HashMap<String, u32> = HashMap::new();
    let mut co_changes: HashMap<(String, String), u32> = HashMap::new();
    let mut total_commits: u32 = 0;
    let mut skipped_large: u32 = 0;
    let mut skipped_old: u32 = 0;

    // Walk commit ancestry
    let mut commit_id = head.id;
    let mut seen = std::collections::HashSet::new();

    // Simple linear walk — follow first parent chain
    loop {
        if !seen.insert(commit_id) {
            break;
        }

        // Respect max_commits limit
        if max_commits > 0 && total_commits >= max_commits {
            tracing::info!("Reached max_commits limit ({})", max_commits);
            break;
        }

        let commit = match repo.find_commit(commit_id) {
            Ok(c) => c,
            Err(_) => break,
        };

        // Apply time window filter
        if let Some(cutoff) = cutoff_timestamp {
            let commit_time = commit.time().ok().map(|t| t.seconds);
            if let Some(ct) = commit_time
                && ct < cutoff
            {
                skipped_old += 1;
                // Once we've hit commits older than the window, stop entirely
                // (first-parent chain is roughly chronological)
                if skipped_old > 100 {
                    tracing::info!("Stopping traversal: consistently outside time window");
                    break;
                }
                // Move to first parent and continue (some commits may be out of order)
                match commit.parent_ids().next() {
                    Some(parent_id) => {
                        commit_id = parent_id.detach();
                        continue;
                    }
                    None => break,
                }
            }
            // Reset consecutive old counter when we find an in-window commit
            skipped_old = 0;
        }

        // Get changed files by diffing against parent (only source code files, respecting .isingignore)
        let changed_files: std::collections::HashSet<String> = get_changed_files(&repo, &commit)?
            .into_iter()
            .filter(|f| Language::is_supported_file(f) && !ignore.is_ignored(f))
            .collect();

        if !changed_files.is_empty() {
            // Skip bulk commits that touch too many files (noisy: mass renames, formatting, etc.)
            if max_files_per_commit > 0 && changed_files.len() > max_files_per_commit {
                skipped_large += 1;
                // Still count individual file changes for frequency, but skip co-change pairs
                for f in &changed_files {
                    *file_changes.entry(f.clone()).or_default() += 1;
                }
            } else {
                for f in &changed_files {
                    *file_changes.entry(f.clone()).or_default() += 1;
                }

                // All unique pairs (only for reasonably-sized commits)
                let files_vec: Vec<&String> = changed_files.iter().collect();
                for i in 0..files_vec.len() {
                    for j in (i + 1)..files_vec.len() {
                        let key = ordered_pair(files_vec[i], files_vec[j]);
                        *co_changes.entry(key).or_default() += 1;
                    }
                }
            }

            total_commits += 1;
        }

        // Move to first parent
        match commit.parent_ids().next() {
            Some(parent_id) => commit_id = parent_id.detach(),
            None => break,
        }
    }

    tracing::info!(
        "Analyzed {} commits, {} unique files, skipped {} large commits",
        total_commits,
        file_changes.len(),
        skipped_large
    );

    // Add module nodes for all files seen in git history
    for file in file_changes.keys() {
        graph.add_node(Node::module(file, file));
    }

    // Pre-build a per-file index of co-change pairs for O(1) lookup
    // instead of scanning all pairs for each file (O(n*m) -> O(n+m))
    let mut file_cochange_index: HashMap<&str, Vec<(&str, &str, u32)>> = HashMap::new();
    for ((a, b), count) in &co_changes {
        if *count >= min_co_changes {
            file_cochange_index.entry(a.as_str()).or_default().push((
                a.as_str(),
                b.as_str(),
                *count,
            ));
            file_cochange_index.entry(b.as_str()).or_default().push((
                a.as_str(),
                b.as_str(),
                *count,
            ));
        }
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

    // Compute per-file change metrics using the pre-built index
    let max_freq = file_changes.values().copied().max().unwrap_or(1) as f64;
    for (file, freq) in &file_changes {
        let normalized_freq = *freq as f64 / max_freq;
        let hotspot = normalized_freq;

        // Sum of coupling for this file — O(neighbors) instead of O(all pairs)
        let sum_coupling: f64 = file_cochange_index
            .get(file.as_str())
            .map(|pairs| {
                pairs
                    .iter()
                    .map(|(a, b, count)| {
                        let freq_a = file_changes[*a] as f64;
                        let freq_b = file_changes[*b] as f64;
                        let denom = freq_a.min(freq_b);
                        if denom > 0.0 {
                            *count as f64 / denom
                        } else {
                            0.0
                        }
                    })
                    .filter(|c| *c >= min_coupling)
                    .sum()
            })
            .unwrap_or(0.0);

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
            ptree.changes()?.for_each_to_obtain_tree(&tree, |change| {
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

    #[test]
    fn test_is_supported_file_includes_rust() {
        assert!(Language::is_supported_file("src/main.rs"));
        assert!(Language::is_supported_file("ising-core/src/lib.rs"));
        assert!(Language::is_supported_file("app.py"));
        assert!(Language::is_supported_file("index.ts"));
        assert!(!Language::is_supported_file("readme.md"));
        assert!(!Language::is_supported_file("Cargo.toml"));
    }
}
