use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use ising_analysis::boundary_health::compute_boundary_health;
use ising_analysis::signals::{detect_signals, summarize_signals};
use ising_analysis::stress;
use ising_core::boundary::BoundaryStructure;
use ising_core::config::Config;
use ising_core::fea::{LoadCase, RiskTier};
use ising_core::graph::NodeType;
use ising_core::metrics::compute_graph_metrics;
use ising_core::path_utils::is_test_file;
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
    /// Show safety factor analysis for all modules
    Safety(SafetyArgs),
    /// Show aggregate repository health index
    Health(HealthArgs),
    /// Simulate a load case and show risk impact
    Simulate(SimulateArgs),
    /// Start the MCP server for AI agent integration
    Serve(ServeArgs),
    /// Show detected module boundaries
    Boundaries(BoundariesArgs),
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
    /// Exclude test files from results
    #[arg(long)]
    exclude_tests: bool,
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
struct SafetyArgs {
    /// Number of top results to show
    #[arg(long, default_value = "20")]
    top: usize,
    /// Filter by safety zone (critical, danger, warning, healthy, stable)
    #[arg(long)]
    zone: Option<String>,
    /// Exclude test files from results
    #[arg(long)]
    exclude_tests: bool,
    /// Database file path
    #[arg(long, default_value = "ising.db")]
    db: PathBuf,
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(clap::Args, Debug)]
struct HealthArgs {
    /// Database file path
    #[arg(long, default_value = "ising.db")]
    db: PathBuf,
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(clap::Args, Debug)]
struct SimulateArgs {
    /// File path or load-case JSON file
    target: String,
    /// Database file path
    #[arg(long, default_value = "ising.db")]
    db: PathBuf,
    /// Config file path
    #[arg(long, default_value = "ising.toml")]
    config: PathBuf,
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
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

#[derive(clap::Args, Debug)]
struct BoundariesArgs {
    /// Path to the repository root
    #[arg(long, default_value = ".")]
    repo_path: PathBuf,
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
enum ExportFormat {
    Json,
    Dot,
    Mermaid,
    VizJson,
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
        Commands::Safety(args) => cmd_safety(args),
        Commands::Health(args) => cmd_health(args),
        Commands::Simulate(args) => cmd_simulate(args),
        Commands::Serve(args) => cmd_serve(args),
        Commands::Boundaries(args) => cmd_boundaries(args),
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

    // Detect module boundaries
    let module_ids: Vec<String> = graph
        .node_ids()
        .filter(|id| {
            graph
                .get_node(id)
                .is_some_and(|n| n.node_type == NodeType::Module)
        })
        .map(|s| s.to_string())
        .collect();
    let module_id_refs: Vec<&str> = module_ids.iter().map(|s| s.as_str()).collect();
    let boundaries = BoundaryStructure::detect(&repo_path, &module_id_refs);

    eprintln!(
        "Boundaries: {} ({:?}, {} modules)",
        match &boundaries.l1_source {
            ising_core::boundary::BoundarySource::Manifest { ecosystem } => ecosystem.clone(),
            ising_core::boundary::BoundarySource::Directory => "directory".to_string(),
            ising_core::boundary::BoundarySource::SingleRoot => "single-root".to_string(),
        },
        boundaries.l1_source,
        boundaries.module_count(),
    );

    // Detect cross-layer signals (boundary-aware)
    let signals = detect_signals(&graph, &config, Some(&boundaries));

    // Compute graph metrics
    let metrics = compute_graph_metrics(&graph);

    // Store to database
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;
    db.clear()?;
    db.store_graph(&graph)?;

    // Store signals
    for signal in &signals {
        let details = serde_json::to_value(signal)?;
        db.store_signal(
            serde_json::to_value(&signal.signal_type)?
                .as_str()
                .unwrap_or("unknown"),
            &signal.node_a,
            signal.node_b.as_deref(),
            signal.severity,
            Some(&details),
        )?;
    }

    // Compute initial risk field for boundary health computation
    let signal_summary = summarize_signals(&signals);
    let initial_field =
        stress::compute_risk_field(&graph, &config, Some(&signal_summary), None, None);

    // Compute boundary health metrics using initial risk field
    let boundary_report = compute_boundary_health(&graph, &boundaries, &initial_field.nodes);

    // Compute final risk field with boundary-aware propagation and health index
    let risk_field = stress::compute_risk_field(
        &graph,
        &config,
        Some(&signal_summary),
        Some(&boundaries),
        Some(&boundary_report),
    );
    db.store_risk_field(&risk_field)?;

    // Count modules vs functions in risk field
    let module_risk_count = risk_field
        .nodes
        .iter()
        .filter(|n| !n.node_id.contains("::"))
        .count();
    let func_risk_count = risk_field.nodes.len() - module_risk_count;
    let critical_count = risk_field
        .nodes
        .iter()
        .filter(|n| n.risk_tier == RiskTier::Critical)
        .count();
    let high_count = risk_field
        .nodes
        .iter()
        .filter(|n| n.risk_tier == RiskTier::High)
        .count();

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
    eprintln!(
        "  Change edges:     {} ({:.0}% module coverage)",
        metrics.change_edges,
        metrics.cochange_coverage * 100.0
    );
    eprintln!("  Defect edges:     {}", metrics.defect_edges);
    eprintln!("  Cycles:           {}", metrics.cycle_count);
    eprintln!("  Signals:          {}", signals.len());
    let health_grade = risk_field
        .health
        .as_ref()
        .map(|h| format!(" health={}", h.grade))
        .unwrap_or_default();
    eprintln!(
        "  Risk analysis:    {} modules + {} functions ({} critical, {} high, converged={} in {} iter){}",
        module_risk_count,
        func_risk_count,
        critical_count,
        high_count,
        risk_field.converged,
        risk_field.iterations,
        health_grade,
    );

    if metrics.cochange_coverage < 0.10 && metrics.total_nodes > 50 {
        eprintln!();
        eprintln!(
            "  ⚠ Low co-change coverage ({:.0}%). Signals depending on temporal data may be sparse.",
            metrics.cochange_coverage * 100.0
        );
        eprintln!(
            "    Try: increase --depth, widen time_window, or lower min_co_changes in config."
        );
    }

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
        println!(
            "Structural Dependencies (fan-out: {}):",
            impact.structural_deps.len()
        );
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
            println!("  [{:.2}] {}{node_b}", signal.severity, signal.signal_type);
        }
    }

    Ok(0)
}

fn cmd_hotspots(args: HotspotsArgs) -> Result<i32> {
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;
    let mut hotspots = db.get_hotspots(args.top)?;
    if args.exclude_tests {
        hotspots.retain(|(id, _, _, _)| !is_test_file(id));
    }

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
        ExportFormat::Dot => db.get_dot_export()?,
        ExportFormat::Mermaid => db.get_mermaid_export()?,
        ExportFormat::VizJson => {
            let viz = db.get_viz_export()?;
            serde_json::to_string_pretty(&viz)?
        }
    };

    if let Some(path) = args.output {
        std::fs::write(&path, &output)?;
        eprintln!("Exported to {}", path.display());
    } else {
        println!("{output}");
    }

    Ok(0)
}

fn cmd_safety(args: SafetyArgs) -> Result<i32> {
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;

    let mut results = if let Some(zone) = &args.zone {
        db.get_nodes_by_zone(zone)?
    } else {
        db.get_safety_ranking(args.top)?
    };
    if args.exclude_tests {
        results.retain(|r| !is_test_file(&r.node_id));
    }
    results.truncate(args.top);

    if results.is_empty() {
        eprintln!("No risk data found. Run `ising build` first.");
        return Ok(1);
    }

    match args.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        OutputFormat::Text => {
            let title = if let Some(zone) = &args.zone {
                format!("Risk Analysis — filter: {zone}")
            } else {
                format!("Risk Analysis — top {}", args.top)
            };
            println!("{title}");
            println!("{}", "═".repeat(86));
            println!(
                "  {:>4}  {:<45} {:>7} {:>5} {:>5} {:>10}",
                "Rank", "File", "Direct", "Cap", "P%", "Tier"
            );
            println!("{}", "─".repeat(86));
            for (i, s) in results.iter().enumerate() {
                let test_tag = if is_test_file(&s.node_id) {
                    " [TEST]"
                } else {
                    ""
                };
                println!(
                    "  {:>4}  {:<45} {:>7.2} {:>5.2} {:>5.1} {:>10}{}",
                    i + 1,
                    truncate_path(&s.node_id, 45),
                    s.direct_score,
                    s.capacity,
                    s.percentile,
                    s.risk_tier.to_uppercase(),
                    test_tag,
                );
            }
        }
    }

    Ok(0)
}

fn cmd_health(args: HealthArgs) -> Result<i32> {
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;

    let health = match db.get_health()? {
        Some(h) => h,
        None => {
            eprintln!("No health data found. Run `ising build` first.");
            return Ok(1);
        }
    };

    match args.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&health)?);
        }
        OutputFormat::Text => {
            println!("Repository Health");
            println!("{}", "═".repeat(50));
            println!(
                "  Grade:              {} ({:.0}%)",
                health.grade,
                health.score * 100.0
            );
            println!(
                "  Active modules:     {} / {}",
                health.active_modules, health.total_modules
            );
            println!();
            println!("  Safety zone distribution (active modules):");
            println!(
                "    Stable  (SF>3.0):  {:>5.1}%",
                health.frac_stable * 100.0
            );
            println!(
                "    Healthy (SF 2-3):  {:>5.1}%",
                health.frac_healthy * 100.0
            );
            println!(
                "    Warning (SF 1.5-2):{:>5.1}%",
                health.frac_warning * 100.0
            );
            println!(
                "    Danger  (SF 1-1.5):{:>5.1}%",
                health.frac_danger * 100.0
            );
            println!(
                "    Critical(SF<1.0):  {:>5.1}%",
                health.frac_critical * 100.0
            );
            println!();
            println!("  Scoring breakdown:");
            println!(
                "    Zone score:        {:.3}  (weighted zone fractions)",
                health.zone_sub_score
            );
            println!(
                "    Coupling modifier: {:.3}  (λ_max={:.1})",
                health.coupling_modifier, health.lambda_max
            );
            println!(
                "    Signal penalty:   -{:.3}  (architectural signals)",
                health.signal_penalty
            );
            println!();
            if health.signal_density > 0.0 {
                println!(
                    "  Signal density:     {:.3} signals/module",
                    health.signal_density
                );
                if health.god_module_density > 0.0 {
                    println!(
                        "    God modules:      {:.1}%",
                        health.god_module_density * 100.0
                    );
                }
                if health.cycle_density > 0.0 {
                    println!("    Dep cycles:       {:.1}%", health.cycle_density * 100.0);
                }
                if health.unstable_dep_density > 0.0 {
                    println!(
                        "    Unstable deps:    {:.1}%",
                        health.unstable_dep_density * 100.0
                    );
                }
                println!();
            }
            println!("  Risk tiers:");
            println!("    Critical (top 1%): {}", health.critical_count);
            println!("    High (top 5%):     {}", health.high_count);
            if !health.caveats.is_empty() {
                println!();
                println!("  Caveats:");
                for caveat in &health.caveats {
                    println!("    * {}", caveat);
                }
            }
        }
    }

    Ok(0)
}

fn cmd_simulate(args: SimulateArgs) -> Result<i32> {
    let config = Config::load_or_default(&args.config);
    let db = Database::open(args.db.to_str().unwrap_or("ising.db"))?;

    // Reconstruct graph from DB
    let graph = db.load_graph()?;
    if graph.node_count() == 0 {
        eprintln!("No graph data found. Run `ising build` first.");
        return Ok(1);
    }

    // Parse load case
    let load_case = if args.target.ends_with(".json") {
        let content = std::fs::read_to_string(&args.target)?;
        serde_json::from_str::<LoadCase>(&content)?
    } else {
        stress::single_file_change(&graph, &args.target)
    };

    // Compute baseline and loaded risk fields
    let baseline = stress::compute_risk_field(&graph, &config, None, None, None);
    let loaded = stress::simulate_load_case(&graph, &config, &load_case);
    let delta = stress::compare_risk_fields(&baseline, &loaded);

    match args.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&delta)?);
        }
        OutputFormat::Text => {
            println!("Load Case Simulation: {}", load_case.name);
            println!("{}", "═".repeat(86));
            println!(
                "  {:<40} {:>7} {:>7} {:>6} {:>6} {:>12}",
                "File", "Risk(b)", "Risk(a)", "SF(b)", "SF(a)", "Zone→"
            );
            println!("{}", "─".repeat(86));

            let shown: Vec<_> = delta
                .deltas
                .iter()
                .filter(|d| (d.safety_factor_before - d.safety_factor_after).abs() > 0.001)
                .take(20)
                .collect();

            if shown.is_empty() {
                println!("  No significant risk changes detected.");
            } else {
                for d in &shown {
                    let zone_change = if d.zone_before != d.zone_after {
                        format!("{} → {}", d.zone_before, d.zone_after)
                    } else {
                        format!("{}", d.zone_after)
                    };
                    println!(
                        "  {:<40} {:>7.2} {:>7.2} {:>6.2} {:>6.2} {:>12}",
                        truncate_path(&d.node_id, 40),
                        d.risk_before,
                        d.risk_after,
                        d.safety_factor_before,
                        d.safety_factor_after,
                        zone_change,
                    );
                }
            }
        }
    }

    Ok(0)
}

/// Truncate a path string to fit in a column width (Unicode-safe).
fn truncate_path(path: &str, max_len: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_len {
        path.to_string()
    } else {
        let keep = max_len.saturating_sub(1);
        if keep == 0 {
            return "\u{2026}".to_string();
        }
        let start = char_count - keep;
        let byte_offset = path.char_indices().nth(start).map(|(i, _)| i).unwrap();
        format!("\u{2026}{}", &path[byte_offset..])
    }
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

fn cmd_boundaries(args: BoundariesArgs) -> Result<i32> {
    let repo_path = args.repo_path.canonicalize()?;

    // We need node IDs to detect boundaries — scan source files
    let ignore = ising_core::ignore::IgnoreRules::load(&repo_path);
    let graph = ising_builders::structural::build_structural_graph(&repo_path, &ignore)?;

    let module_ids: Vec<String> = graph
        .node_ids()
        .filter(|id| {
            graph
                .get_node(id)
                .is_some_and(|n| n.node_type == NodeType::Module)
        })
        .map(|s| s.to_string())
        .collect();
    let module_id_refs: Vec<&str> = module_ids.iter().map(|s| s.as_str()).collect();
    let boundaries = BoundaryStructure::detect(&repo_path, &module_id_refs);

    if args.format == OutputFormat::Json {
        let json = serde_json::to_string_pretty(&boundaries)?;
        println!("{json}");
        return Ok(0);
    }

    let source_desc = match &boundaries.l1_source {
        ising_core::boundary::BoundarySource::Manifest { ecosystem } => {
            format!("Manifest ({ecosystem})")
        }
        ising_core::boundary::BoundarySource::Directory => "Directory fallback".to_string(),
        ising_core::boundary::BoundarySource::SingleRoot => "Single root".to_string(),
    };

    println!("Boundary source: {}", source_desc);
    println!(
        "Packages: {}  Modules: {}",
        boundaries.packages.len(),
        boundaries.module_count()
    );
    println!();

    for pkg in &boundaries.packages {
        println!("  {}  ({} modules)", pkg.id, pkg.modules.len());
        for module in &pkg.modules {
            println!(
                "    {}  {} files  [{:?}]",
                module.id,
                module.members.len(),
                module.detection,
            );
        }
    }

    if !boundaries.uncategorized.is_empty() {
        println!();
        println!("  _uncategorized  {} files", boundaries.uncategorized.len());
    }

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
        let cli = Cli::try_parse_from([
            "ising",
            "signals",
            "--type",
            "ghost_coupling",
            "--min-severity",
            "0.5",
        ])
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

    #[test]
    fn safety_command_parses() {
        let cli = Cli::try_parse_from(["ising", "safety", "--top", "10"]).unwrap();
        assert!(matches!(cli.command, Commands::Safety(_)));
    }

    #[test]
    fn safety_command_with_zone_parses() {
        let cli =
            Cli::try_parse_from(["ising", "safety", "--zone", "critical", "--format", "json"])
                .unwrap();
        assert!(matches!(cli.command, Commands::Safety(_)));
    }

    #[test]
    fn simulate_command_parses() {
        let cli = Cli::try_parse_from(["ising", "simulate", "src/main.rs"]).unwrap();
        assert!(matches!(cli.command, Commands::Simulate(_)));
    }

    #[test]
    fn health_command_parses() {
        let cli = Cli::try_parse_from(["ising", "health"]).unwrap();
        assert!(matches!(cli.command, Commands::Health(_)));
    }

    #[test]
    fn health_command_json_parses() {
        let cli = Cli::try_parse_from(["ising", "health", "--format", "json"]).unwrap();
        assert!(matches!(cli.command, Commands::Health(_)));
    }
}
