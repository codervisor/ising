//! Module boundary detection and classification.
//!
//! Implements a resolution chain to detect module boundaries:
//! 1. Build manifest detection (Cargo.toml, package.json, go.mod, etc.)
//! 2. Language module systems (__init__.py, Go package dirs, barrel files)
//! 3. Directory fallback
//!
//! All strategies produce a unified `BoundaryStructure` that downstream
//! code (signals, propagation, health index) uses to classify pairs as
//! same-module, cross-module, or cross-package.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// How Level 1 (package) boundaries were detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundarySource {
    /// Auto-detected from build manifests (Cargo.toml, package.json, etc.)
    Manifest { ecosystem: String },
    /// Fallback: directory prefix grouping (reserved for future L1 directory detection)
    Directory,
    /// Single root (no workspace detected)
    SingleRoot,
}

/// How a Level 2 (intra-package) module was detected.
///
/// Note: Current implementation uses directory-based grouping for all languages,
/// labeled by the dominant language. Future work: validate Python directories
/// contain `__init__.py`, check TS/JS directories for barrel files (`index.ts`),
/// parse Rust `mod` declarations, etc. See spec 046 Phase 1.3 for the full plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleDetection {
    /// Python directory (future: validate `__init__.py` presence)
    PythonPackage,
    /// Go package directory (each dir with .go files)
    GoPackage,
    /// Java package directory
    JavaPackage,
    /// TS/JS directory (future: validate barrel file presence)
    BarrelFile,
    /// C# directory with .cs files
    CSharpNamespace,
    /// Directory grouping fallback (no language-specific detection)
    Directory,
}

/// Three-level classification for signal severity scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossingType {
    /// Both nodes in same L2 module — co-change is expected
    SameModule,
    /// Same L1 package, different L2 modules — moderate concern
    CrossModule,
    /// Different L1 packages — highest concern
    CrossPackage,
}

/// Information about a detected package (Level 1 boundary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub id: String,
    pub root_path: String,
    pub modules: Vec<ModuleInfo>,
}

/// Information about a module within a package (Level 2 boundary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub id: String,
    pub members: Vec<String>,
    pub detection: ModuleDetection,
}

/// Unified boundary structure produced by all detection strategies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryStructure {
    /// How Level 1 boundaries were detected.
    pub l1_source: BoundarySource,

    /// Level 1: workspace / package boundaries.
    pub packages: Vec<PackageInfo>,

    /// Map from node_id → (package_id, module_id).
    pub assignments: HashMap<String, (String, String)>,

    /// Files not assigned to any module.
    pub uncategorized: Vec<String>,
}

impl BoundaryStructure {
    /// Detect boundaries for a repository using the resolution chain.
    ///
    /// Resolution chain:
    /// L1: workspace manifest → single root
    /// L2: language module system → directory fallback
    pub fn detect(repo_root: &Path, node_ids: &[&str]) -> Self {
        // Level 1: try workspace manifests
        let (l1_source, mut packages) =
            if let Some((ecosystem, pkgs)) = detect_workspace_manifests(repo_root, node_ids) {
                (
                    BoundarySource::Manifest {
                        ecosystem: ecosystem.to_string(),
                    },
                    pkgs,
                )
            } else {
                // Single root: the whole repo is one package
                (
                    BoundarySource::SingleRoot,
                    vec![PackageInfo {
                        id: "_root".to_string(),
                        root_path: String::new(),
                        modules: Vec::new(),
                    }],
                )
            };

        // Level 2: detect intra-package modules for each package
        for pkg in &mut packages {
            let pkg_members: Vec<&str> = node_ids
                .iter()
                .copied()
                .filter(|id| node_belongs_to_package(id, &pkg.root_path, &l1_source))
                .collect();
            pkg.modules = detect_intra_package_modules(&pkg_members, &pkg.root_path);
        }

        // Build assignments map using a reverse index from module members.
        // This avoids O(N × modules × members) scanning.
        let mut assignments = HashMap::new();
        let mut uncategorized = Vec::new();

        // First pass: build reverse index from member → (pkg_id, module_id)
        let mut member_index: HashMap<&str, (String, String)> = HashMap::new();
        for pkg in &packages {
            for module in &pkg.modules {
                for member in &module.members {
                    member_index.insert(member.as_str(), (pkg.id.clone(), module.id.clone()));
                }
            }
        }

        // Second pass: assign each node using the index
        for node_id in node_ids {
            if let Some((pkg_id, mod_id)) = member_index.get(node_id) {
                assignments.insert(node_id.to_string(), (pkg_id.clone(), mod_id.clone()));
            } else {
                // Not in any module — check if it belongs to a package root
                let mut assigned = false;
                for pkg in &packages {
                    if node_belongs_to_package(node_id, &pkg.root_path, &l1_source) {
                        assignments
                            .insert(node_id.to_string(), (pkg.id.clone(), "_root".to_string()));
                        assigned = true;
                        break;
                    }
                }
                if !assigned {
                    uncategorized.push(node_id.to_string());
                    assignments.insert(
                        node_id.to_string(),
                        ("_uncategorized".to_string(), "_uncategorized".to_string()),
                    );
                }
            }
        }

        BoundaryStructure {
            l1_source,
            packages,
            assignments,
            uncategorized,
        }
    }

    /// Are two nodes in the same Level 2 module?
    ///
    /// Returns false if either node is not assigned to any module,
    /// so unknown/uncategorized nodes are never treated as same-module.
    pub fn same_module(&self, a: &str, b: &str) -> bool {
        matches!(
            (self.assignments.get(a), self.assignments.get(b)),
            (Some(left), Some(right)) if left == right
        )
    }

    /// Are two nodes in the same Level 1 package?
    ///
    /// Returns false if either node is not assigned.
    pub fn same_package(&self, a: &str, b: &str) -> bool {
        matches!(
            (self.assignments.get(a), self.assignments.get(b)),
            (Some((pkg_a, _)), Some((pkg_b, _))) if pkg_a == pkg_b
        )
    }

    /// Classify the boundary crossing type for a pair of nodes.
    pub fn crossing_type(&self, a: &str, b: &str) -> CrossingType {
        if self.same_module(a, b) {
            CrossingType::SameModule
        } else if self.same_package(a, b) {
            CrossingType::CrossModule
        } else {
            CrossingType::CrossPackage
        }
    }

    /// Get the (package_id, module_id) for a node.
    pub fn module_of(&self, node_id: &str) -> (&str, &str) {
        self.assignments
            .get(node_id)
            .map(|(p, m)| (p.as_str(), m.as_str()))
            .unwrap_or(("_uncategorized", "_uncategorized"))
    }

    /// Does a pair cross any boundary (module or package)?
    pub fn crosses_boundary(&self, a: &str, b: &str) -> bool {
        !self.same_module(a, b)
    }

    /// Get the number of detected modules across all packages.
    pub fn module_count(&self) -> usize {
        self.packages.iter().map(|p| p.modules.len()).sum()
    }
}

/// Severity multiplier based on boundary crossing type.
pub fn severity_multiplier(crossing: &CrossingType) -> f64 {
    match crossing {
        CrossingType::SameModule => 0.0,   // suppress
        CrossingType::CrossModule => 1.0,  // normal
        CrossingType::CrossPackage => 2.0, // elevated
    }
}

// ============================================================================
// Level 1: Workspace manifest detection
// ============================================================================

/// Try to detect workspace-level packages from build manifests.
/// Returns (ecosystem_name, packages) if found.
fn detect_workspace_manifests(
    repo_root: &Path,
    node_ids: &[&str],
) -> Option<(&'static str, Vec<PackageInfo>)> {
    type Detector = fn(&Path, &[&str]) -> Option<Vec<PackageInfo>>;
    let detectors: &[(&str, Detector)] = &[
        ("cargo-workspace", detect_cargo_workspace),
        ("js-workspace", detect_js_workspaces),
        ("go-modules", detect_go_modules),
        ("python-packages", detect_python_packages),
        ("jvm-modules", detect_jvm_modules),
        ("dotnet-solution", detect_dotnet_projects),
    ];

    for &(name, detector) in detectors {
        if let Some(pkgs) = detector(repo_root, node_ids)
            && pkgs.len() > 1
        {
            return Some((name, pkgs));
        }
    }

    None
}

/// Detect Cargo workspace members from Cargo.toml.
fn detect_cargo_workspace(repo_root: &Path, _node_ids: &[&str]) -> Option<Vec<PackageInfo>> {
    let cargo_toml = repo_root.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml).ok()?;

    // Parse TOML to find workspace.members
    let table: toml::Table = content.parse().ok()?;
    let workspace = table.get("workspace")?.as_table()?;
    let members = workspace.get("members")?.as_array()?;

    let mut packages = Vec::new();
    for member in members {
        let member_str = member.as_str()?;
        // Handle glob patterns like "crates/*"
        if member_str.contains('*') {
            let pattern_base = member_str.trim_end_matches("/*").trim_end_matches("/*");
            let dir = repo_root.join(pattern_base);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() && entry.path().join("Cargo.toml").exists() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let root_path = format!("{}/{}", pattern_base, name);
                        packages.push(PackageInfo {
                            id: name,
                            root_path,
                            modules: Vec::new(),
                        });
                    }
                }
            }
        } else {
            packages.push(PackageInfo {
                id: member_str.to_string(),
                root_path: member_str.to_string(),
                modules: Vec::new(),
            });
        }
    }

    if packages.is_empty() {
        None
    } else {
        Some(packages)
    }
}

/// Detect JS/TS workspaces (package.json workspaces or pnpm-workspace.yaml).
fn detect_js_workspaces(repo_root: &Path, _node_ids: &[&str]) -> Option<Vec<PackageInfo>> {
    // Try pnpm-workspace.yaml first
    let pnpm_ws = repo_root.join("pnpm-workspace.yaml");
    if let Ok(content) = std::fs::read_to_string(&pnpm_ws) {
        let pkgs = parse_pnpm_workspace_packages(&content, repo_root);
        if !pkgs.is_empty() {
            return Some(pkgs);
        }
    }

    // Try package.json workspaces
    let pkg_json = repo_root.join("package.json");
    if let Ok(content) = std::fs::read_to_string(&pkg_json)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(workspaces) = json.get("workspaces")
    {
        let patterns = match workspaces {
            serde_json::Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            serde_json::Value::Object(obj) => obj
                .get("packages")
                .and_then(|p| p.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let pkgs = resolve_workspace_globs(&patterns, repo_root);
        if !pkgs.is_empty() {
            return Some(pkgs);
        }
    }

    None
}

/// Parse pnpm-workspace.yaml to extract package patterns.
fn parse_pnpm_workspace_packages(content: &str, repo_root: &Path) -> Vec<PackageInfo> {
    // Simple line-by-line parser for the packages section
    let mut in_packages = false;
    let mut patterns = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "packages:" {
            in_packages = true;
            continue;
        }
        if in_packages {
            if let Some(rest) = trimmed.strip_prefix("- ") {
                let pattern = rest.trim().trim_matches('\'').trim_matches('"');
                patterns.push(pattern.to_string());
            } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                break; // End of packages section
            }
        }
    }

    resolve_workspace_globs(&patterns, repo_root)
}

/// Resolve workspace glob patterns (like "packages/*") to actual directories.
fn resolve_workspace_globs(patterns: &[String], repo_root: &Path) -> Vec<PackageInfo> {
    let mut packages = Vec::new();

    for pattern in patterns {
        if pattern.ends_with("/*") || pattern.ends_with("/**") {
            let base = pattern.trim_end_matches("/**").trim_end_matches("/*");
            let dir = repo_root.join(base);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with('.') {
                            continue;
                        }
                        let root_path = format!("{}/{}", base, name);
                        packages.push(PackageInfo {
                            id: name,
                            root_path,
                            modules: Vec::new(),
                        });
                    }
                }
            }
        } else if !pattern.contains('*') {
            // Literal directory
            let dir = repo_root.join(pattern);
            if dir.is_dir() {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| pattern.clone());
                packages.push(PackageInfo {
                    id: name,
                    root_path: pattern.clone(),
                    modules: Vec::new(),
                });
            }
        }
    }

    packages
}

/// Detect Go modules (go.work or multiple go.mod files).
fn detect_go_modules(repo_root: &Path, _node_ids: &[&str]) -> Option<Vec<PackageInfo>> {
    // Try go.work first
    let go_work = repo_root.join("go.work");
    if let Ok(content) = std::fs::read_to_string(&go_work) {
        let mut packages = Vec::new();
        let mut in_use = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "use (" {
                in_use = true;
                continue;
            }
            if trimmed == ")" {
                in_use = false;
                continue;
            }
            if in_use || trimmed.starts_with("use ") {
                let path = trimmed.strip_prefix("use ").map_or(trimmed, str::trim);
                let path = path.trim_matches('"').trim_matches('\'');
                if !path.is_empty() && path != "." {
                    let name = path.rsplit('/').next().unwrap_or(path);
                    packages.push(PackageInfo {
                        id: name.to_string(),
                        root_path: path.to_string(),
                        modules: Vec::new(),
                    });
                }
            }
        }
        if !packages.is_empty() {
            return Some(packages);
        }
    }

    // Look for multiple go.mod files (monorepo pattern)
    let mut packages = Vec::new();
    scan_for_go_mods(repo_root, repo_root, &mut packages, 0);
    if packages.len() > 1 {
        Some(packages)
    } else {
        None
    }
}

fn scan_for_go_mods(root: &Path, dir: &Path, packages: &mut Vec<PackageInfo>, depth: usize) {
    if depth > 4 {
        return;
    }
    if dir.join("go.mod").exists() && dir != root {
        let rel = dir
            .strip_prefix(root)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| rel.clone());
        packages.push(PackageInfo {
            id: name,
            root_path: rel,
            modules: Vec::new(),
        });
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.')
                    || name == "vendor"
                    || name == "node_modules"
                    || name == "testdata"
                {
                    continue;
                }
                scan_for_go_mods(root, &path, packages, depth + 1);
            }
        }
    }
}

/// Detect Python multi-package layouts (libs/, packages/, or src/ with multiple dirs).
fn detect_python_packages(repo_root: &Path, _node_ids: &[&str]) -> Option<Vec<PackageInfo>> {
    // Check for libs/ or packages/ directory with multiple pyproject.toml
    for subdir in &["libs", "packages"] {
        let dir = repo_root.join(subdir);
        if dir.is_dir() {
            let mut packages = Vec::new();
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir()
                        && (path.join("pyproject.toml").exists() || path.join("setup.py").exists())
                    {
                        let name = entry.file_name().to_string_lossy().to_string();
                        packages.push(PackageInfo {
                            id: name.clone(),
                            root_path: format!("{}/{}", subdir, name),
                            modules: Vec::new(),
                        });
                    }
                }
            }
            if packages.len() > 1 {
                return Some(packages);
            }
        }
    }
    None
}

/// Detect JVM modules (Maven pom.xml modules, Gradle settings).
fn detect_jvm_modules(repo_root: &Path, _node_ids: &[&str]) -> Option<Vec<PackageInfo>> {
    // Check settings.gradle(.kts) for include/subprojects
    for settings_file in &["settings.gradle", "settings.gradle.kts"] {
        let path = repo_root.join(settings_file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let pkgs = parse_gradle_settings(&content);
            if pkgs.len() > 1 {
                return Some(pkgs);
            }
        }
    }

    // Check pom.xml for <modules>
    let pom = repo_root.join("pom.xml");
    if let Ok(content) = std::fs::read_to_string(&pom) {
        let pkgs = parse_maven_modules(&content);
        if pkgs.len() > 1 {
            return Some(pkgs);
        }
    }

    None
}

fn parse_gradle_settings(content: &str) -> Vec<PackageInfo> {
    let mut packages = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        // Match: include("module-name") or include ':module-name' etc.
        if trimmed.starts_with("include") {
            let after = trimmed.trim_start_matches("include");
            // Extract module names from various formats
            for part in after.split(',') {
                let name = part.trim().trim_matches(|c: char| {
                    c == '(' || c == ')' || c == '\'' || c == '"' || c == ':' || c.is_whitespace()
                });
                if !name.is_empty() {
                    // Convert Gradle notation (":module:submodule") to path
                    let root_path = name.replace(':', "/").trim_start_matches('/').to_string();
                    let id = root_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&root_path)
                        .to_string();
                    packages.push(PackageInfo {
                        id,
                        root_path,
                        modules: Vec::new(),
                    });
                }
            }
        }
    }
    packages
}

fn parse_maven_modules(content: &str) -> Vec<PackageInfo> {
    let mut packages = Vec::new();
    // Simple XML extraction for <module>name</module>
    let mut in_modules = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("<modules>") {
            in_modules = true;
            continue;
        }
        if trimmed.contains("</modules>") {
            in_modules = false;
            continue;
        }
        if in_modules
            && let Some(start) = trimmed.find("<module>")
            && let Some(end) = trimmed.find("</module>")
        {
            let name = &trimmed[start + 8..end];
            packages.push(PackageInfo {
                id: name.to_string(),
                root_path: name.to_string(),
                modules: Vec::new(),
            });
        }
    }
    packages
}

/// Detect .NET solution projects.
fn detect_dotnet_projects(repo_root: &Path, _node_ids: &[&str]) -> Option<Vec<PackageInfo>> {
    // Find .sln files
    let entries = std::fs::read_dir(repo_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "sln")
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            let pkgs = parse_sln_projects(&content);
            if pkgs.len() > 1 {
                return Some(pkgs);
            }
        }
    }
    None
}

fn parse_sln_projects(content: &str) -> Vec<PackageInfo> {
    let mut packages = Vec::new();
    for line in content.lines() {
        // Project("{...}") = "Name", "Path\Name.csproj", "{...}"
        if line.starts_with("Project(") && line.contains(".csproj") {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 6 {
                let name = parts[3];
                let proj_path = parts[5].replace('\\', "/");
                let root_path = proj_path
                    .rsplit_once('/')
                    .map(|(dir, _)| dir.to_string())
                    .unwrap_or_default();
                packages.push(PackageInfo {
                    id: name.to_string(),
                    root_path,
                    modules: Vec::new(),
                });
            }
        }
    }
    packages
}

// ============================================================================
// Level 2: Intra-package module detection
// ============================================================================

/// Check if a node belongs to a package based on its file path prefix.
///
/// Uses `Path::starts_with` to ensure component-boundary matching:
/// "pkg" matches "pkg/foo.rs" but not "pkg2/foo.rs" or "pkg-old/foo.rs".
fn node_belongs_to_package(node_id: &str, root_path: &str, _source: &BoundarySource) -> bool {
    if root_path.is_empty() {
        // Single root: everything belongs
        return true;
    }
    let node_path = Path::new(node_id);
    let root = Path::new(root_path);
    node_path == root || node_path.starts_with(root)
}

/// Detect Level 2 modules within a single package.
fn detect_intra_package_modules(members: &[&str], package_root: &str) -> Vec<ModuleInfo> {
    if members.is_empty() {
        return Vec::new();
    }

    // Determine dominant language from file extensions
    let lang = detect_dominant_language(members);

    // Group files by their module directory
    let mut dir_groups: HashMap<String, Vec<String>> = HashMap::new();

    for &member in members {
        let rel = strip_package_prefix(member, package_root);
        let module_dir = match lang.as_deref() {
            Some("python") => python_module_dir(rel),
            Some("go") => go_module_dir(rel),
            Some("java") => java_module_dir(rel),
            Some("typescript") | Some("javascript") => ts_module_dir(rel),
            Some("csharp") => csharp_module_dir(rel),
            _ => directory_module_dir(rel),
        };
        dir_groups
            .entry(module_dir)
            .or_default()
            .push(member.to_string());
    }

    let detection = match lang.as_deref() {
        Some("python") => ModuleDetection::PythonPackage,
        Some("go") => ModuleDetection::GoPackage,
        Some("java") => ModuleDetection::JavaPackage,
        Some("typescript") | Some("javascript") => ModuleDetection::BarrelFile,
        Some("csharp") => ModuleDetection::CSharpNamespace,
        _ => ModuleDetection::Directory,
    };

    let mut modules: Vec<ModuleInfo> = dir_groups
        .into_iter()
        .map(|(dir, members)| ModuleInfo {
            id: if dir.is_empty() {
                "_root".to_string()
            } else {
                dir
            },
            members,
            detection: detection.clone(),
        })
        .collect();

    modules.sort_by(|a, b| a.id.cmp(&b.id));
    modules
}

/// Strip the package root prefix from a node path.
fn strip_package_prefix<'a>(node_id: &'a str, package_root: &str) -> &'a str {
    if package_root.is_empty() {
        return node_id;
    }
    node_id
        .strip_prefix(package_root)
        .unwrap_or(node_id)
        .trim_start_matches('/')
}

/// Detect the dominant language from file extensions.
fn detect_dominant_language(members: &[&str]) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for &m in members {
        let ext = m.rsplit('.').next().unwrap_or("");
        let lang = match ext {
            "py" => "python",
            "go" => "go",
            "java" => "java",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "rs" => "rust",
            "cs" => "csharp",
            "kt" | "kts" => "kotlin",
            "rb" => "ruby",
            "php" => "php",
            "c" | "h" => "c",
            "cpp" | "cc" | "cxx" | "hpp" => "cpp",
            "vue" => "vue",
            _ => continue,
        };
        *counts.entry(lang).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(lang, _)| lang.to_string())
}

/// Python: group by directory containing __init__.py (sub-package).
/// Files in the same directory are in the same module.
fn python_module_dir(rel_path: &str) -> String {
    // e.g. "flask/json/provider.py" → "flask/json"
    // e.g. "flask/app.py" → "flask"
    // e.g. "app.py" → "" (root)
    rel_path
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

/// Go: each directory is a package.
fn go_module_dir(rel_path: &str) -> String {
    rel_path
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

/// Java: package directories under src/main/java (or similar).
fn java_module_dir(rel_path: &str) -> String {
    // Strip src/main/java or similar prefix
    let stripped = rel_path
        .strip_prefix("src/main/java/")
        .or_else(|| rel_path.strip_prefix("src/main/kotlin/"))
        .or_else(|| rel_path.strip_prefix("src/"))
        .unwrap_or(rel_path);
    stripped
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

/// TypeScript/JavaScript: group by directory.
fn ts_module_dir(rel_path: &str) -> String {
    rel_path
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

/// C#: each directory with .cs files.
fn csharp_module_dir(rel_path: &str) -> String {
    rel_path
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

/// Fallback: group by parent directory path.
///
/// Files in the same directory are grouped together. Root-level files
/// (no directory component) go into the default module.
fn directory_module_dir(rel_path: &str) -> String {
    rel_path
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crossing_type_same_module() {
        let mut bs = BoundaryStructure {
            l1_source: BoundarySource::SingleRoot,
            packages: vec![],
            assignments: HashMap::new(),
            uncategorized: vec![],
        };
        bs.assignments
            .insert("a.py".to_string(), ("pkg".to_string(), "mod1".to_string()));
        bs.assignments
            .insert("b.py".to_string(), ("pkg".to_string(), "mod1".to_string()));
        bs.assignments
            .insert("c.py".to_string(), ("pkg".to_string(), "mod2".to_string()));
        bs.assignments
            .insert("d.py".to_string(), ("pkg2".to_string(), "mod3".to_string()));

        assert_eq!(bs.crossing_type("a.py", "b.py"), CrossingType::SameModule);
        assert_eq!(bs.crossing_type("a.py", "c.py"), CrossingType::CrossModule);
        assert_eq!(bs.crossing_type("a.py", "d.py"), CrossingType::CrossPackage);
    }

    #[test]
    fn test_severity_multiplier() {
        assert_eq!(severity_multiplier(&CrossingType::SameModule), 0.0);
        assert_eq!(severity_multiplier(&CrossingType::CrossModule), 1.0);
        assert_eq!(severity_multiplier(&CrossingType::CrossPackage), 2.0);
    }

    #[test]
    fn test_python_module_dir() {
        assert_eq!(python_module_dir("flask/json/provider.py"), "flask/json");
        assert_eq!(python_module_dir("flask/app.py"), "flask");
        assert_eq!(python_module_dir("app.py"), "");
    }

    #[test]
    fn test_go_module_dir() {
        assert_eq!(go_module_dir("binding/json.go"), "binding");
        assert_eq!(go_module_dir("gin.go"), "");
    }

    #[test]
    fn test_detect_dominant_language() {
        let members = &["src/a.py", "src/b.py", "src/c.py", "README.md"];
        assert_eq!(
            detect_dominant_language(members),
            Some("python".to_string())
        );

        let members = &["cmd/main.go", "internal/server.go"];
        assert_eq!(detect_dominant_language(members), Some("go".to_string()));
    }

    #[test]
    fn test_detect_intra_package_modules() {
        let members = &[
            "flask/__init__.py",
            "flask/app.py",
            "flask/blueprints.py",
            "flask/json/__init__.py",
            "flask/json/provider.py",
            "flask/sansio/__init__.py",
            "flask/sansio/app.py",
        ];
        let modules = detect_intra_package_modules(members, "");
        assert!(
            modules.len() >= 3,
            "Expected at least 3 modules for Flask layout, got {}",
            modules.len()
        );

        // Check that files in same directory are grouped
        let flask_mod = modules.iter().find(|m| m.id == "flask").unwrap();
        assert!(flask_mod.members.contains(&"flask/__init__.py".to_string()));
        assert!(flask_mod.members.contains(&"flask/app.py".to_string()));
    }

    #[test]
    fn test_boundary_structure_detect_from_node_ids() {
        // Test with simple node IDs (no actual filesystem)
        let node_ids = &[
            "src/auth/login.py",
            "src/auth/register.py",
            "src/api/routes.py",
            "src/api/middleware.py",
        ];
        // Without filesystem, detect will use SingleRoot + directory fallback
        let bs = BoundaryStructure::detect(Path::new("/nonexistent"), node_ids);

        // Same-directory files should be in the same module
        assert_eq!(
            bs.crossing_type("src/auth/login.py", "src/auth/register.py"),
            CrossingType::SameModule
        );
        // Different directories should be cross-module
        assert_eq!(
            bs.crossing_type("src/auth/login.py", "src/api/routes.py"),
            CrossingType::CrossModule
        );
    }

    #[test]
    fn test_parse_gradle_settings() {
        let content = r#"
rootProject.name = 'spring-boot'
include 'spring-boot-project:spring-boot'
include 'spring-boot-project:spring-boot-autoconfigure'
include 'spring-boot-project:spring-boot-tools:spring-boot-maven-plugin'
"#;
        let pkgs = parse_gradle_settings(content);
        assert!(
            pkgs.len() >= 3,
            "Expected at least 3 packages, got {}",
            pkgs.len()
        );
    }

    #[test]
    fn test_parse_maven_modules() {
        let content = r#"
<project>
  <modules>
    <module>core</module>
    <module>api</module>
    <module>web</module>
  </modules>
</project>
"#;
        let pkgs = parse_maven_modules(content);
        assert_eq!(pkgs.len(), 3);
        assert_eq!(pkgs[0].id, "core");
    }
}
