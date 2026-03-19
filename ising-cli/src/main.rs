use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use ising_analysis::signals::detect_signals;
use ising_core::config::Config;
use ising_core::metrics::compute_graph_metrics;
use ising_db::Database;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "ising")]
#[command(about = "Three-layer code graph analysis engine")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Build the code graph: parse code + analyze git history + detect signals
    Build(BuildArgs),
    /// Show blast radius, dependencies, and risk signals for a file
    Impact(ImpactArgs),
    /// Show top hotspots ranked by change frequency × complexity
    Hotspots(HotspotsArgs),
    /// Show detected cross-layer signals
    Signals(SignalsArgs),
    /// Show global graph statistics
    Stats(StatsArgs),
    /// Export the graph in various formats
    Export(ExportArgs),
    /// Start the MCP server for AI agent integration
    Serve(ServeArgs),
}

#[derive(clap::Args, Debug)]
struct BuildArgs {
    /// Path to the repository root
    #[arg(long, default_value = ".")]
    repo_path: PathBuf,
    /// Git history time window (e.g., "6 months ago")
    #[arg(long)]
    since: Option<String>,
    /// Database file path
    #[arg(long, default_value = "ising.db")]
    db: PathBuf,
    /// Config file path
    #[arg(long, default_value = "ising.toml")]
    config: PathBuf,
}

#[derive(clap::Args, Debug)]
struct ImpactArgs {
    /// File path or qualified function name to analyze
    target: String,
    /// Database file path
    #[arg(long, default_value = "ising.db")]
    db: PathBuf,
}

#[derive(clap::Args, Debug)]
struct HotspotsArgs {
    /// Number of top hotspots to show
    #[arg(long, default_value = "20")]
    top: usize,
    /// Database file path
    #[arg(long, default_value = "ising.db")]
    db: PathBuf,
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(clap::Args, Debug)]
struct SignalsArgs {
    /// Filter by signal type
    #[arg(long, rename_all = "snake_case")]
    r#type: Option<String>,
    /// Minimum severity threshold
    #[arg(long)]
    min_severity: Option<f64>,
    /// Database file path
    #[arg(long, default_value = "ising.db")]
    db: PathBuf,
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(clap::Args, Debug)]
struct StatsArgs {
    /// Database file path
    #[arg(long, default_value = "ising.db")]
    db: PathBuf,
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(clap::Args, Debug)]
struct ExportArgs {
    /// Export format
    #[arg(long, value_enum)]
    format: ExportFormat,
    /// Database file path
    #[arg(long, default_value = "ising.db")]
    db: PathBuf,
    /// Output file (stdout if not specified)
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(clap::Args, Debug)]
struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value = "3000")]
    port: u16,
    /// Database file path
    #[arg(long, default_value = "ising.db")]
    db: PathBuf,
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
enum ExportFormat {
    Json,
    Dot,
    Mermaid,
}

fn main() {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let exit_code = match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            2
        }
    };
    std::process::exit(exit_code);
}

fn run(cli: Cli) -> Result<i32> {
    match cli.command {
        Commands::Build(args) => cmd_build(args),
        Commands::Impact(args) => cmd_impact(args),
        Commands::Hotspots(args) => cmd_hotspots(args),
        Commands::Signals(args) => cmd_signals(args),
        Commands::Stats(args) => cmd_stats(args),
        Commands::Export(args) => cmd_export(args),
        Commands::Serve(args) => cmd_serve(args),
    }
}

fn cmd_build(args: BuildArgs) -> Result<i32> {
    let mut config = Config::load_or_default(&args.config);

    if let Some(since) = args.since {
        config.build.time_window = since;
    }

    let repo_path = args.repo_path.canonicalize()?;
    eprintln!("Building graph for {}...", repo_path.display());

    // Build the multi-layer graph
    let graph = ising_builders::build_all(&repo_path, &config)?;

    // Detect cross-layer signals
    let signals = detect_signals(&graph, &config);

    // Compute graph metrics
    let metrics = compute_graph_metrics(&graph);

    // Store to database
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;
    db.clear()?;
    db.store_graph(&graph)?;

    // Store signals
    for signal in &signals {
        let details = serde_json::to_value(&signal)?;
        db.store_signal(
            &serde_json::to_value(&signal.signal_type)?
                .as_str()
                .unwrap_or("unknown"),
            &signal.node_a,
            signal.node_b.as_deref(),
            signal.severity,
            Some(&details),
        )?;
    }

    // Store build metadata
    let now = chrono::Utc::now().to_rfc3339();
    db.set_build_info("last_build", &now)?;
    db.set_build_info("repo_path", &repo_path.display().to_string())?;
    db.set_build_info("time_window", &config.build.time_window)?;

    // Summary output
    eprintln!();
    eprintln!("Build complete:");
    eprintln!("  Nodes:            {}", metrics.total_nodes);
    eprintln!("  Structural edges: {}", metrics.structural_edges);
    eprintln!("  Change edges:     {}", metrics.change_edges);
    eprintln!("  Defect edges:     {}", metrics.defect_edges);
    eprintln!("  Cycles:           {}", metrics.cycle_count);
    eprintln!("  Signals:          {}", signals.len());

    if !signals.is_empty() {
        eprintln!();
        eprintln!("Top signals:");
        for signal in signals.iter().take(5) {
            let priority = signal.signal_type.priority().to_uppercase();
            let target = match &signal.node_b {
                Some(b) => format!("{} <-> {}", signal.node_a, b),
                None => signal.node_a.clone(),
            };
            eprintln!("  [{priority}] {:?}: {target}", signal.signal_type);
        }
    }

    Ok(0)
}

fn cmd_impact(args: ImpactArgs) -> Result<i32> {
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;
    let impact = db.get_impact(&args.target)?;

    if impact.structural_deps.is_empty()
        && impact.temporal_coupling.is_empty()
        && impact.signals.is_empty()
    {
        eprintln!("No data found for '{}'", args.target);
        return Ok(1);
    }

    println!("Impact: {}", args.target);
    println!("{}", "═".repeat(40));

    if let Some(cm) = &impact.change_metrics {
        println!(
            "  Change Freq: {} | Hotspot: {:.2} | Churn Rate: {:.2}",
            cm.change_freq, cm.hotspot_score, cm.churn_rate
        );
        println!();
    }

    if !impact.structural_deps.is_empty() {
        println!("Structural Dependencies (fan-out: {}):", impact.structural_deps.len());
        for (target, edge_type, _weight) in &impact.structural_deps {
            println!("  -> {target}  ({edge_type})");
        }
        println!();
    }

    if !impact.temporal_coupling.is_empty() {
        println!("Temporal Coupling (co-change > threshold):");
        for (target, coupling) in &impact.temporal_coupling {
            println!("  <-> {target}  coupling: {coupling:.2}");
        }
        println!();
    }

    if !impact.signals.is_empty() {
        println!("Signals:");
        for signal in &impact.signals {
            let node_b = signal
                .node_b
                .as_deref()
                .map(|b| format!(" <-> {b}"))
                .unwrap_or_default();
            println!(
                "  [{:.2}] {}{node_b}",
                signal.severity, signal.signal_type
            );
        }
    }

    Ok(0)
}

fn cmd_hotspots(args: HotspotsArgs) -> Result<i32> {
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;
    let hotspots = db.get_hotspots(args.top)?;

    match args.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&hotspots)?);
        }
        OutputFormat::Text => {
            println!("Top {} Hotspots", args.top);
            println!("{}", "═".repeat(60));
            for (rank, (id, score, complexity, freq)) in hotspots.iter().enumerate() {
                println!(
                    "  {:>2}. {:<40} score: {:.2}  freq: {:.0}  complexity: {}",
                    rank + 1,
                    id,
                    score,
                    freq,
                    complexity
                );
            }
        }
    }

    Ok(0)
}

fn cmd_signals(args: SignalsArgs) -> Result<i32> {
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;
    let signals = db.get_signals(args.r#type.as_deref(), args.min_severity)?;

    match args.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&signals)?);
        }
        OutputFormat::Text => {
            println!("Signals ({} found)", signals.len());
            println!("{}", "═".repeat(60));
            for signal in &signals {
                let node_b = signal
                    .node_b
                    .as_deref()
                    .map(|b| format!(" <-> {b}"))
                    .unwrap_or_default();
                println!(
                    "  [{:.2}] {}: {}{}",
                    signal.severity, signal.signal_type, signal.node_a, node_b
                );
            }
        }
    }

    Ok(0)
}

fn cmd_stats(args: StatsArgs) -> Result<i32> {
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;
    let stats = db.get_stats()?;

    match args.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&stats)?);
        }
        OutputFormat::Text => {
            println!("Ising Graph Statistics");
            println!("{}", "═".repeat(30));
            println!("  Nodes:            {}", stats.node_count);
            println!("  Total edges:      {}", stats.edge_count);
            println!("  Structural edges: {}", stats.structural_edges);
            println!("  Change edges:     {}", stats.change_edges);
            println!("  Signals:          {}", stats.signal_count);

            if let Ok(Some(last_build)) = db.get_build_info("last_build") {
                println!("  Last build:       {last_build}");
            }
            if let Ok(Some(repo_path)) = db.get_build_info("repo_path") {
                println!("  Repository:       {repo_path}");
            }
        }
    }

    Ok(0)
}

fn cmd_export(args: ExportArgs) -> Result<i32> {
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;
    let stats = db.get_stats()?;
    let signals = db.get_signals(None, None)?;
    let hotspots = db.get_hotspots(100)?;

    let output = match args.format {
        ExportFormat::Json => {
            let export = serde_json::json!({
                "stats": stats,
                "signals": signals,
                "hotspots": hotspots,
            });
            serde_json::to_string_pretty(&export)?
        }
        ExportFormat::Dot => generate_dot(&db)?,
        ExportFormat::Mermaid => generate_mermaid(&db)?,
    };

    if let Some(path) = args.output {
        std::fs::write(&path, &output)?;
        eprintln!("Exported to {}", path.display());
    } else {
        println!("{output}");
    }

    Ok(0)
}

fn generate_dot(db: &Database) -> Result<String> {
    let mut out = String::from("digraph ising {\n  rankdir=LR;\n  node [shape=box];\n\n");

    // Query edges from db
    let signals = db.get_signals(None, None)?;
    let hotspots = db.get_hotspots(50)?;

    // Add hotspot nodes with color
    for (id, score, _, _) in &hotspots {
        let color = if *score > 0.7 {
            "red"
        } else if *score > 0.4 {
            "orange"
        } else {
            "lightblue"
        };
        let label = id.replace('"', "\\\"");
        out.push_str(&format!(
            "  \"{}\" [label=\"{}\\n{:.2}\", style=filled, fillcolor={}];\n",
            id, label, score, color
        ));
    }

    // Add signal edges
    for signal in &signals {
        if let Some(node_b) = &signal.node_b {
            let style = match signal.signal_type.as_str() {
                "ghost_coupling" => "style=dashed, color=purple",
                "fragile_boundary" => "style=bold, color=red",
                "over_engineering" => "style=dotted, color=gray",
                _ => "color=black",
            };
            out.push_str(&format!(
                "  \"{}\" -> \"{}\" [{}, label=\"{}\"];\n",
                signal.node_a, node_b, style, signal.signal_type
            ));
        }
    }

    out.push_str("}\n");
    Ok(out)
}

fn generate_mermaid(db: &Database) -> Result<String> {
    let mut out = String::from("graph LR\n");

    let signals = db.get_signals(None, None)?;
    let hotspots = db.get_hotspots(50)?;

    // Add hotspot nodes
    for (id, score, _, _) in &hotspots {
        let safe_id = id.replace('/', "_").replace('.', "_").replace(':', "_");
        let label = id.replace('"', "");
        if *score > 0.7 {
            out.push_str(&format!("  {safe_id}[\"{label}\\n🔥 {score:.2}\"]:::hot\n"));
        } else {
            out.push_str(&format!("  {safe_id}[\"{label}\\n{score:.2}\"]\n"));
        }
    }

    // Add signal edges
    for signal in &signals {
        if let Some(node_b) = &signal.node_b {
            let safe_a = signal
                .node_a
                .replace('/', "_")
                .replace('.', "_")
                .replace(':', "_");
            let safe_b = node_b
                .replace('/', "_")
                .replace('.', "_")
                .replace(':', "_");
            let arrow = match signal.signal_type.as_str() {
                "ghost_coupling" => "-.->",
                "fragile_boundary" => "==>",
                _ => "-->",
            };
            out.push_str(&format!(
                "  {safe_a} {arrow}|{}| {safe_b}\n",
                signal.signal_type
            ));
        }
    }

    out.push_str("  classDef hot fill:#f96,stroke:#333\n");
    Ok(out)
}

fn cmd_serve(args: ServeArgs) -> Result<i32> {
    let db_path = args.db.to_str().unwrap_or("ising.db").to_string();
    let port = args.port;

    eprintln!("Starting MCP server on port {port}...");
    eprintln!("  Database: {db_path}");
    eprintln!("  Endpoints:");
    eprintln!("    GET /tools     - list available tools");
    eprintln!("    GET /impact    - blast radius for a file");
    eprintln!("    GET /signals   - risk signals");
    eprintln!("    GET /health    - graph statistics");

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        ising_server::serve(&db_path, port)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    })?;

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli_structure() {
        Cli::command().debug_assert();
    }

    #[test]
    fn help_is_exposed() {
        let help = Cli::try_parse_from(["ising", "--help"]).unwrap_err();
        assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn version_is_exposed() {
        let version = Cli::try_parse_from(["ising", "--version"]).unwrap_err();
        assert_eq!(version.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn build_command_parses() {
        let cli = Cli::try_parse_from(["ising", "build", "--repo-path", "."]).unwrap();
        assert!(matches!(cli.command, Commands::Build(_)));
    }

    #[test]
    fn impact_command_parses() {
        let cli = Cli::try_parse_from(["ising", "impact", "src/main.rs"]).unwrap();
        assert!(matches!(cli.command, Commands::Impact(_)));
    }

    #[test]
    fn hotspots_command_parses() {
        let cli = Cli::try_parse_from(["ising", "hotspots", "--top", "10"]).unwrap();
        assert!(matches!(cli.command, Commands::Hotspots(_)));
    }

    #[test]
    fn signals_command_parses() {
        let cli =
            Cli::try_parse_from(["ising", "signals", "--type", "ghost_coupling", "--min-severity", "0.5"])
                .unwrap();
        assert!(matches!(cli.command, Commands::Signals(_)));
    }

    #[test]
    fn stats_command_parses() {
        let cli = Cli::try_parse_from(["ising", "stats"]).unwrap();
        assert!(matches!(cli.command, Commands::Stats(_)));
    }

    #[test]
    fn serve_command_parses() {
        let cli = Cli::try_parse_from(["ising", "serve", "--port", "8080"]).unwrap();
        assert!(matches!(cli.command, Commands::Serve(_)));
    }

    #[test]
    fn export_dot_parses() {
        let cli = Cli::try_parse_from(["ising", "export", "--format", "dot"]).unwrap();
        assert!(matches!(cli.command, Commands::Export(_)));
    }

    #[test]
    fn export_mermaid_parses() {
        let cli = Cli::try_parse_from(["ising", "export", "--format", "mermaid"]).unwrap();
        assert!(matches!(cli.command, Commands::Export(_)));
    }
}
