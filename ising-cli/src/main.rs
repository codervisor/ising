use clap::{Parser, Subcommand, ValueEnum};
use ising_core::graph::IsingGraph;
use ising_core::physics::detect_phase_transition;
use ising_scip::ScipLoader;
use petgraph::unionfind::UnionFind;
use petgraph::visit::EdgeRef;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("missing index.scip in directory `{0}`; generate one and retry")]
    MissingIndex(String),
    #[error("invalid input path `{0}`")]
    InvalidInput(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Scip(#[from] ising_scip::ScipError),
}

#[derive(Parser, Debug)]
#[command(name = "ising")]
#[command(about = "Maintainability analysis for software projects")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Analyze(AnalyzeArgs),
}

#[derive(clap::Args, Debug)]
struct AnalyzeArgs {
    path: PathBuf,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
enum OutputFormat {
    Json,
    Text,
}

#[derive(Debug, Serialize)]
struct AnalyzeReport {
    version: String,
    path: String,
    health: HealthReport,
    summary: SummaryReport,
}

#[derive(Debug, Serialize)]
struct HealthReport {
    lambda_max: f64,
    status: String,
    modularity_q: f64,
}

#[derive(Debug, Serialize)]
struct SummaryReport {
    symbols: usize,
    dependencies: usize,
}

fn main() {
    let exit_code = match run(Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            2
        }
    };
    std::process::exit(exit_code);
}

fn run(cli: Cli) -> Result<i32, CliError> {
    match cli.command {
        Commands::Analyze(args) => analyze(args),
    }
}

fn analyze(args: AnalyzeArgs) -> Result<i32, CliError> {
    let (scip_path, report_path) = resolve_input_path(&args.path)?;
    let graph = ScipLoader::load_from_file(&scip_path)?;
    let lambda = detect_phase_transition(&graph).lambda();
    let modularity_q = estimate_modularity_q(&graph);

    let report = AnalyzeReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        path: report_path,
        health: HealthReport {
            lambda_max: lambda,
            status: status_from_lambda(lambda).to_string(),
            modularity_q,
        },
        summary: SummaryReport {
            symbols: graph.node_count(),
            dependencies: graph.edge_count(),
        },
    };

    let rendered = match args.format {
        OutputFormat::Json => serde_json::to_string_pretty(&report).expect("serializable report"),
        OutputFormat::Text => format_text_report(&report, &graph),
    };

    if let Some(path) = args.output {
        fs::write(path, rendered)?;
    } else {
        println!("{rendered}");
    }

    Ok(exit_code(lambda))
}

fn resolve_input_path(path: &Path) -> Result<(PathBuf, String), CliError> {
    if path.is_dir() {
        let scip_path = path.join("index.scip");
        if !scip_path.is_file() {
            return Err(CliError::MissingIndex(path.display().to_string()));
        }
        return Ok((scip_path, path.display().to_string()));
    }
    if path.is_file() {
        return Ok((path.to_path_buf(), path.display().to_string()));
    }
    Err(CliError::InvalidInput(path.display().to_string()))
}

fn status_from_lambda(lambda: f64) -> &'static str {
    if lambda >= 1.0 { "critical" } else { "stable" }
}

fn exit_code(lambda: f64) -> i32 {
    if lambda >= 1.0 { 1 } else { 0 }
}

fn format_text_report(report: &AnalyzeReport, graph: &IsingGraph) -> String {
    let mut lines = Vec::new();
    lines.push("Ising Health Report".to_string());
    lines.push("═══════════════════".to_string());
    lines.push(format!("Repository:   {}", report.path));
    lines.push(format!("Symbols:      {}", report.summary.symbols));
    lines.push(format!("Dependencies: {}", report.summary.dependencies));
    lines.push(String::new());
    lines.push(format!(
        "λ_max:        {:.2}  {}",
        report.health.lambda_max,
        if report.health.status == "critical" {
            "⚠ CRITICAL (>= 1.0)"
        } else {
            "✓ STABLE (< 1.0)"
        }
    ));
    lines.push(format!("Modularity:   {:.2}", report.health.modularity_q));
    lines.push(String::new());
    lines.push("Top coupling hotspots:".to_string());

    for (rank, (name, degree)) in top_hotspots(graph).into_iter().take(5).enumerate() {
        lines.push(format!("  {}. {} (degree: {})", rank + 1, name, degree));
    }

    lines.join("\n")
}

fn top_hotspots(graph: &IsingGraph) -> Vec<(String, usize)> {
    let mut hotspots: Vec<(String, usize)> = graph
        .graph
        .node_indices()
        .map(|node| {
            let degree = graph.graph.neighbors(node).count()
                + graph
                    .graph
                    .neighbors_directed(node, petgraph::Direction::Incoming)
                    .count();
            (graph.graph[node].file.clone(), degree)
        })
        .collect();

    hotspots.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    hotspots
}

fn estimate_modularity_q(graph: &IsingGraph) -> f64 {
    let n = graph.node_count();
    if n == 0 {
        return 1.0;
    }

    let mut union_find = UnionFind::new(n);
    let mut edges = HashSet::new();
    let mut degree = vec![0usize; n];

    for edge in graph.graph.edge_references() {
        let s = edge.source().index();
        let t = edge.target().index();
        union_find.union(s, t);
        if s == t {
            continue;
        }
        let (a, b) = if s < t { (s, t) } else { (t, s) };
        if edges.insert((a, b)) {
            degree[a] += 1;
            degree[b] += 1;
        }
    }

    let m = edges.len() as f64;
    if m == 0.0 {
        return 1.0;
    }

    let mut community = vec![0usize; n];
    for (i, item) in community.iter_mut().enumerate() {
        *item = union_find.find(i);
    }

    let mut sum = 0.0;
    for i in 0..n {
        for j in 0..n {
            if community[i] != community[j] {
                continue;
            }
            let (a, b) = if i < j { (i, j) } else { (j, i) };
            let a_ij = if i != j && edges.contains(&(a, b)) {
                1.0
            } else {
                0.0
            };
            sum += a_ij - (degree[i] as f64 * degree[j] as f64) / (2.0 * m);
        }
    }

    (sum / (2.0 * m)).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use protobuf::{EnumOrUnknown, Message};
    use scip::types::{Document, Index, Occurrence, SymbolInformation, SymbolRole, symbol_information};
    use tempfile::NamedTempFile;

    fn make_symbol(symbol: &str) -> SymbolInformation {
        SymbolInformation {
            symbol: symbol.to_string(),
            kind: EnumOrUnknown::new(symbol_information::Kind::Function),
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

    fn write_index(index: Index) -> NamedTempFile {
        let mut tmp = NamedTempFile::new().unwrap();
        index.write_to_writer(&mut tmp).unwrap();
        tmp
    }

    fn stable_index_file() -> NamedTempFile {
        write_index(Index {
            documents: vec![Document {
                relative_path: "src/lib.rs".to_string(),
                symbols: vec![make_symbol("sym a"), make_symbol("sym b")],
                occurrences: vec![
                    def_occ("sym a", vec![0, 0, 2, 0]),
                    def_occ("sym b", vec![3, 0, 5, 0]),
                    ref_occ("sym b", vec![1, 0, 1, 1]),
                ],
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    fn critical_index_file() -> NamedTempFile {
        write_index(Index {
            documents: vec![Document {
                relative_path: "src/lib.rs".to_string(),
                symbols: vec![make_symbol("a"), make_symbol("b"), make_symbol("c")],
                occurrences: vec![
                    def_occ("a", vec![0, 0, 3, 0]),
                    def_occ("b", vec![4, 0, 7, 0]),
                    def_occ("c", vec![8, 0, 11, 0]),
                    ref_occ("b", vec![1, 0, 1, 1]),
                    ref_occ("c", vec![2, 0, 2, 1]),
                    ref_occ("a", vec![5, 0, 5, 1]),
                    ref_occ("c", vec![6, 0, 6, 1]),
                    ref_occ("a", vec![9, 0, 9, 1]),
                    ref_occ("b", vec![10, 0, 10, 1]),
                ],
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    #[test]
    fn analyze_json_output_schema() {
        let file = stable_index_file();
        let output = NamedTempFile::new().unwrap();
        let cli = Cli::parse_from([
            "ising",
            "analyze",
            file.path().to_str().unwrap(),
            "--format",
            "json",
            "--output",
            output.path().to_str().unwrap(),
        ]);
        let code = run(cli).unwrap();
        assert_eq!(code, 0);

        let raw = fs::read_to_string(output.path()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(json.get("version").is_some());
        assert!(json.get("path").is_some());
        assert!(json.get("health").and_then(|h| h.get("lambda_max")).is_some());
        assert!(json.get("health").and_then(|h| h.get("modularity_q")).is_some());
        assert!(json.get("summary").and_then(|s| s.get("symbols")).is_some());
        assert!(json.get("summary").and_then(|s| s.get("dependencies")).is_some());
    }

    #[test]
    fn analyze_text_output_written_to_file() {
        let file = stable_index_file();
        let output = NamedTempFile::new().unwrap();
        let cli = Cli::parse_from([
            "ising",
            "analyze",
            file.path().to_str().unwrap(),
            "--format",
            "text",
            "--output",
            output.path().to_str().unwrap(),
        ]);
        let code = run(cli).unwrap();
        assert_eq!(code, 0);

        let raw = fs::read_to_string(output.path()).unwrap();
        assert!(raw.contains("Ising Health Report"));
        assert!(raw.contains("Top coupling hotspots:"));
    }

    #[test]
    fn exit_code_critical_when_lambda_at_least_one() {
        let file = critical_index_file();
        let output = NamedTempFile::new().unwrap();
        let cli = Cli::parse_from([
            "ising",
            "analyze",
            file.path().to_str().unwrap(),
            "--output",
            output.path().to_str().unwrap(),
        ]);
        let code = run(cli).unwrap();
        assert_eq!(code, 1);
    }

    #[test]
    fn help_and_version_are_exposed() {
        let cmd = Cli::command();
        cmd.debug_assert();

        let help = Cli::try_parse_from(["ising", "--help"]).unwrap_err();
        assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);

        let version = Cli::try_parse_from(["ising", "--version"]).unwrap_err();
        assert_eq!(version.kind(), clap::error::ErrorKind::DisplayVersion);

        let analyze_help = Cli::try_parse_from(["ising", "analyze", "--help"]).unwrap_err();
        assert_eq!(analyze_help.kind(), clap::error::ErrorKind::DisplayHelp);
    }
}
