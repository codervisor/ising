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
use std::process::Command;

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("missing index.scip in directory `{0}`; generate one with `ising index`")]
    MissingIndex(String),
    #[error("invalid input path `{0}`")]
    InvalidInput(String),
    #[error("could not detect project language in `{0}`; use --language to specify")]
    UnknownLanguage(String),
    #[error("indexer `{tool}` is not installed and auto-install failed: {reason}")]
    InstallFailed { tool: String, reason: String },
    #[error("indexer failed: {0}")]
    IndexerFailed(String),
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
    /// Generate a SCIP index for a project (auto-installs indexer if needed)
    Index(IndexArgs),
    /// Analyze a project's maintainability from its SCIP index
    Analyze(AnalyzeArgs),
}

#[derive(clap::Args, Debug)]
struct IndexArgs {
    /// Path to the project root (default: current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Override language detection
    #[arg(long, value_enum)]
    language: Option<Language>,
    /// Output path for the SCIP index file
    #[arg(long, default_value = "index.scip")]
    output: PathBuf,
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
enum Language {
    Rust,
    Typescript,
    Javascript,
    Python,
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
        Commands::Index(args) => index(args),
        Commands::Analyze(args) => analyze(args),
    }
}

fn index(args: IndexArgs) -> Result<i32, CliError> {
    let path = args.path.canonicalize().map_err(|_| {
        CliError::InvalidInput(args.path.display().to_string())
    })?;

    let lang = match args.language {
        Some(l) => l,
        None => detect_language(&path)?,
    };

    ensure_indexer(lang)?;

    eprintln!("Indexing {} project at {}...", lang.label(), path.display());
    run_indexer(lang, &path, &args.output)?;

    let output_path = path.join(&args.output);
    let size = fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(0);
    eprintln!("Generated {} ({} bytes)", args.output.display(), size);
    Ok(0)
}

impl Language {
    fn label(self) -> &'static str {
        match self {
            Language::Rust => "Rust",
            Language::Typescript => "TypeScript",
            Language::Javascript => "JavaScript",
            Language::Python => "Python",
        }
    }
}

fn detect_language(path: &Path) -> Result<Language, CliError> {
    if path.join("Cargo.toml").is_file() {
        return Ok(Language::Rust);
    }
    if path.join("tsconfig.json").is_file() {
        return Ok(Language::Typescript);
    }
    if path.join("package.json").is_file() {
        return Ok(Language::Javascript);
    }
    if path.join("pyproject.toml").is_file()
        || path.join("setup.py").is_file()
        || path.join("setup.cfg").is_file()
    {
        return Ok(Language::Python);
    }
    Err(CliError::UnknownLanguage(path.display().to_string()))
}

fn ensure_indexer(lang: Language) -> Result<(), CliError> {
    match lang {
        Language::Rust => ensure_rust_analyzer(),
        Language::Typescript | Language::Javascript => ensure_npm_tool("scip-typescript", "@sourcegraph/scip-typescript"),
        Language::Python => ensure_npm_tool("scip-python", "@sourcegraph/scip-python"),
    }
}

fn ensure_rust_analyzer() -> Result<(), CliError> {
    if command_exists("rust-analyzer") {
        return Ok(());
    }
    eprintln!("rust-analyzer not found, installing via rustup...");
    let status = Command::new("rustup")
        .args(["component", "add", "rust-analyzer"])
        .status()
        .map_err(|e| CliError::InstallFailed {
            tool: "rust-analyzer".into(),
            reason: format!("failed to run rustup: {e}"),
        })?;
    if !status.success() {
        return Err(CliError::InstallFailed {
            tool: "rust-analyzer".into(),
            reason: "rustup component add failed".into(),
        });
    }
    Ok(())
}

fn ensure_npm_tool(bin_name: &str, package: &str) -> Result<(), CliError> {
    if command_exists(bin_name) {
        return Ok(());
    }
    eprintln!("{bin_name} not found, installing {package}...");
    let status = Command::new("npm")
        .args(["install", "-g", package])
        .status()
        .map_err(|e| CliError::InstallFailed {
            tool: bin_name.into(),
            reason: format!("failed to run npm: {e}"),
        })?;
    if !status.success() {
        return Err(CliError::InstallFailed {
            tool: bin_name.into(),
            reason: format!("npm install -g {package} failed"),
        });
    }
    Ok(())
}

fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn run_indexer(lang: Language, project_dir: &Path, output: &Path) -> Result<(), CliError> {
    let status = match lang {
        Language::Rust => Command::new("rust-analyzer")
            .args(["scip", ".", "--output"])
            .arg(output)
            .current_dir(project_dir)
            .status(),
        Language::Typescript => Command::new("scip-typescript")
            .arg("index")
            .current_dir(project_dir)
            .status(),
        Language::Javascript => Command::new("scip-typescript")
            .args(["index", "--infer-tsconfig"])
            .current_dir(project_dir)
            .status(),
        Language::Python => {
            let project_name = project_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project");
            Command::new("scip-python")
                .args(["index", ".", &format!("--project-name={project_name}")])
                .current_dir(project_dir)
                .status()
        }
    };

    let status = status.map_err(|e| CliError::IndexerFailed(e.to_string()))?;
    if !status.success() {
        return Err(CliError::IndexerFailed(format!(
            "{} indexer exited with {}",
            lang.label(),
            status
        )));
    }

    // For TS/JS/Python, the indexer writes index.scip in the project dir.
    // If a custom --output was given and differs, rename it.
    if lang != Language::Rust {
        let default_output = project_dir.join("index.scip");
        let target = project_dir.join(output);
        if default_output != target && default_output.is_file() {
            fs::rename(&default_output, &target)?;
        }
    }

    Ok(())
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
