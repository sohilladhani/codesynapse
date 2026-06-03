use crate::ui;
use clap::{CommandFactory, Parser, Subcommand};
use codesynapse_core::analyze::Analyzer;
use codesynapse_core::build::GraphBuilder;
use codesynapse_core::cache::FileCache;
use codesynapse_core::cluster::CommunityDetector;
use codesynapse_core::config::{CodeSynapseConfig, LlmConfig};
use codesynapse_core::detect::Detector;
use codesynapse_core::diagnostics::{
    diagnose_file, format_diagnostic_json, format_diagnostic_report,
};
use codesynapse_core::error::Result;
use codesynapse_core::export::Exporter;
use codesynapse_core::extract::Extractor;
use codesynapse_core::global_graph::{
    embed_global_graph, global_add, global_add_force, global_list, global_remove,
};
use codesynapse_core::graph::{GraphStore, MemoryGraphStore, SledGraphStore, StoreBackend};
use codesynapse_core::llm_extract::build_extractor;
use codesynapse_core::query::QueryEngine;
use codesynapse_core::ts_extract::{
    JsonPackageExtractor, McpConfigExtractor, TsBashExtractor, TsCExtractor, TsCSharpExtractor,
    TsCmakeExtractor, TsCppExtractor, TsDartExtractor, TsGoExtractor, TsGroovyExtractor,
    TsHaskellExtractor, TsJavaExtractor, TsJavaScriptExtractor, TsKotlinExtractor, TsLuaExtractor,
    TsPhpExtractor, TsPythonExtractor, TsRacketExtractor, TsRubyExtractor, TsRustExtractor,
    TsScalaExtractor, TsSqlExtractor, TsSvelteExtractor, TsSwiftExtractor, TsTypeScriptExtractor,
    TsVueExtractor, TsZigExtractor,
};
use codesynapse_core::types::FileType;
use codesynapse_core::types::{Edge, Node};
use codesynapse_core::watch::{WatchConfig, Watcher};
use console::style;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "codesynapse", version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Extract code structure from source files
    Extract {
        path: PathBuf,
        #[arg(long)]
        format: Option<String>,
    },
    /// Build graph from extraction fragments
    Build {
        path: PathBuf,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        #[arg(long, value_delimiter = ',')]
        format: Option<Vec<String>>,
        #[arg(long)]
        no_llm: bool,
        /// Enable LLM extraction for doc/paper files (provider from codesynapse.toml or env)
        #[arg(long)]
        llm: bool,
        #[arg(long)]
        code_only: bool,
        #[arg(long)]
        update: bool,
        #[arg(long)]
        force: bool,
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
    },
    /// Run community detection
    Cluster { path: PathBuf },
    /// Analyze the graph
    Analyze { path: PathBuf },
    /// Export graph in various formats
    Export {
        path: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        html: bool,
        #[arg(long)]
        graphml: bool,
        #[arg(long)]
        obsidian: bool,
    },
    /// Query the graph (BM25 + optional dense search via RRF)
    Query {
        query: String,
        /// Path to graph.json exported by the build command.
        #[arg(long)]
        graph: Option<PathBuf>,
        /// Path to model directory with tokenizer.json + model.safetensors.
        #[arg(long)]
        model_path: Option<PathBuf>,
        /// Traversal mode: bfs or dfs.
        #[arg(long, default_value = "bfs")]
        mode: String,
        /// Traversal depth.
        #[arg(long, default_value_t = 2)]
        depth: usize,
    },
    /// Find shortest path between two nodes
    Path {
        source: String,
        target: String,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Show graph statistics
    Stats { path: PathBuf },
    /// Validate extraction JSON
    Validate { path: PathBuf },
    /// Merge two graph JSON files
    Merge { a: PathBuf, b: PathBuf },
    /// Git merge driver for graph JSON files (union merge)
    #[command(name = "merge-driver")]
    MergeDriver {
        /// Base version (ignored — present for git merge driver %O %A %B compatibility)
        base: PathBuf,
        /// Current version (written back with merged result)
        current: PathBuf,
        /// Other version to merge in
        other: PathBuf,
    },
    /// Start MCP server (stdio transport for AI clients)
    Mcp {
        /// Override global registry directory (default: ~/.codesynapse)
        #[arg(long)]
        global_dir: Option<PathBuf>,
    },
    /// Start gRPC server
    Serve {
        #[arg(long)]
        graph: Option<PathBuf>,
        path: Option<PathBuf>,
        /// TCP port for gRPC listener (default: 50051)
        #[arg(long, default_value = "50051")]
        port: u16,
    },
    /// Diff two graphs for structural equivalence
    #[command(alias = "compare")]
    Diff {
        path: PathBuf,
        #[arg(long)]
        baseline: PathBuf,
    },
    /// Explain a node — show its details and relationships
    Explain {
        id: String,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Find nodes affected by changes to a given node
    Affected {
        id: String,
        #[arg(long)]
        path: Option<PathBuf>,
        /// Load graph from JSON file instead of sled store
        #[arg(long)]
        graph: Option<PathBuf>,
        #[arg(long, default_value = "2")]
        depth: usize,
        /// Filter to specific relation types (can be repeated)
        #[arg(long = "relation", value_name = "RELATION")]
        relations: Vec<String>,
    },
    /// Print an ASCII tree of the graph structure
    Tree {
        path: PathBuf,
        #[arg(long)]
        root: Option<String>,
        #[arg(long, default_value = "3")]
        max_depth: usize,
    },
    /// Benchmark the extraction pipeline
    Benchmark {
        path: PathBuf,
        #[arg(long, default_value = "3")]
        runs: usize,
    },
    /// Perform temporal risk analysis on the graph
    Risk {
        path: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    /// Incrementally update the graph from changed sources
    Update {
        path: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Watch a directory for changes and rebuild automatically
    Watch {
        #[arg(long, short = 'p')]
        path: Option<PathBuf>,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    /// Ingest content from a URL (web page, GitHub, paper, etc.)
    Ingest {
        url: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Manage git hooks for automatic graph building
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Initialize a new codesynapse.toml config in the current directory
    Init {
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    /// Generate shell completions
    Completions { shell: String },
    /// Save the last query or analysis result to a file
    SaveResult {
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    /// Install codesynapse for a specific platform (cursor, claude, etc.)
    Install {
        #[arg(long)]
        platform: String,
    },
    /// Manage Claude Desktop integration
    Claude {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage OpenCode integration
    Opencode {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Codex integration
    Codex {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage CodeBuddy integration
    Codebuddy {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Claw integration
    Claw {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Droid integration
    Droid {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Cursor integration
    Cursor {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage VS Code integration
    Vscode {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Aider integration
    Aider {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage GitHub Copilot integration
    Copilot {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Trae integration
    Trae {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Trae CN integration
    #[command(name = "trae-cn")]
    TraeCn {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Antigravity integration
    Antigravity {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Hermes integration
    Hermes {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Kiro integration
    Kiro {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Pi integration
    Pi {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage Devin integration
    Devin {
        #[command(subcommand)]
        action: PlatformAction,
    },
    /// Manage the cross-repo global graph (~/.codesynapse/)
    Global {
        #[command(subcommand)]
        action: GlobalAction,
    },
    /// Diagnose multigraph edge-collapse issues in a graph JSON file
    Diagnose {
        /// Path to graph.json to diagnose
        #[arg(long)]
        graph: PathBuf,
        /// Output JSON instead of human-readable report
        #[arg(long)]
        json: bool,
        /// Max examples per edge group (default: 3)
        #[arg(long, default_value_t = 3)]
        max_examples: usize,
        /// Force directed graph interpretation
        #[arg(long)]
        directed: bool,
        /// Force undirected graph interpretation
        #[arg(long)]
        undirected: bool,
    },
    /// Generate Mermaid call-flow architecture HTML from a graph.json
    #[command(name = "callflow-html")]
    CallflowHtml {
        /// Optional path to graph.json (positional)
        graph_path: Option<PathBuf>,
        /// Path to graph.json (long flag, overrides positional)
        #[arg(long)]
        graph: Option<PathBuf>,
        /// Output HTML file path
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        /// Project root directory
        #[arg(long)]
        project: Option<PathBuf>,
        /// Maximum auto-derived sections (default: 15)
        #[arg(long, default_value_t = 15)]
        max_sections: usize,
        /// Mermaid diagram scale (0.65–1.8, default: 1.0)
        #[arg(long, default_value_t = 1.0)]
        diagram_scale: f64,
        /// Max nodes per section diagram (default: 18)
        #[arg(long, default_value_t = 18)]
        max_diagram_nodes: usize,
        /// Max edges per section diagram (default: 24)
        #[arg(long, default_value_t = 24)]
        max_diagram_edges: usize,
    },
    /// Generate D3 collapsible-tree HTML from a graph.json (filesystem hierarchy view)
    #[command(name = "tree-html")]
    TreeHtml {
        /// Path to graph.json
        #[arg(long)]
        graph: Option<PathBuf>,
        /// Output HTML file path
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        /// Override common root directory
        #[arg(long)]
        root: Option<String>,
        /// Max children per directory node before truncation (default: 200)
        #[arg(long, default_value_t = 200)]
        max_children: usize,
        /// Project label for root node
        #[arg(long)]
        label: Option<String>,
    },
    /// Clone a GitHub repo to a local cache dir (or pull if already cloned)
    Clone {
        /// GitHub URL to clone
        url: String,
        /// Branch to clone/pull
        #[arg(long)]
        branch: Option<String>,
        /// Override destination directory
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Show open PRs dashboard (requires gh CLI)
    Prs {
        /// Override detected default/base branch
        #[arg(long)]
        base: Option<String>,
        /// Target GitHub repo (owner/repo)
        #[arg(long)]
        repo: Option<String>,
        /// Max PRs to fetch (default: 50)
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Path to graph.json for blast-radius calculation
        #[arg(long)]
        graph: Option<PathBuf>,
    },
    /// Print the codesynapse version string
    Version,
    /// No-op hook-check (installed agents call this on every tool use; always exits 0)
    #[command(name = "hook-check")]
    HookCheck,
    /// Check if semantic re-extraction is pending for a path (cron-safe)
    #[command(name = "check-update")]
    CheckUpdate { path: PathBuf },
    /// Check semantic cache for a list of files; writes .codesynapse_cached.json + .codesynapse_uncached.txt
    #[command(name = "cache-check")]
    CacheCheck {
        /// File containing paths to check (one per line)
        files_from: PathBuf,
        /// Root directory for relative paths and output (default: current dir)
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Merge chunk JSON files produced by parallel semantic extraction
    #[command(name = "merge-chunks")]
    MergeChunks {
        /// Chunk JSON files to merge
        files: Vec<PathBuf>,
        /// Output path for merged JSON
        #[arg(long)]
        out: PathBuf,
    },
    /// Merge cached semantic results with freshly-extracted chunks
    #[command(name = "merge-semantic")]
    MergeSemantic {
        /// Path to cached results (.codesynapse_cached.json)
        #[arg(long)]
        cached: Option<PathBuf>,
        /// Path to newly-extracted chunk results
        #[arg(long)]
        new: Option<PathBuf>,
        /// Output path for merged JSON
        #[arg(long)]
        out: PathBuf,
    },
    /// Register codesynapse MCP server in installed AI clients
    Setup {
        /// Register only this client (claude-code, cursor, vscode, windsurf, zed, continue, jetbrains)
        #[arg(long)]
        client: Option<String>,
        /// Workspace directory for VS Code .vscode/mcp.json (default: current dir)
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    /// Answer an architecture question using the graph (same as codesynapse_resolve MCP tool)
    Resolve {
        /// Natural-language question or symbol name
        query: String,
        /// Number of top nodes to return
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// Manage source modules: add, refresh, list, remove
    Module {
        #[command(subcommand)]
        action: ModuleAction,
    },
    /// Manage anonymous usage telemetry (opt-in)
    Telemetry {
        #[command(subcommand)]
        action: TelemetryAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum TelemetryAction {
    /// Enable anonymous usage telemetry
    On,
    /// Disable telemetry and delete any buffered data
    Off,
    /// Show current telemetry status
    Status,
}

#[derive(Debug, Subcommand)]
pub enum PlatformAction {
    /// Install the integration
    Install,
    /// Uninstall the integration
    Uninstall,
}

#[derive(Debug, Subcommand)]
pub enum HookAction {
    /// Install git hooks for automatic graph building
    Install {
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Remove git hooks
    Uninstall {
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Show hook installation status
    Status {
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum GlobalAction {
    /// Add a local graph JSON into the global graph
    Add {
        /// Path to graph.json to merge in
        path: PathBuf,
        /// Repo tag/name (default: directory name)
        #[arg(long = "as")]
        tag: Option<String>,
    },
    /// Remove a repo's nodes from the global graph
    Remove {
        /// Repo tag to remove
        tag: String,
    },
    /// List repos in the global graph
    List,
    /// Print path to the global graph directory
    Path,
}

#[derive(Debug, Subcommand)]
pub enum ModuleAction {
    /// Extract a source directory, register in global graph, save to modules.conf
    Add {
        /// Module name — supports hierarchy (e.g. core/context-impl)
        name: String,
        /// Source directory to extract from
        source: PathBuf,
        /// Path to modules.conf (default: ~/.codesynapse/modules.conf)
        #[arg(long)]
        modules_conf: Option<PathBuf>,
        /// Re-extract even if graph is up to date
        #[arg(long)]
        force: bool,
        /// Enable LLM extraction for doc/paper files (provider from codesynapse.toml or env)
        #[arg(long)]
        llm: bool,
    },
    /// Re-extract modules registered in modules.conf
    Refresh {
        /// Module name to refresh (default: all)
        name: Option<String>,
        /// Path to modules.conf (default: ~/.codesynapse/modules.conf)
        #[arg(long)]
        modules_conf: Option<PathBuf>,
        /// Re-extract even if graph is up to date
        #[arg(long)]
        force: bool,
        /// Enable LLM extraction for doc/paper files (provider from codesynapse.toml or env)
        #[arg(long)]
        llm: bool,
    },
    /// List modules registered in modules.conf with node counts
    List {
        /// Path to modules.conf (default: ~/.codesynapse/modules.conf)
        #[arg(long)]
        modules_conf: Option<PathBuf>,
    },
    /// Remove a module from modules.conf and global graph
    Remove {
        /// Module name to remove
        name: String,
        /// Path to modules.conf (default: ~/.codesynapse/modules.conf)
        #[arg(long)]
        modules_conf: Option<PathBuf>,
    },
}

pub(crate) fn make_extractor() -> Extractor {
    let mut extractor = Extractor::new();
    extractor.register("py", Box::new(TsPythonExtractor));
    extractor.register("js", Box::new(TsJavaScriptExtractor));
    extractor.register("jsx", Box::new(TsJavaScriptExtractor));
    extractor.register("mjs", Box::new(TsJavaScriptExtractor));
    extractor.register("cjs", Box::new(TsJavaScriptExtractor));
    extractor.register("ts", Box::new(TsTypeScriptExtractor));
    extractor.register("tsx", Box::new(TsTypeScriptExtractor));
    extractor.register("mts", Box::new(TsTypeScriptExtractor));
    extractor.register("cts", Box::new(TsTypeScriptExtractor));
    extractor.register("go", Box::new(TsGoExtractor));
    extractor.register("rs", Box::new(TsRustExtractor));
    extractor.register("java", Box::new(TsJavaExtractor));
    extractor.register("c", Box::new(TsCExtractor));
    extractor.register("h", Box::new(TsCExtractor));
    extractor.register("cpp", Box::new(TsCppExtractor));
    extractor.register("cxx", Box::new(TsCppExtractor));
    extractor.register("hpp", Box::new(TsCppExtractor));
    extractor.register("cs", Box::new(TsCSharpExtractor));
    extractor.register("kt", Box::new(TsKotlinExtractor));
    extractor.register("kts", Box::new(TsKotlinExtractor));
    extractor.register("swift", Box::new(TsSwiftExtractor));
    extractor.register("php", Box::new(TsPhpExtractor));
    extractor.register("rb", Box::new(TsRubyExtractor));
    extractor.register("sql", Box::new(TsSqlExtractor));
    extractor.register("sh", Box::new(TsBashExtractor));
    extractor.register("bash", Box::new(TsBashExtractor));
    extractor.register("vue", Box::new(TsVueExtractor));
    extractor.register("svelte", Box::new(TsSvelteExtractor));
    extractor.register("json", Box::new(JsonPackageExtractor));
    extractor.register("mcp.json", Box::new(McpConfigExtractor));
    extractor.register("scala", Box::new(TsScalaExtractor));
    extractor.register("dart", Box::new(TsDartExtractor));
    extractor.register("lua", Box::new(TsLuaExtractor));
    extractor.register("zig", Box::new(TsZigExtractor));
    extractor.register("hs", Box::new(TsHaskellExtractor));
    extractor.register("lhs", Box::new(TsHaskellExtractor));
    extractor.register("groovy", Box::new(TsGroovyExtractor));
    extractor.register("gvy", Box::new(TsGroovyExtractor));
    extractor.register("cmake", Box::new(TsCmakeExtractor));
    extractor.register("rkt", Box::new(TsRacketExtractor));
    extractor
}

pub(crate) fn read_modules_conf(path: &std::path::Path) -> Vec<(String, PathBuf)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let mut parts = l.splitn(2, '|');
            let name = parts.next()?.trim().to_string();
            let source = parts.next()?.trim().to_string();
            if name.is_empty() || source.is_empty() {
                return None;
            }
            Some((name, PathBuf::from(source)))
        })
        .collect()
}

pub(crate) fn upsert_modules_conf(
    path: &std::path::Path,
    name: &str,
    source: &std::path::Path,
) -> std::io::Result<()> {
    let existing = if path.exists() {
        std::fs::read_to_string(path)?
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        String::new()
    };
    let entry = format!("{}|{}", name, source.display());
    let prefix = format!("{}|", name);
    let mut lines: Vec<String> = existing.lines().map(|l| l.to_string()).collect();
    let pos = lines.iter().position(|l| {
        let t = l.trim();
        !t.starts_with('#') && t.starts_with(&prefix)
    });
    match pos {
        Some(i) => lines[i] = entry,
        None => lines.push(entry),
    }
    let mut content = lines.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    std::fs::write(path, content)
}

pub(crate) fn remove_from_modules_conf(path: &std::path::Path, name: &str) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(path)?;
    let prefix = format!("{}|", name);
    let filtered: Vec<&str> = text
        .lines()
        .filter(|l| {
            let t = l.trim();
            t.starts_with('#') || t.is_empty() || !t.starts_with(&prefix)
        })
        .collect();
    let mut content = filtered.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    std::fs::write(path, content)
}

pub(crate) fn find_git_root(start: &std::path::Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub(crate) fn gitignore_codesynapse_artifacts(source_root: &std::path::Path) {
    let Some(git_root) = find_git_root(source_root) else {
        return;
    };
    let exclude = git_root.join(".git").join("info").join("exclude");
    if let Some(parent) = exclude.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    let mut content = existing.clone();
    for pattern in &[".codesynapse-store", "codesynapse-out"] {
        if !existing.lines().any(|l| l.trim() == *pattern) {
            content.push_str(pattern);
            content.push('\n');
        }
    }
    if content != existing {
        std::fs::write(&exclude, content).ok();
    }
}

pub(crate) fn has_newer_sources(
    source_root: &std::path::Path,
    graph_json: &std::path::Path,
) -> bool {
    let Ok(meta) = std::fs::metadata(graph_json) else {
        return true;
    };
    let Ok(graph_mtime) = meta.modified() else {
        return true;
    };
    walkdir::WalkDir::new(source_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .map(|ext| {
                    matches!(
                        ext,
                        "java"
                            | "py"
                            | "js"
                            | "ts"
                            | "tsx"
                            | "jsx"
                            | "mjs"
                            | "cjs"
                            | "mts"
                            | "cts"
                            | "rs"
                            | "go"
                            | "kt"
                            | "kts"
                            | "cs"
                            | "cpp"
                            | "cxx"
                            | "c"
                            | "h"
                            | "hpp"
                            | "rb"
                            | "php"
                            | "swift"
                            | "scala"
                            | "dart"
                            | "lua"
                            | "zig"
                            | "hs"
                            | "groovy"
                            | "gvy"
                            | "sh"
                            | "bash"
                            | "vue"
                            | "svelte"
                            | "sql"
                            | "rkt"
                    )
                })
                .unwrap_or(false)
        })
        .any(|e| {
            std::fs::metadata(e.path())
                .and_then(|m| m.modified())
                .map(|mtime| mtime > graph_mtime)
                .unwrap_or(false)
        })
}

pub(crate) fn build_graph_to_json(
    source: &std::path::Path,
    output_json: &std::path::Path,
    force: bool,
    llm: bool,
) -> Result<(usize, usize)> {
    let output_dir = output_json.parent().unwrap_or(output_json);
    std::fs::create_dir_all(output_dir)?;

    let config = CodeSynapseConfig::discover(source).ok();
    let store_path = source.join(".codesynapse-store");
    let store = SledGraphStore::open(&store_path)?;
    let backend = StoreBackend::Sled(store);
    let builder = codesynapse_core::build::GraphBuilder::new(Box::new(backend));

    let detector = Detector::new(source);
    let files = detector.discover(source)?;

    let extractor = make_extractor();
    let cache = codesynapse_core::cache::FileCache::from_output_dir(output_dir);
    if force {
        cache.clear().ok();
    }

    let results: Vec<(String, Vec<Node>, Vec<Edge>, bool)> = files
        .par_iter()
        .filter(|f| f.file_type.as_str() == "code")
        .filter_map(|file| {
            let src = std::fs::read(&file.path).ok()?;
            let hash = codesynapse_core::cache::FileCache::compute_hash(&src);
            if !force {
                if let Some(cached) = cache.get_cached(&hash) {
                    return Some((file.relative_path.clone(), cached.nodes, cached.edges, true));
                }
            }
            let fragment = extractor.extract_file(&file.path, &src).ok()?;
            cache.set_cached(&hash, &fragment).ok();
            Some((
                file.relative_path.clone(),
                fragment.nodes,
                fragment.edges,
                false,
            ))
        })
        .collect();

    let mut all_fragments: Vec<(String, Vec<Node>, Vec<Edge>)> =
        results.into_iter().map(|(p, n, e, _)| (p, n, e)).collect();

    let use_llm = llm || config.as_ref().and_then(|c| c.llm.as_ref()).is_some();
    if use_llm {
        let llm_config = config
            .as_ref()
            .and_then(|c| c.llm.clone())
            .unwrap_or(LlmConfig {
                provider: None,
                model: None,
                api_key: None,
                base_url: None,
            });
        if let Ok(llm_extractor) = build_extractor(&llm_config) {
            let doc_files: Vec<_> = files
                .iter()
                .filter(|f| matches!(f.file_type, FileType::Document | FileType::Paper))
                .collect();
            let mut llm_nodes = 0usize;
            let mut llm_edges = 0usize;
            for file in &doc_files {
                if let Ok(src) = std::fs::read(&file.path) {
                    if let Ok(fragment) = llm_extractor.extract(&src, &file.path) {
                        llm_nodes += fragment.nodes.len();
                        llm_edges += fragment.edges.len();
                        all_fragments.push((
                            file.relative_path.clone(),
                            fragment.nodes,
                            fragment.edges,
                        ));
                    }
                }
            }
            let _ = (llm_nodes, llm_edges);
        }
    }

    builder.build_from_fragments(all_fragments)?;
    drop(builder); // release sled lock before re-opening

    let store2 = SledGraphStore::open(&store_path)?;
    let backend2 = StoreBackend::Sled(store2);
    let stored_nodes = backend2.node_count()?;
    let stored_edges = backend2.edge_count()?;
    let all_nodes = backend2.get_all_nodes()?;
    let all_edges = backend2.get_all_edges()?;

    codesynapse_core::export::Exporter.to_json_compat_file(&all_nodes, &all_edges, output_json)?;

    Ok((stored_nodes, stored_edges))
}

pub(crate) fn extract_path_to_json(path: &std::path::Path) -> Result<String> {
    let detector = Detector::new(path);
    let files = detector.discover(path)?;
    let extractor = make_extractor();
    let mut fragments = Vec::new();
    for file in &files {
        if file.file_type.as_str() == "code" {
            let source = std::fs::read(&file.path)?;
            if let Ok(fragment) = extractor.extract_file(&file.path, &source) {
                fragments.push(fragment);
            }
        }
    }
    Ok(serde_json::to_string(&fragments)?)
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Extract { path, format } => {
            if matches!(format.as_deref(), Some("json")) {
                let json = extract_path_to_json(&path)?;
                println!("{}", json);
            } else {
                let detector = Detector::new(&path);
                let files = detector.discover(&path)?;
                let extractor = make_extractor();
                for file in &files {
                    if file.file_type.as_str() == "code" {
                        let source = std::fs::read(&file.path)?;
                        match extractor.extract_file(&file.path, &source) {
                            Ok(fragment) => {
                                println!(
                                    "{}: {} nodes, {} edges",
                                    file.relative_path,
                                    fragment.nodes.len(),
                                    fragment.edges.len()
                                );
                            }
                            Err(e) => {
                                eprintln!("Error extracting {}: {}", file.relative_path, e);
                            }
                        }
                    }
                }
            }
        }
        Command::Build {
            path,
            output,
            format,
            no_llm,
            llm,
            code_only,
            update,
            force,
            jobs,
        } => {
            // Load config from codesynapse.toml if present
            let config = CodeSynapseConfig::discover(&path).ok();

            // Merge CLI args with config (CLI takes precedence)
            let output_dir = output
                .clone()
                .or_else(|| {
                    config
                        .as_ref()
                        .and_then(|c| c.output.clone())
                        .map(PathBuf::from)
                })
                .unwrap_or_else(|| path.join("codesynapse-out"));
            std::fs::create_dir_all(&output_dir)?;

            let formats: Vec<String> = format.clone().unwrap_or_else(|| {
                config
                    .as_ref()
                    .and_then(|c| c.formats.clone())
                    .unwrap_or_else(|| vec!["json".to_string()])
            });

            let no_llm = no_llm || config.as_ref().is_some_and(|c| c.no_llm);
            let code_only = code_only || config.as_ref().is_some_and(|c| c.code_only);

            let store_path = path.join(".codesynapse-store");
            let store = SledGraphStore::open(&store_path)?;
            let backend = StoreBackend::Sled(store);
            let builder = GraphBuilder::new(Box::new(backend));

            let detector = Detector::new(&path);
            let files = detector.discover(&path)?;

            let mut extractor = Extractor::new();
            extractor.register("py", Box::new(TsPythonExtractor));
            extractor.register("js", Box::new(TsJavaScriptExtractor));
            extractor.register("jsx", Box::new(TsJavaScriptExtractor));
            extractor.register("mjs", Box::new(TsJavaScriptExtractor));
            extractor.register("cjs", Box::new(TsJavaScriptExtractor));
            extractor.register("ts", Box::new(TsTypeScriptExtractor));
            extractor.register("tsx", Box::new(TsTypeScriptExtractor));
            extractor.register("mts", Box::new(TsTypeScriptExtractor));
            extractor.register("cts", Box::new(TsTypeScriptExtractor));
            extractor.register("go", Box::new(TsGoExtractor));
            extractor.register("rs", Box::new(TsRustExtractor));
            extractor.register("java", Box::new(TsJavaExtractor));
            extractor.register("c", Box::new(TsCExtractor));
            extractor.register("h", Box::new(TsCExtractor));
            extractor.register("cpp", Box::new(TsCppExtractor));
            extractor.register("cxx", Box::new(TsCppExtractor));
            extractor.register("hpp", Box::new(TsCppExtractor));
            extractor.register("cs", Box::new(TsCSharpExtractor));
            extractor.register("kt", Box::new(TsKotlinExtractor));
            extractor.register("kts", Box::new(TsKotlinExtractor));
            extractor.register("swift", Box::new(TsSwiftExtractor));
            extractor.register("php", Box::new(TsPhpExtractor));
            extractor.register("rb", Box::new(TsRubyExtractor));
            extractor.register("sql", Box::new(TsSqlExtractor));
            extractor.register("sh", Box::new(TsBashExtractor));
            extractor.register("bash", Box::new(TsBashExtractor));
            extractor.register("vue", Box::new(TsVueExtractor));
            extractor.register("svelte", Box::new(TsSvelteExtractor));
            extractor.register("json", Box::new(JsonPackageExtractor));
            extractor.register("mcp.json", Box::new(McpConfigExtractor));
            extractor.register("scala", Box::new(TsScalaExtractor));
            extractor.register("dart", Box::new(TsDartExtractor));
            extractor.register("lua", Box::new(TsLuaExtractor));
            extractor.register("zig", Box::new(TsZigExtractor));
            extractor.register("hs", Box::new(TsHaskellExtractor));
            extractor.register("lhs", Box::new(TsHaskellExtractor));
            extractor.register("groovy", Box::new(TsGroovyExtractor));
            extractor.register("gvy", Box::new(TsGroovyExtractor));
            extractor.register("cmake", Box::new(TsCmakeExtractor));
            extractor.register("rkt", Box::new(TsRacketExtractor));

            let cache = FileCache::from_output_dir(&output_dir);
            if force {
                cache.clear().ok();
                println!("Cache cleared (--force)");
            }

            if let Some(num_threads) = jobs {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(num_threads)
                    .build_global()
                    .ok();
            }

            let results: Vec<(String, Vec<Node>, Vec<Edge>, bool)> = files
                .par_iter()
                .filter(|file| file.file_type.as_str() == "code")
                .filter_map(|file| {
                    let source = match std::fs::read(&file.path) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("Error reading {}: {}", file.relative_path, e);
                            return None;
                        }
                    };
                    let hash = FileCache::compute_hash(&source);

                    if !force {
                        if let Some(cached) = cache.get_cached(&hash) {
                            return Some((
                                file.relative_path.clone(),
                                cached.nodes,
                                cached.edges,
                                true,
                            ));
                        }
                    }

                    let fragment = extractor.extract_file(&file.path, &source).ok()?;
                    cache.set_cached(&hash, &fragment).ok();
                    Some((
                        file.relative_path.clone(),
                        fragment.nodes,
                        fragment.edges,
                        false,
                    ))
                })
                .collect();

            let total_cached: usize = results.iter().filter(|(_, _, _, cached)| *cached).count();

            let mut all_fragments: Vec<(String, Vec<Node>, Vec<Edge>)> = results
                .into_iter()
                .map(|(path, nodes, edges, _)| (path, nodes, edges))
                .collect();

            // Filter by code_only
            if code_only {
                all_fragments.retain(|(_, nodes, _)| nodes.iter().any(|n| n.file_type == "code"));
            }

            if update {
                println!("Update mode — incremental graph rebuild");
            }

            let use_llm =
                !no_llm && (llm || config.as_ref().and_then(|c| c.llm.as_ref()).is_some());
            if use_llm {
                let llm_config = config
                    .as_ref()
                    .and_then(|c| c.llm.clone())
                    .unwrap_or(LlmConfig {
                        provider: None,
                        model: None,
                        api_key: None,
                        base_url: None,
                    });
                match build_extractor(&llm_config) {
                    Ok(extractor) => {
                        let doc_files: Vec<_> = files
                            .iter()
                            .filter(|f| matches!(f.file_type, FileType::Document | FileType::Paper))
                            .collect();
                        let mut llm_nodes = 0usize;
                        let mut llm_edges = 0usize;
                        for file in &doc_files {
                            match std::fs::read(&file.path) {
                                Ok(source) => match extractor.extract(&source, &file.path) {
                                    Ok(fragment) => {
                                        llm_nodes += fragment.nodes.len();
                                        llm_edges += fragment.edges.len();
                                        all_fragments.push((
                                            file.relative_path.clone(),
                                            fragment.nodes,
                                            fragment.edges,
                                        ));
                                    }
                                    Err(e) => eprintln!(
                                        "LLM extraction error for {}: {}",
                                        file.relative_path, e
                                    ),
                                },
                                Err(e) => eprintln!("Error reading {}: {}", file.relative_path, e),
                            }
                        }
                        println!(
                            "LLM extracted {} nodes, {} edges from {} doc/paper files",
                            llm_nodes,
                            llm_edges,
                            doc_files.len()
                        );
                    }
                    Err(e) => eprintln!("LLM extractor build error: {e}"),
                }
            } else if no_llm {
                println!("LLM extraction disabled (--no-llm)");
            }

            builder.build_from_fragments(all_fragments)?;
            drop(builder); // release sled lock before re-opening

            // Re-open store to read back built graph
            let store = SledGraphStore::open(&store_path)?;
            let backend = StoreBackend::Sled(store);
            let stored_nodes = backend.node_count()?;
            let stored_edges = backend.edge_count()?;
            if total_cached > 0 {
                println!(
                    "Graph built: {} nodes, {} edges from {} files ({} cached)",
                    stored_nodes,
                    stored_edges,
                    files.len(),
                    total_cached
                );
            } else {
                println!(
                    "Graph built: {} nodes, {} edges from {} files",
                    stored_nodes,
                    stored_edges,
                    files.len()
                );
            }
            let all_nodes = backend.get_all_nodes()?;
            let all_edges = backend.get_all_edges()?;

            // Export in requested formats
            let exporter = Exporter;

            for fmt in &formats {
                let output_path = match fmt.as_str() {
                    "json" => output_dir.join("graph.json"),
                    "html" => output_dir.join("graph.html"),
                    "svg" => output_dir.join("graph.svg"),
                    "graphml" => output_dir.join("graph.graphml"),
                    "cypher" => output_dir.join("graph.cypher"),
                    "wiki" => output_dir.join("wiki").join("index.md"),
                    "obsidian" => output_dir.join("obsidian").join("index.md"),
                    "report" => output_dir.join("GRAPH_REPORT.md"),
                    other => {
                        eprintln!("Unknown format: {}", other);
                        continue;
                    }
                };
                std::fs::create_dir_all(output_path.parent().unwrap_or(&output_dir))?;

                match fmt.as_str() {
                    "json" => {
                        exporter.to_json_compat_file(&all_nodes, &all_edges, &output_path)?;
                    }
                    "html" => {
                        exporter.to_html_file(&all_nodes, &all_edges, &output_path)?;
                    }
                    "svg" => {
                        exporter.to_svg_file(&all_nodes, &all_edges, &output_path)?;
                    }
                    "graphml" => {
                        let content = exporter.to_graphml(&all_nodes, &all_edges)?;
                        std::fs::write(&output_path, content)?;
                    }
                    "cypher" => {
                        exporter.to_cypher_file(&all_nodes, &all_edges, &output_path)?;
                    }
                    "wiki" => {
                        exporter.to_wiki(&all_nodes, &all_edges, &output_dir.join("wiki"))?;
                    }
                    "obsidian" => {
                        exporter.to_obsidian_vault(
                            &all_nodes,
                            &all_edges,
                            &output_dir.join("obsidian"),
                        )?;
                    }
                    "report" => {
                        let report = format!(
                            "# Graph Report\n\nNodes: {}\nEdges: {}\n",
                            all_nodes.len(),
                            all_edges.len()
                        );
                        std::fs::write(&output_path, report)?;
                    }
                    _ => {}
                }
                println!("  Exported {} -> {:?}", fmt, output_path);
            }
        }
        Command::Cluster { path } => {
            let store_path = path.join(".codesynapse-store");
            if !store_path.exists() {
                eprintln!(
                    "No graph store found at {:?}. Run `build` first.",
                    store_path
                );
                return Ok(());
            }
            let store = SledGraphStore::open(&store_path)?;
            let backend = StoreBackend::Sled(store);
            let nodes = backend.get_all_nodes()?;
            let edges = backend.get_all_edges()?;

            if nodes.is_empty() {
                eprintln!("Graph store is empty. Run `build` first.");
                return Ok(());
            }

            let detector = CommunityDetector;
            let communities = detector.detect(&nodes, &edges, 1.0)?;

            println!("Detected {} communities:", communities.len());
            for community in &communities {
                println!(
                    "  Community {}: {} nodes, cohesion {:.3}",
                    community.id,
                    community.nodes.len(),
                    community.cohesion
                );
                // Assign community id to each node in the store
                for node_id in &community.nodes {
                    if let Some(mut node) = backend.get_node(node_id)? {
                        node.community = Some(community.id);
                        backend.add_node(node)?;
                    }
                }
            }
        }
        Command::Analyze { path } => {
            let store_path = path.join(".codesynapse-store");
            if !store_path.exists() {
                eprintln!(
                    "No graph store found at {:?}. Run `build` first.",
                    store_path
                );
                return Ok(());
            }
            let store = SledGraphStore::open(&store_path)?;
            let backend = StoreBackend::Sled(store);
            let nodes = backend.get_all_nodes()?;
            let edges = backend.get_all_edges()?;

            if nodes.is_empty() {
                eprintln!("Graph store is empty. Run `build` first.");
                return Ok(());
            }

            let analyzer = Analyzer;
            let result = analyzer.analyze(&nodes, &edges)?;

            println!("Analysis Results:");
            println!("  God nodes ({}):", result.god_nodes.len());
            for node in &result.god_nodes {
                println!("    - {} ({})", node.label, node.id);
            }
            println!(
                "  Surprising connections: {}",
                result.surprising_connections.len()
            );
            for edge in &result.surprising_connections {
                println!(
                    "    - {} -> {} ({})",
                    edge.source, edge.target, edge.relation
                );
            }
            println!(
                "  Suggested questions: {}",
                result.suggested_questions.len()
            );
            for (i, question) in result.suggested_questions.iter().enumerate() {
                println!("    {}. {}", i + 1, question);
            }
        }
        Command::Export {
            path,
            json,
            html: _,
            graphml: _,
            obsidian: _,
        } => {
            if json {
                let exporter = Exporter;
                // Load from store
                let _store = MemoryGraphStore::new();
                // For now, try to load graph.json if it exists
                let graph_path = path.join("graph.json");
                if graph_path.exists() {
                    let data = exporter.load_json(&graph_path)?;
                    let json = exporter.to_json(&data.nodes, &data.edges, None)?;
                    println!("{}", json);
                }
            }
        }
        Command::Query {
            query,
            graph,
            model_path,
            mode,
            depth,
        } => {
            if let Some(graph_path) = graph {
                let g = codesynapse_serve::graph_query::load_graph(&graph_path)
                    .map_err(|e| codesynapse_core::error::CodeSynapseError::Other(e.to_string()))?;

                // Build node label pairs for embedding
                let node_pairs: Vec<(String, String)> = g
                    .nodes_iter()
                    .map(|(id, n)| (id.to_string(), n.label.clone()))
                    .collect();

                let (embedder_opt, node_embs) = if let Some(mp) = model_path {
                    let embedder = codesynapse_core::embedding::StaticEmbedder::from_path(&mp)
                        .map_err(codesynapse_core::error::CodeSynapseError::Other)?;
                    let refs: Vec<(&str, &str)> = node_pairs
                        .iter()
                        .map(|(id, lbl)| (id.as_str(), lbl.as_str()))
                        .collect();
                    let embs = embedder.embed_nodes(&refs);
                    (Some(embedder), embs)
                } else {
                    (None, std::collections::HashMap::new())
                };

                let dense = embedder_opt.as_ref().map(|e| (e, &node_embs));

                let result = codesynapse_serve::graph_query::query_graph_text_hybrid(
                    &g, &query, &mode, depth, 4000, None, dense,
                );
                println!("{}", result);
            } else {
                let global_dir = dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("~"))
                    .join(".codesynapse");
                let graph_path = global_dir.join("global-graph.json");
                if !graph_path.exists() {
                    eprintln!("No graph found. Run `codesynapse module add <name> <path>` first.");
                    std::process::exit(1);
                }
                let g = codesynapse_serve::graph_query::load_graph(&graph_path)
                    .map_err(|e| codesynapse_core::error::CodeSynapseError::Other(e.to_string()))?;

                let model_path_opt = {
                    let mp = global_dir.join("models").join("potion-code-16M");
                    if mp.exists() {
                        Some(mp)
                    } else {
                        None
                    }
                };

                let node_pairs: Vec<(String, String)> = g
                    .nodes_iter()
                    .map(|(id, n)| (id.to_string(), n.label.clone()))
                    .collect();

                let (embedder_opt, node_embs) = if let Some(mp) = model_path_opt {
                    let embedder = codesynapse_core::embedding::StaticEmbedder::from_path(&mp)
                        .map_err(codesynapse_core::error::CodeSynapseError::Other)?;
                    let refs: Vec<(&str, &str)> = node_pairs
                        .iter()
                        .map(|(id, lbl)| (id.as_str(), lbl.as_str()))
                        .collect();
                    let embs = embedder.embed_nodes(&refs);
                    (Some(embedder), embs)
                } else {
                    (None, std::collections::HashMap::new())
                };

                let dense = embedder_opt.as_ref().map(|e| (e, &node_embs));
                let result = codesynapse_serve::graph_query::query_graph_text_hybrid(
                    &g, &query, &mode, depth, 4000, None, dense,
                );
                println!("{}", result);
            }
        }
        Command::Path {
            source,
            target,
            path,
        } => {
            let store_path = path
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".codesynapse-store");
            if !store_path.exists() {
                eprintln!(
                    "No graph store found at {:?}. Run `codesynapse module add <name> <source>` first.",
                    store_path
                );
                return Ok(());
            }
            let store = SledGraphStore::open(&store_path)?;
            let backend = StoreBackend::Sled(store);
            let engine = QueryEngine::new(Box::new(backend));
            match engine.shortest_path(&source, &target)? {
                Some(path) => {
                    println!("Path found ({} hops):", path.len());
                    for node in &path {
                        println!("  - {}", node.label);
                    }
                }
                None => {
                    println!("No path found between '{}' and '{}'", source, target);
                }
            }
        }
        Command::Stats { path } => {
            let store_path = path.join(".codesynapse-store");
            if store_path.exists() {
                let store = SledGraphStore::open(&store_path)?;
                let backend = StoreBackend::Sled(store);
                println!("Nodes: {}", backend.node_count()?);
                println!("Edges: {}", backend.edge_count()?);
            } else {
                println!("No graph store found at {:?}", store_path);
            }
        }
        Command::Validate { path } => {
            let exporter = Exporter;
            let data = exporter.load_json(&path)?;
            let mut errors = Vec::new();

            for (i, node) in data.nodes.iter().enumerate() {
                if node.id.is_empty() {
                    errors.push(format!("Node {} has empty id", i));
                }
                if node.label.is_empty() {
                    errors.push(format!("Node {} has empty label", i));
                }
            }

            for (i, edge) in data.edges.iter().enumerate() {
                if edge.source.is_empty() {
                    errors.push(format!("Edge {} has empty source", i));
                }
                if edge.target.is_empty() {
                    errors.push(format!("Edge {} has empty target", i));
                }
                if edge.relation.is_empty() {
                    errors.push(format!("Edge {} has empty relation", i));
                }
            }

            if errors.is_empty() {
                println!(
                    "Validation passed: {} nodes, {} edges",
                    data.nodes.len(),
                    data.edges.len()
                );
            } else {
                for err in &errors {
                    eprintln!("Validation error: {}", err);
                }
                std::process::exit(1);
            }
        }
        Command::Merge { a, b } => {
            let exporter = Exporter;
            let data_a = exporter.load_json(&a)?;
            let data_b = exporter.load_json(&b)?;

            let mut all_nodes = data_a.nodes;
            all_nodes.extend(data_b.nodes);

            let mut all_edges = data_a.edges;
            all_edges.extend(data_b.edges);

            let merged = exporter.to_json(&all_nodes, &all_edges, None)?;
            println!("{}", merged);
        }
        Command::MergeDriver {
            base: _,
            current,
            other,
        } => {
            const MERGE_MAX_BYTES: u64 = 50 * 1024 * 1024;
            const MERGE_MAX_NODES: usize = 100_000;

            let check_size = |path: &PathBuf| -> std::result::Result<(), String> {
                let size = std::fs::metadata(path)
                    .map_err(|e| format!("cannot stat {}: {}", path.display(), e))?
                    .len();
                if size > MERGE_MAX_BYTES {
                    return Err(format!(
                        "{} is {} bytes, exceeds {}-byte cap",
                        path.display(),
                        size,
                        MERGE_MAX_BYTES
                    ));
                }
                Ok(())
            };
            if let Err(e) = check_size(&current) {
                eprintln!("[codesynapse merge-driver] {}", e);
                std::process::exit(1);
            }
            if let Err(e) = check_size(&other) {
                eprintln!("[codesynapse merge-driver] {}", e);
                std::process::exit(1);
            }

            let load_value = |path: &PathBuf| -> std::result::Result<serde_json::Value, String> {
                let text = std::fs::read_to_string(path)
                    .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
                serde_json::from_str(&text)
                    .map_err(|e| format!("invalid JSON in {}: {}", path.display(), e))
            };
            let val_cur = match load_value(&current) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[codesynapse merge-driver] error loading graphs: {}", e);
                    std::process::exit(1);
                }
            };
            let val_oth = match load_value(&other) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[codesynapse merge-driver] error loading graphs: {}", e);
                    std::process::exit(1);
                }
            };

            let get_nodes = |v: &serde_json::Value| -> Vec<serde_json::Value> {
                v.get("nodes")
                    .and_then(|n| n.as_array())
                    .cloned()
                    .unwrap_or_default()
            };
            let get_edges = |v: &serde_json::Value| -> Vec<serde_json::Value> {
                v.get("edges")
                    .or_else(|| v.get("links"))
                    .and_then(|e| e.as_array())
                    .cloned()
                    .unwrap_or_default()
            };
            let edges_key = if val_cur.get("links").is_some() {
                "links"
            } else {
                "edges"
            };

            let nodes_cur = get_nodes(&val_cur);
            let edges_cur = get_edges(&val_cur);
            let nodes_oth = get_nodes(&val_oth);
            let edges_oth = get_edges(&val_oth);

            let mut seen_ids: HashSet<String> = HashSet::new();
            let mut merged_nodes: Vec<serde_json::Value> = Vec::new();
            for n in nodes_cur.iter().chain(nodes_oth.iter()) {
                let id = n
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if id.is_empty() || seen_ids.insert(id) {
                    merged_nodes.push(n.clone());
                }
            }

            let mut seen_edges: HashSet<String> = HashSet::new();
            let mut merged_edges: Vec<serde_json::Value> = Vec::new();
            for e in edges_cur.iter().chain(edges_oth.iter()) {
                let src = e.get("source").and_then(|v| v.as_str()).unwrap_or("");
                let tgt = e.get("target").and_then(|v| v.as_str()).unwrap_or("");
                let rel = e.get("relation").and_then(|v| v.as_str()).unwrap_or("");
                let key = format!("{}|{}|{}", src, tgt, rel);
                if seen_edges.insert(key) {
                    merged_edges.push(e.clone());
                }
            }

            if merged_nodes.len() > MERGE_MAX_NODES {
                eprintln!(
                    "[codesynapse merge-driver] merged graph has {} nodes, exceeds {}-node cap; aborting merge.",
                    merged_nodes.len(), MERGE_MAX_NODES
                );
                std::process::exit(1);
            }

            let mut output = val_cur.clone();
            if let Some(obj) = output.as_object_mut() {
                obj.insert("nodes".to_string(), serde_json::Value::Array(merged_nodes));
                obj.insert(
                    edges_key.to_string(),
                    serde_json::Value::Array(merged_edges),
                );
            }

            let out_text = match serde_json::to_string_pretty(&output) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("[codesynapse merge-driver] serialization error: {}", e);
                    std::process::exit(1);
                }
            };
            if let Err(e) = std::fs::write(&current, out_text) {
                eprintln!(
                    "[codesynapse merge-driver] cannot write {}: {}",
                    current.display(),
                    e
                );
                std::process::exit(1);
            }
        }
        Command::Mcp { global_dir } => {
            use codesynapse_mcp::mcp::McpServer;
            let global_dir = global_dir.unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".codesynapse")
            });
            let tmp_sled =
                std::env::temp_dir().join(format!("codesynapse-mcp-sled-{}", std::process::id()));
            match McpServer::new_with_global(&tmp_sled, global_dir) {
                Ok(server) => {
                    if let Err(e) = server.run() {
                        eprintln!("[codesynapse mcp] error: {}", e);
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("[codesynapse mcp] failed to start: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Command::Serve { graph, path, port } => {
            let _graph_path = graph.or(path).unwrap_or_else(|| PathBuf::from("."));
            let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
            println!("Starting gRPC server on {}", addr);
            if let Err(e) = codesynapse_serve::server::start_grpc_blocking(addr) {
                eprintln!("[codesynapse serve] error: {}", e);
                std::process::exit(1);
            }
        }
        Command::Explain { id, path } => {
            let store_path = path
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".codesynapse-store");
            if !store_path.exists() {
                eprintln!("No graph store found. Run `build` first.");
                return Ok(());
            }
            let store = SledGraphStore::open(&store_path)?;
            let backend = StoreBackend::Sled(store);

            if let Some(node) = backend.get_node(&id)? {
                println!("Node: {}", node.label);
                println!("  ID: {}", node.id);
                println!("  File: {}", node.source_file);
                if let Some(loc) = &node.source_location {
                    println!("  Location: {}", loc);
                }
                if let Some(community) = node.community {
                    println!("  Community: {}", community);
                }
                if let Some(rationale) = &node.rationale {
                    println!("  Rationale: {}", rationale);
                }

                let neighbors = backend.neighbors(&id, None)?;
                if !neighbors.is_empty() {
                    println!("\nRelationships:");
                    for (_neighbor, edge) in &neighbors {
                        println!("  {} --[{}]--> {}", edge.source, edge.relation, edge.target);
                    }
                }
            } else {
                println!("Node '{}' not found in graph store.", id);
            }
        }
        Command::Affected {
            id,
            path,
            graph,
            depth,
            relations,
        } => {
            if let Some(graph_path) = graph {
                let gdata =
                    codesynapse_core::affected::load_graph_json(&graph_path).map_err(|e| {
                        codesynapse_core::error::CodeSynapseError::Io(std::io::Error::other(
                            e.to_string(),
                        ))
                    })?;
                let rels: Vec<&str> = if relations.is_empty() {
                    codesynapse_core::affected::DEFAULT_AFFECTED_RELATIONS.to_vec()
                } else {
                    relations.iter().map(|s| s.as_str()).collect()
                };
                let output = codesynapse_core::affected::format_affected(&gdata, &id, &rels, depth);
                println!("{}", output);
            } else {
                let store_path = path
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".codesynapse-store");
                if !store_path.exists() {
                    eprintln!("No graph store found. Run `build` first.");
                    return Ok(());
                }
                let store = SledGraphStore::open(&store_path)?;
                let backend = StoreBackend::Sled(store);

                let engine = QueryEngine::new(Box::new(backend));
                if let Some(node) = engine.resolve_node(&id)? {
                    let result = engine.query_text(&node.id, "bfs", depth, None, None)?;
                    println!(
                        "Nodes affected by '{}': {} directly + {} in neighborhood",
                        node.label,
                        result.seed_nodes.len(),
                        result.neighborhood.len()
                    );
                    for n in &result.neighborhood {
                        if n.id != node.id {
                            println!("  {} ({})", n.label, n.source_file);
                        }
                    }
                } else {
                    eprintln!("Node '{}' not found.", id);
                }
            }
        }
        Command::Tree {
            path,
            root,
            max_depth,
        } => {
            let store_path = path.join(".codesynapse-store");
            if !store_path.exists() {
                eprintln!(
                    "No graph store found at {:?}. Run `build` first.",
                    store_path
                );
                return Ok(());
            }
            let store = SledGraphStore::open(&store_path)?;
            let backend = StoreBackend::Sled(store);
            let all_nodes = backend.get_all_nodes()?;
            let all_edges = backend.get_all_edges()?;

            let root_id = root.unwrap_or_else(|| {
                // Use the first file node as root
                all_nodes
                    .iter()
                    .find(|n| n.file_type == "code")
                    .map(|n| n.id.clone())
                    .unwrap_or_else(|| all_nodes.first().map(|n| n.id.clone()).unwrap_or_default())
            });

            #[allow(clippy::too_many_arguments)]
            fn print_tree(
                node_id: &str,
                all_nodes: &[Node],
                all_edges: &[Edge],
                prefix: &str,
                is_last: bool,
                depth: usize,
                max_depth: usize,
                visited: &mut HashSet<String>,
            ) {
                if depth > max_depth || visited.contains(node_id) {
                    return;
                }
                visited.insert(node_id.to_string());

                let connector = if is_last { "└── " } else { "├── " };
                let node = all_nodes.iter().find(|n| n.id == node_id);
                if let Some(n) = node {
                    println!("{}{}{}", prefix, connector, n.label);
                } else {
                    println!("{}{}{}", prefix, connector, node_id);
                }

                let child_prefix = if is_last { "    " } else { "│   " };
                let children: Vec<&str> = all_edges
                    .iter()
                    .filter(|e| e.source == node_id)
                    .map(|e| e.target.as_str())
                    .collect();

                for (i, child) in children.iter().enumerate() {
                    let child_is_last = i == children.len() - 1;
                    print_tree(
                        child,
                        all_nodes,
                        all_edges,
                        &format!("{}{}", prefix, child_prefix),
                        child_is_last,
                        depth + 1,
                        max_depth,
                        visited,
                    );
                }
            }

            let mut visited = HashSet::new();
            print_tree(
                &root_id,
                &all_nodes,
                &all_edges,
                "",
                true,
                0,
                max_depth,
                &mut visited,
            );
        }
        Command::Benchmark { path, runs } => {
            use std::time::Instant;

            let detector = Detector::new(&path);
            let files = detector.discover(&path)?;

            let mut extractor = Extractor::new();
            extractor.register("py", Box::new(TsPythonExtractor));
            extractor.register("js", Box::new(TsJavaScriptExtractor));
            extractor.register("ts", Box::new(TsTypeScriptExtractor));
            extractor.register("rs", Box::new(TsRustExtractor));
            extractor.register("go", Box::new(TsGoExtractor));
            extractor.register("java", Box::new(TsJavaExtractor));

            let mut total_detect = 0u128;
            let mut total_extract = 0u128;
            let mut total_cluster = 0u128;

            for i in 0..runs {
                let start = Instant::now();
                let _detected = detector.discover(&path)?;
                let detect_elapsed = start.elapsed().as_micros();
                total_detect += detect_elapsed;

                let mut all_fragments = Vec::new();
                let ext_start = Instant::now();
                for file in &files {
                    if file.file_type.as_str() == "code" {
                        if let Ok(source) = std::fs::read(&file.path) {
                            if let Ok(fragment) = extractor.extract_file(&file.path, &source) {
                                all_fragments.push((
                                    file.relative_path.clone(),
                                    fragment.nodes,
                                    fragment.edges,
                                ));
                            }
                        }
                    }
                }
                let extract_elapsed = ext_start.elapsed().as_millis();
                total_extract += extract_elapsed;

                let mut all_nodes = Vec::new();
                let mut all_edges = Vec::new();
                for (_, nodes, edges) in &all_fragments {
                    all_nodes.extend(nodes.iter().cloned());
                    all_edges.extend(edges.iter().cloned());
                }

                let cluster_start = Instant::now();
                let detector = CommunityDetector;
                let _communities = detector.detect(&all_nodes, &all_edges, 1.0)?;
                let cluster_elapsed = cluster_start.elapsed().as_micros();
                total_cluster += cluster_elapsed;

                println!(
                    "Run {}: detect={}ms, extract={}ms, cluster={}ms",
                    i + 1,
                    detect_elapsed / 1000,
                    extract_elapsed,
                    cluster_elapsed / 1000
                );
            }

            println!("\n--- Benchmark Results ---");
            println!(
                "Files: {}",
                files
                    .iter()
                    .filter(|f| f.file_type.as_str() == "code")
                    .count()
            );
            println!("Average detect: {}ms", (total_detect / runs as u128) / 1000);
            println!("Average extract: {}ms", total_extract / runs as u128);
            println!(
                "Average cluster: {}ms",
                (total_cluster / runs as u128) / 1000
            );
        }
        Command::Risk { path, dry_run } => {
            let store_path = path.join(".codesynapse-store");
            if !store_path.exists() {
                eprintln!(
                    "No graph store found at {:?}. Run `build` first.",
                    store_path
                );
                return Ok(());
            }
            let store = SledGraphStore::open(&store_path)?;
            let backend = StoreBackend::Sled(store);
            let mut nodes = backend.get_all_nodes()?;
            let edges = backend.get_all_edges()?;

            if nodes.is_empty() {
                eprintln!("Graph store is empty. Run `build` first.");
                return Ok(());
            }

            let analyzer = Analyzer;
            if let Err(e) = analyzer.compute_temporal_risk(&mut nodes, &edges) {
                eprintln!("Failed to compute temporal risk: {}", e);
                return Ok(());
            }

            if dry_run {
                // Just print the risk scores, don't store them back
                println!("Temporal risk analysis (dry run):");
                let mut risk_nodes: Vec<(&Node, f64)> = nodes
                    .iter()
                    .filter_map(|n| {
                        n.metadata
                            .get("risk_score")
                            .and_then(|s| s.parse::<f64>().ok())
                            .map(|score| (n, score))
                    })
                    .collect();
                risk_nodes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                for (node, score) in risk_nodes.iter().take(20) {
                    println!(
                        "  {} (file: {}): risk={:.2}",
                        node.label, node.source_file, score
                    );
                }
            } else {
                // Store the updated nodes back to the store
                for node in nodes {
                    backend.add_node(node)?;
                }
                println!("Temporal risk analysis complete. Risk scores stored in node metadata.");
            }
        }
        Command::Update { path, force: _ } => {
            println!("Update command — rebuilding graph incrementally");
            // For now, fall back to a full rebuild
            let store_path = path.join(".codesynapse-store");
            let store = SledGraphStore::open(&store_path)?;
            let backend = StoreBackend::Sled(store);
            let builder = GraphBuilder::new(Box::new(backend));

            let detector = Detector::new(&path);
            let files = detector.discover(&path)?;

            let mut extractor = Extractor::new();
            extractor.register("py", Box::new(TsPythonExtractor));
            extractor.register("js", Box::new(TsJavaScriptExtractor));
            extractor.register("ts", Box::new(TsTypeScriptExtractor));
            extractor.register("rs", Box::new(TsRustExtractor));
            extractor.register("go", Box::new(TsGoExtractor));
            extractor.register("java", Box::new(TsJavaExtractor));

            let mut all_fragments = Vec::new();
            for file in &files {
                if file.file_type.as_str() == "code" {
                    if let Ok(source) = std::fs::read(&file.path) {
                        if let Ok(fragment) = extractor.extract_file(&file.path, &source) {
                            all_fragments.push((
                                file.relative_path.clone(),
                                fragment.nodes,
                                fragment.edges,
                            ));
                        }
                    }
                }
            }
            builder.build_from_fragments(all_fragments)?;
            println!("Graph rebuilt successfully.");
        }
        Command::Watch { path: _, output: _ } => {
            let global_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".codesynapse");
            watch_all_modules(global_dir)?;
        }
        Command::Ingest { url, output } => {
            println!("Ingesting: {}", url);
            // Basic HTTP fetch
            match ureq::get(&url).call() {
                Ok(resp) => {
                    let body = resp.into_string().unwrap_or_default();
                    let size = body.len();
                    println!("Fetched {} bytes from {}", size, url);
                    if let Some(out) = output {
                        std::fs::write(&out, &body)?;
                        println!("Saved to {:?}", out);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to fetch {}: {}", url, e);
                }
            }
        }
        Command::Hook { action } => match action {
            HookAction::Install { path } => {
                let repo_root = path.unwrap_or_else(|| PathBuf::from("."));
                let hooks_dir = repo_root.join(".git").join("hooks");
                let hook_path = hooks_dir.join("pre-commit");
                std::fs::create_dir_all(&hooks_dir)?;
                let hook_script = r#"#!/bin/sh
# codesynapse pre-commit hook — auto-build graph on commit
echo "Building graph via codesynapse..."
codesynapse build .
"#;
                std::fs::write(&hook_path, hook_script)?;
                // Make executable
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755))?;
                }
                println!("Hook installed at {:?}", hook_path);
            }
            HookAction::Uninstall { path } => {
                let repo_root = path.unwrap_or_else(|| PathBuf::from("."));
                let hook_path = repo_root.join(".git").join("hooks").join("pre-commit");
                if hook_path.exists() {
                    std::fs::remove_file(&hook_path)?;
                    println!("Hook removed from {:?}", hook_path);
                } else {
                    println!("No codesynapse hook found at {:?}", hook_path);
                }
            }
            HookAction::Status { path } => {
                let repo_root = path.unwrap_or_else(|| PathBuf::from("."));
                let hook_path = repo_root.join(".git").join("hooks").join("pre-commit");
                if hook_path.exists() {
                    let content = std::fs::read_to_string(&hook_path)?;
                    if content.contains("codesynapse") {
                        println!("✅ codesynapse hook is installed at {:?}", hook_path);
                    } else {
                        println!(
                            "⚠️  Hook exists at {:?} but is not a codesynapse hook",
                            hook_path
                        );
                    }
                } else {
                    println!("❌ No codesynapse hook installed at {:?}", hook_path);
                }
            }
        },
        Command::Init { path, force } => {
            let dir = path.unwrap_or_else(|| PathBuf::from("."));
            let config_path = dir.join("codesynapse.toml");
            if config_path.exists() && !force {
                eprintln!(
                    "codesynapse.toml already exists at {:?}. Use --force to overwrite.",
                    config_path
                );
                return Ok(());
            }
            let config_content = r#"# codesynapse configuration
output = "codesynapse-out"
no_llm = false
code_only = false
formats = ["json", "html", "report"]

# [llm]
# provider = "anthropic"
# model = "claude-sonnet-4-20250514"
"#;
            std::fs::write(&config_path, config_content)?;
            ui::ok(&format!(
                "Created codesynapse.toml at {}",
                config_path.display()
            ));
            ui::info("Edit [modules] to configure source paths");
            ui::info("Then run: codesynapse module add <name> <path>");
        }
        Command::Diff { path, baseline } => {
            let exporter = Exporter;
            let data_a = exporter.load_json(&path)?;
            let data_b = exporter.load_json(&baseline)?;

            let node_diff =
                (data_a.nodes.len() as isize - data_b.nodes.len() as isize).unsigned_abs();
            let edge_diff =
                (data_a.edges.len() as isize - data_b.edges.len() as isize).unsigned_abs();

            let node_recall = if data_b.nodes.is_empty() {
                1.0
            } else {
                1.0 - node_diff as f64 / data_b.nodes.len() as f64
            };

            let edge_recall = if data_b.edges.is_empty() {
                1.0
            } else {
                1.0 - edge_diff as f64 / data_b.edges.len() as f64
            };

            println!("Comparison Results:");
            println!(
                "  Nodes: {} vs {} (recall: {:.3})",
                data_a.nodes.len(),
                data_b.nodes.len(),
                node_recall
            );
            println!(
                "  Edges: {} vs {} (recall: {:.3})",
                data_a.edges.len(),
                data_b.edges.len(),
                edge_recall
            );
            println!("  Harmonic mean: {:.3}", (node_recall * edge_recall).sqrt());
        }
        Command::Completions { shell } => {
            use clap_complete::{generate, Shell};
            let shell = match shell.as_str() {
                "bash" => Shell::Bash,
                "zsh" => Shell::Zsh,
                "fish" => Shell::Fish,
                "powershell" => Shell::PowerShell,
                "elvish" => Shell::Elvish,
                _ => {
                    return Err(codesynapse_core::error::CodeSynapseError::msg(format!(
                        "Unknown shell '{}'. Supported: bash, zsh, fish, powershell, elvish",
                        shell
                    )));
                }
            };
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            generate(shell, &mut cmd, &name, &mut std::io::stdout());
        }
        Command::SaveResult { output } => {
            let out_path = output.unwrap_or_else(|| PathBuf::from("codesynapse-result.json"));
            // For now, save a placeholder — real implementation stores last query/analysis result
            let placeholder = serde_json::json!({
                "saved_at": chrono::Utc::now().to_rfc3339(),
                "message": "save-result placeholder"
            });
            std::fs::write(&out_path, serde_json::to_string_pretty(&placeholder)?)?;
            println!("Result saved to {:?}", out_path);
        }
        Command::Install { platform } => {
            let config_dir = dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("~/.config"))
                .join("codesynapse");
            std::fs::create_dir_all(&config_dir)?;
            let install_path = config_dir.join(format!("{}.json", platform));
            let config = serde_json::json!({
                "platform": platform,
                "installed_at": chrono::Utc::now().to_rfc3339(),
                "version": env!("CARGO_PKG_VERSION"),
            });
            std::fs::write(&install_path, serde_json::to_string_pretty(&config)?)?;
            println!(
                "Installed codesynapse for platform '{}' at {:?}",
                platform, install_path
            );
        }
        Command::Claude { ref action }
        | Command::Opencode { ref action }
        | Command::Codex { ref action }
        | Command::Codebuddy { ref action }
        | Command::Claw { ref action }
        | Command::Droid { ref action }
        | Command::Cursor { ref action }
        | Command::Vscode { ref action }
        | Command::Aider { ref action }
        | Command::Copilot { ref action }
        | Command::Trae { ref action }
        | Command::TraeCn { ref action }
        | Command::Antigravity { ref action }
        | Command::Hermes { ref action }
        | Command::Kiro { ref action }
        | Command::Pi { ref action }
        | Command::Devin { ref action } => {
            let cmd_name = match &cli.command {
                Command::Claude { .. } => "claude",
                Command::Opencode { .. } => "opencode",
                Command::Codex { .. } => "codex",
                Command::Codebuddy { .. } => "codebuddy",
                Command::Claw { .. } => "claw",
                Command::Droid { .. } => "droid",
                Command::Cursor { .. } => "cursor",
                Command::Vscode { .. } => "vscode",
                Command::Aider { .. } => "aider",
                Command::Copilot { .. } => "copilot",
                Command::Trae { .. } => "trae",
                Command::TraeCn { .. } => "trae-cn",
                Command::Antigravity { .. } => "antigravity",
                Command::Hermes { .. } => "hermes",
                Command::Kiro { .. } => "kiro",
                Command::Pi { .. } => "pi",
                Command::Devin { .. } => "devin",
                _ => unreachable!(),
            };
            match action {
                PlatformAction::Install => {
                    let config_dir = dirs::config_dir()
                        .unwrap_or_else(|| PathBuf::from("~/.config"))
                        .join("codesynapse")
                        .join("platforms");
                    std::fs::create_dir_all(&config_dir)?;
                    let path = config_dir.join(format!("{}.json", cmd_name));
                    let config = serde_json::json!({
                        "platform": cmd_name,
                        "installed_at": chrono::Utc::now().to_rfc3339(),
                        "version": env!("CARGO_PKG_VERSION"),
                    });
                    std::fs::write(&path, serde_json::to_string_pretty(&config)?)?;
                    println!(
                        "Installed codesynapse integration for '{}' at {:?}",
                        cmd_name, path
                    );
                }
                PlatformAction::Uninstall => {
                    let config_dir = dirs::config_dir()
                        .unwrap_or_else(|| PathBuf::from("~/.config"))
                        .join("codesynapse")
                        .join("platforms");
                    let path = config_dir.join(format!("{}.json", cmd_name));
                    if path.exists() {
                        std::fs::remove_file(&path)?;
                        println!("Uninstalled codesynapse integration for '{}'", cmd_name);
                    } else {
                        println!("No integration found for '{}'", cmd_name);
                    }
                }
            }
        }
        Command::Diagnose {
            graph,
            json,
            max_examples,
            directed,
            undirected,
        } => {
            let force_directed = if directed {
                Some(true)
            } else if undirected {
                Some(false)
            } else {
                None
            };
            match diagnose_file(&graph, force_directed, max_examples, None) {
                Ok(summary) => {
                    if json {
                        println!("{}", format_diagnostic_json(&summary));
                    } else {
                        println!("{}", format_diagnostic_report(&summary));
                    }
                }
                Err(e) => {
                    eprintln!("diagnose failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Command::CallflowHtml {
            graph_path,
            graph,
            output,
            project,
            max_sections,
            diagram_scale,
            max_diagram_nodes,
            max_diagram_edges,
        } => {
            use codesynapse_core::callflow_html::write_callflow_html;
            let resolved_graph = graph.or(graph_path);
            match write_callflow_html(
                project.as_deref(),
                None,
                resolved_graph.as_deref(),
                None,
                None,
                None,
                output.as_deref(),
                "en",
                max_sections,
                diagram_scale,
                max_diagram_nodes,
                max_diagram_edges,
                true,
            ) {
                Ok(path) => println!("callflow HTML written: {}", path.display()),
                Err(e) => {
                    eprintln!("callflow-html failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Command::TreeHtml {
            graph,
            output,
            root,
            max_children,
            label,
        } => {
            use codesynapse_core::tree_html::write_tree_html;
            use codesynapse_core::tree_html::DEFAULT_MAX_CHILDREN;
            let graph_path = graph.unwrap_or_else(|| PathBuf::from("codesynapse-out/graph.json"));
            let output_path = output.unwrap_or_else(|| PathBuf::from("codesynapse-out/tree.html"));
            let mc = if max_children == 200 {
                DEFAULT_MAX_CHILDREN
            } else {
                max_children
            };
            match write_tree_html(
                &graph_path,
                &output_path,
                root.as_deref(),
                mc,
                label.as_deref(),
            ) {
                Ok(path) => println!("tree HTML written: {}", path.display()),
                Err(e) => {
                    eprintln!("tree-html failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Command::Clone { url, branch, out } => {
            match clone_repo(&url, branch.as_deref(), out.as_deref()) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("clone failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Command::Prs {
            base,
            repo,
            limit,
            graph,
        } => {
            use codesynapse_core::prs::{
                compute_pr_impact, fetch_prs, fetch_worktrees, format_prs_text,
            };
            let mut prs = match fetch_prs(repo.as_deref(), base.as_deref(), limit) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
            let worktrees = fetch_worktrees();
            for pr in &mut prs {
                pr.worktree_path = worktrees.get(&pr.branch).cloned();
            }
            if let Some(graph_path) = graph {
                if let Ok(text) = std::fs::read_to_string(&graph_path) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        let nodes_val = v
                            .get("nodes")
                            .and_then(|n| n.as_array())
                            .cloned()
                            .unwrap_or_default();
                        let nodes: Vec<codesynapse_core::types::Node> = nodes_val
                            .iter()
                            .filter_map(|item| serde_json::from_value(item.clone()).ok())
                            .collect();
                        for pr in &mut prs {
                            let files: Vec<&str> =
                                pr.files_changed.iter().map(|s| s.as_str()).collect();
                            let (comms, affected) = compute_pr_impact(&files, &nodes);
                            pr.communities_touched = comms;
                            pr.nodes_affected = affected;
                        }
                    }
                }
            }
            let base_branch = prs
                .first()
                .map(|p| p.expected_base.clone())
                .unwrap_or_else(|| "main".to_string());
            println!("{}", format_prs_text(&prs, &base_branch));
        }
        Command::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Command::HookCheck => {
            std::process::exit(0);
        }
        Command::CheckUpdate { path } => {
            let flag = path.join("codesynapse-out").join("needs_update");
            if flag.exists() {
                println!(
                    "[codesynapse check-update] Pending non-code changes in {}.",
                    path.display()
                );
                println!(
                    "[codesynapse check-update] Run `/codesynapse --update` to apply semantic re-extraction."
                );
            }
        }
        Command::CacheCheck { files_from, root } => {
            use codesynapse_core::cache::check_semantic_cache;
            let text = std::fs::read_to_string(&files_from).unwrap_or_default();
            let files: Vec<String> = text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect();
            let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
            let (cached_nodes, cached_edges, cached_hyperedges, uncached) =
                check_semantic_cache(&file_refs, &root);
            let out_dir = root.join("codesynapse-out");
            std::fs::create_dir_all(&out_dir)?;
            if !cached_nodes.is_empty() || !cached_edges.is_empty() || !cached_hyperedges.is_empty()
            {
                let cached_json = serde_json::json!({
                    "nodes": cached_nodes,
                    "edges": cached_edges,
                    "hyperedges": cached_hyperedges,
                });
                std::fs::write(
                    out_dir.join(".codesynapse_cached.json"),
                    serde_json::to_string(&cached_json)?,
                )?;
            }
            std::fs::write(
                out_dir.join(".codesynapse_uncached.txt"),
                uncached.join("\n"),
            )?;
            let hits = files.len() - uncached.len();
            println!("Cache: {} hit, {} miss", hits, uncached.len());
        }
        Command::MergeChunks { files, out } => {
            let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut merged_nodes: Vec<serde_json::Value> = Vec::new();
            let mut merged_edges: Vec<serde_json::Value> = Vec::new();
            let mut merged_hyperedges: Vec<serde_json::Value> = Vec::new();
            let mut input_tokens: u64 = 0;
            let mut output_tokens: u64 = 0;
            let n_files = files.len();
            for cf in &files {
                let text = match std::fs::read_to_string(cf) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!(
                            "[codesynapse merge-chunks] warning: skipping {:?}: {}",
                            cf, e
                        );
                        continue;
                    }
                };
                let chunk: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!(
                            "[codesynapse merge-chunks] warning: skipping {:?}: {}",
                            cf, e
                        );
                        continue;
                    }
                };
                if let Some(nodes) = chunk.get("nodes").and_then(|n| n.as_array()) {
                    for n in nodes {
                        let id = n
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if id.is_empty() || seen_ids.insert(id) {
                            merged_nodes.push(n.clone());
                        }
                    }
                }
                if let Some(edges) = chunk.get("edges").and_then(|e| e.as_array()) {
                    merged_edges.extend(edges.iter().cloned());
                }
                if let Some(he) = chunk.get("hyperedges").and_then(|h| h.as_array()) {
                    merged_hyperedges.extend(he.iter().cloned());
                }
                input_tokens += chunk
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                output_tokens += chunk
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
            }
            let n_nodes = merged_nodes.len();
            let n_edges = merged_edges.len();
            let result = serde_json::json!({
                "nodes": merged_nodes,
                "edges": merged_edges,
                "hyperedges": merged_hyperedges,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
            });
            if let Some(parent) = out.parent() {
                if parent != std::path::Path::new("") {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(&out, serde_json::to_string(&result)?)?;
            println!(
                "Merged {} chunks: {} nodes, {} edges, {} in / {} out tokens",
                n_files, n_nodes, n_edges, input_tokens, output_tokens
            );
        }
        Command::MergeSemantic { cached, new, out } => {
            let load_json = |p: &Option<PathBuf>| -> serde_json::Value {
                p.as_ref()
                    .filter(|p| p.exists())
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                    .unwrap_or_else(
                        || serde_json::json!({"nodes": [], "edges": [], "hyperedges": []}),
                    )
            };
            let cached_data = load_json(&cached);
            let new_data = load_json(&new);

            let get_arr = |v: &serde_json::Value, key: &str| -> Vec<serde_json::Value> {
                v.get(key)
                    .and_then(|x| x.as_array())
                    .cloned()
                    .unwrap_or_default()
            };

            let cached_nodes = get_arr(&cached_data, "nodes");
            let new_nodes = get_arr(&new_data, "nodes");
            let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut all_nodes: Vec<serde_json::Value> = Vec::new();
            for n in cached_nodes.iter().chain(new_nodes.iter()) {
                let id = n
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if id.is_empty() || seen_ids.insert(id) {
                    all_nodes.push(n.clone());
                }
            }

            let mut all_edges = get_arr(&cached_data, "edges");
            all_edges.extend(get_arr(&new_data, "edges"));
            let mut all_hyperedges = get_arr(&cached_data, "hyperedges");
            all_hyperedges.extend(get_arr(&new_data, "hyperedges"));

            let n_nodes = all_nodes.len();
            let n_edges = all_edges.len();
            let result = serde_json::json!({
                "nodes": all_nodes,
                "edges": all_edges,
                "hyperedges": all_hyperedges,
            });
            if let Some(parent) = out.parent() {
                if parent != std::path::Path::new("") {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(&out, serde_json::to_string(&result)?)?;
            println!("Merged: {} nodes, {} edges", n_nodes, n_edges);
        }
        Command::Global { action } => {
            let global_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".codesynapse");
            match action {
                GlobalAction::Add { path, tag } => {
                    let repo_tag = tag.unwrap_or_else(|| {
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("repo")
                            .to_string()
                    });
                    match global_add(&path, &repo_tag, &global_dir) {
                        Ok(result) if result.skipped => {
                            println!(
                                "Skipped '{}' — graph unchanged (same hash).",
                                result.repo_tag
                            );
                        }
                        Ok(result) => {
                            println!(
                                "Added '{}': +{} nodes, -{} nodes removed.",
                                result.repo_tag, result.nodes_added, result.nodes_removed
                            );
                        }
                        Err(e) => {
                            eprintln!("global add failed: {}", e);
                            std::process::exit(1);
                        }
                    }
                    let model_present = global_dir.join("models").join("potion-code-16M").exists();
                    match embed_global_graph(&global_dir) {
                        Ok(0) if !model_present => eprintln!("No embedding model found — run `codesynapse setup` to enable hybrid search."),
                        Ok(_) => {}
                        Err(e) => eprintln!("Embedding failed: {}", e),
                    }
                }
                GlobalAction::Remove { tag } => match global_remove(&tag, &global_dir) {
                    Ok(removed) => {
                        println!("Removed '{}': {} nodes pruned.", tag, removed);
                    }
                    Err(e) => {
                        eprintln!("global remove failed: {}", e);
                        std::process::exit(1);
                    }
                },
                GlobalAction::List => {
                    let repos = global_list(&global_dir);
                    if repos.is_empty() {
                        println!("No repos in global graph.");
                    } else {
                        for (tag, entry) in &repos {
                            println!(
                                "{}\t{} nodes\t{} edges\t{}",
                                tag, entry.node_count, entry.edge_count, entry.added_at
                            );
                        }
                    }
                }
                GlobalAction::Path => {
                    println!("{}", global_dir.display());
                }
            }
        }
        Command::Setup { client, workspace } => {
            let binary = std::env::current_exe()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "codesynapse".to_string());
            let ws = workspace
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

            ui::print_banner();

            // Model check
            let pb = ui::spinner("Checking embedding model...");
            let model_status = setup_model();
            match &model_status {
                ModelStatus::AlreadyInstalled => {
                    ui::spin_ok(&pb, &format!("Model ready  ({})", MODEL_NAME))
                }
                ModelStatus::Copied => {
                    ui::spin_ok(&pb, &format!("Model installed  ({})", MODEL_NAME))
                }
                ModelStatus::Downloaded => {
                    ui::spin_ok(&pb, &format!("Model downloaded  ({})", MODEL_NAME))
                }
                ModelStatus::NotFound => {
                    ui::spin_warn(&pb, "Model not found — hybrid search disabled");
                    println!();
                    ui::info("  Download failed. Check your internet connection and try again.");
                    ui::info(&format!(
                        "  Or copy model files manually to ~/.codesynapse/models/{}",
                        MODEL_NAME
                    ));
                    println!();
                }
            }

            // MCP clients
            let pb = ui::spinner("Registering MCP clients...");
            let client_results = setup_mcp_clients(&binary, client.as_deref(), &ws);
            pb.finish_and_clear();

            let mut registered_clients: Vec<String> = Vec::new();
            let mut failed_clients: Vec<String> = Vec::new();
            for (name, status) in &client_results {
                match status {
                    ClientStatus::Registered(_) => {
                        ui::ok(name);
                        registered_clients.push(name.clone());
                    }
                    ClientStatus::Failed => {
                        ui::fail(&format!("{}  (write failed)", name));
                        failed_clients.push(name.clone());
                    }
                    ClientStatus::Skipped => {}
                }
            }

            let hybrid = !matches!(model_status, ModelStatus::NotFound);
            ui::print_setup_summary(&registered_clients, &failed_clients, hybrid);

            let global_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".codesynapse");

            // Telemetry consent — only on first run (no config file yet)
            let telemetry_config = global_dir.join("telemetry.json");
            let skip_telemetry_env = std::env::var("DO_NOT_TRACK")
                .map(|v| v == "1")
                .unwrap_or(false)
                || std::env::var("CODESYNAPSE_TELEMETRY")
                    .map(|v| v == "0")
                    .unwrap_or(false);
            if !telemetry_config.exists() && !skip_telemetry_env && ui::is_tty() {
                println!();
                println!(
                    "  {}  Codesynapse can collect anonymous usage stats (tool call counts,",
                    console::style("·").dim()
                );
                println!("     token-savings buckets). No code, paths, or queries are ever sent.");
                println!(
                    "     Details: https://github.com/sohilladhani/codesynapse/blob/master/TELEMETRY.md"
                );
                println!();
                print!("  Enable telemetry? [y/N]: ");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                let mut input = String::new();
                if std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut input).is_ok() {
                    let answer = input.trim().to_lowercase();
                    let t = codesynapse_core::telemetry::Telemetry::new(global_dir.clone());
                    if answer == "y" || answer == "yes" {
                        t.set_enabled(true);
                        ui::ok("Telemetry enabled — run `codesynapse telemetry off` to disable.");
                    } else {
                        t.set_enabled(false);
                        ui::info("Telemetry disabled — run `codesynapse telemetry on` to enable.");
                    }
                }
            }

            let _ = inject_agent_instructions(&ws, &global_dir);
        }
        Command::Resolve { query, limit } => {
            let global_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".codesynapse");
            let graph_path = global_dir.join("global-graph.json");
            if !graph_path.exists() {
                eprintln!("No graph found. Run `codesynapse module add <name> <path>` first.");
                std::process::exit(1);
            }
            let limit = limit.max(1);
            let out =
                codesynapse_serve::graph_query::resolve_query(&query, &graph_path, limit, 24000)
                    .map_err(codesynapse_core::error::CodeSynapseError::Other)?;
            println!("{}", out);
        }
        Command::Module { action } => {
            let global_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".codesynapse");
            match action {
                ModuleAction::Add {
                    name,
                    source,
                    modules_conf,
                    force,
                    llm,
                } => {
                    let conf_path = modules_conf.unwrap_or_else(|| global_dir.join("modules.conf"));
                    let module_graph_dir = global_dir.join("modules").join(&name);
                    std::fs::create_dir_all(&module_graph_dir)?;
                    let out_json = module_graph_dir.join("graph.json");

                    let pb = ui::spinner(format!("Extracting {}...", name));
                    let t0 = std::time::Instant::now();
                    let (nodes, edges) = build_graph_to_json(&source, &out_json, force, llm)?;
                    let elapsed = t0.elapsed().as_secs();
                    ui::spin_ok(
                        &pb,
                        &format!(
                            "{}  {} nodes · {} edges  {}",
                            name,
                            ui::fmt_count(nodes),
                            ui::fmt_count(edges),
                            style(format!("({}s)", elapsed)).dim(),
                        ),
                    );

                    match global_add_force(&out_json, &name, &global_dir, force) {
                        Ok(r) if r.skipped => ui::info("global graph unchanged (same hash)"),
                        Ok(_) => {}
                        Err(e) => {
                            ui::fail(&format!("global add failed: {}", e));
                            std::process::exit(1);
                        }
                    }
                    upsert_modules_conf(&conf_path, &name, &source)?;
                    ui::info(&format!("saved to {}", conf_path.display()));
                    if find_git_root(&source).is_none() {
                        ui::warn("no .git root found — add .codesynapse-store to .git/info/exclude manually");
                    }
                    gitignore_codesynapse_artifacts(&source);

                    let pb = ui::spinner("Building search index...");
                    let t_embed = std::time::Instant::now();
                    let model_present = global_dir.join("models").join("potion-code-16M").exists();
                    match embed_global_graph(&global_dir) {
                        Ok(0) if !model_present => ui::spin_warn(
                            &pb,
                            "no embedding model — run `codesynapse setup` to enable hybrid search",
                        ),
                        Ok(0) => ui::spin_ok(&pb, "search index up to date"),
                        Ok(n) => ui::spin_ok(
                            &pb,
                            &format!(
                                "embedded {} nodes  {}",
                                ui::fmt_count(n),
                                style(format!("({})", ui::fmt_duration(t_embed.elapsed()))).dim(),
                            ),
                        ),
                        Err(e) => ui::spin_fail(&pb, &format!("embedding failed: {}", e)),
                    }
                }
                ModuleAction::Refresh {
                    name,
                    modules_conf,
                    force,
                    llm,
                } => {
                    let conf_path = modules_conf.unwrap_or_else(|| global_dir.join("modules.conf"));
                    let modules = read_modules_conf(&conf_path);
                    if modules.is_empty() {
                        ui::fail(&format!(
                            "no modules in {}. Run `codesynapse module add` first.",
                            conf_path.display()
                        ));
                        std::process::exit(1);
                    }
                    let to_refresh: Vec<_> = match &name {
                        Some(n) => modules.into_iter().filter(|(m, _)| m == n).collect(),
                        None => modules,
                    };
                    if to_refresh.is_empty() {
                        ui::fail(&format!(
                            "module '{}' not found in {}.",
                            name.unwrap_or_default(),
                            conf_path.display()
                        ));
                        std::process::exit(1);
                    }
                    let mut n_refreshed = 0usize;
                    let mut n_skipped = 0usize;
                    for (module_name, source_path) in to_refresh {
                        let module_graph_dir = global_dir.join("modules").join(&module_name);
                        std::fs::create_dir_all(&module_graph_dir)?;
                        let out_json = module_graph_dir.join("graph.json");
                        if !force
                            && out_json.exists()
                            && !has_newer_sources(&source_path, &out_json)
                        {
                            ui::info(&format!("{}  skipped (up to date)", module_name));
                            n_skipped += 1;
                            continue;
                        }
                        let pb = ui::spinner(format!("Refreshing {}...", module_name));
                        let t0 = std::time::Instant::now();
                        let (nodes, edges) =
                            build_graph_to_json(&source_path, &out_json, force, llm)?;
                        let elapsed = t0.elapsed().as_secs();
                        ui::spin_ok(
                            &pb,
                            &format!(
                                "{}  {} nodes · {} edges  {}",
                                module_name,
                                ui::fmt_count(nodes),
                                ui::fmt_count(edges),
                                style(format!("({}s)", elapsed)).dim(),
                            ),
                        );
                        match global_add_force(&out_json, &module_name, &global_dir, force) {
                            Ok(_) => {}
                            Err(e) => {
                                ui::warn(&format!("'{}': global add failed: {}", module_name, e))
                            }
                        }
                        n_refreshed += 1;
                    }
                    let pb = ui::spinner("Building search index...");
                    let t_embed = std::time::Instant::now();
                    let model_present = global_dir.join("models").join("potion-code-16M").exists();
                    match embed_global_graph(&global_dir) {
                        Ok(0) if !model_present => ui::spin_warn(
                            &pb,
                            "no embedding model — run `codesynapse setup` to enable hybrid search",
                        ),
                        Ok(0) => ui::spin_ok(&pb, "search index up to date"),
                        Ok(n) => ui::spin_ok(
                            &pb,
                            &format!(
                                "embedded {} nodes  {}",
                                ui::fmt_count(n),
                                style(format!("({})", ui::fmt_duration(t_embed.elapsed()))).dim(),
                            ),
                        ),
                        Err(e) => ui::spin_fail(&pb, &format!("embedding failed: {}", e)),
                    }
                    println!();
                    match (n_refreshed, n_skipped) {
                        (0, s) => ui::info(&format!("{} skipped (all up to date)", s)),
                        (r, 0) => ui::ok(&format!("{} refreshed", r)),
                        (r, s) => ui::ok(&format!("{} refreshed · {} skipped", r, s)),
                    }
                }
                ModuleAction::List { modules_conf } => {
                    let conf_path = modules_conf.unwrap_or_else(|| global_dir.join("modules.conf"));
                    let modules = read_modules_conf(&conf_path);
                    if modules.is_empty() {
                        ui::info(
                            "no modules registered. Run `codesynapse module add <name> <path>`.",
                        );
                        return Ok(());
                    }
                    let manifest_path = global_dir.join("global-manifest.json");
                    let manifest: serde_json::Value = if manifest_path.exists() {
                        serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)
                            .unwrap_or(serde_json::json!({}))
                    } else {
                        serde_json::json!({})
                    };
                    if ui::is_tty() {
                        println!(
                            "  {}  {:>8}  {}",
                            style(format!("{:<40}", "MODULE")).bold(),
                            style("NODES").bold(),
                            style("SOURCE").bold(),
                        );
                        println!("  {}", style("─".repeat(78)).dim());
                        for (name, source) in &modules {
                            let nodes = manifest["repos"][name]["node_count"].as_u64().unwrap_or(0);
                            let nodes_s = if nodes > 0 {
                                style(format!("{:>8}", ui::fmt_count(nodes as usize)))
                                    .green()
                                    .to_string()
                            } else {
                                style(format!("{:>8}", "0")).yellow().to_string()
                            };
                            println!(
                                "  {:<40} {}  {}",
                                name,
                                nodes_s,
                                style(source.display().to_string()).dim(),
                            );
                        }
                    } else {
                        println!("{:<40} {:>8}  SOURCE", "MODULE", "NODES");
                        println!("{}", "-".repeat(80));
                        for (name, source) in &modules {
                            let nodes = manifest["repos"][name]["node_count"].as_u64().unwrap_or(0);
                            println!("{:<40} {:>8}  {}", name, nodes, source.display());
                        }
                    }
                }
                ModuleAction::Remove { name, modules_conf } => {
                    let conf_path = modules_conf.unwrap_or_else(|| global_dir.join("modules.conf"));
                    remove_from_modules_conf(&conf_path, &name)?;
                    match global_remove(&name, &global_dir) {
                        Ok(removed) => ui::ok(&format!(
                            "removed {}  —  {} nodes pruned",
                            name,
                            ui::fmt_count(removed)
                        )),
                        Err(e) => ui::fail(&format!("global remove failed: {}", e)),
                    }
                    let module_graph = global_dir.join("modules").join(&name).join("graph.json");
                    if module_graph.exists() {
                        std::fs::remove_file(&module_graph).ok();
                    }
                    let module_dir = global_dir.join("modules").join(&name);
                    if module_dir.exists() {
                        std::fs::remove_dir_all(&module_dir).ok();
                    }

                    let remaining = read_modules_conf(&conf_path);
                    if remaining.is_empty() {
                        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                        for filename in &["CLAUDE.md", "AGENTS.md", "GEMINI.md"] {
                            let p = ws.join(filename);
                            strip_marker_block(
                                &p,
                                "<!-- codesynapse:start -->",
                                "<!-- codesynapse:end -->",
                            )
                            .ok();
                        }
                    }
                }
            }
        }
        Command::Telemetry { action } => {
            let global_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".codesynapse");
            let t = codesynapse_core::telemetry::Telemetry::new(global_dir);
            match action {
                TelemetryAction::On => {
                    t.set_enabled(true);
                    println!("Telemetry enabled. Anonymous usage data will be sent daily.");
                    println!("Run `codesynapse telemetry off` or set DO_NOT_TRACK=1 to disable.");
                }
                TelemetryAction::Off => {
                    t.set_enabled(false);
                    println!("Telemetry disabled. Local queue deleted.");
                }
                TelemetryAction::Status => {
                    let s = t.status();
                    println!(
                        "Telemetry: {}",
                        if s.enabled { "enabled" } else { "disabled" }
                    );
                    println!("Decided by: {}", s.decided_by);
                    if let Some(id) = s.machine_id {
                        println!("Machine ID: {}", id);
                    }
                    println!("Config: {}", s.config_path.display());
                    println!(
                        "Docs: https://github.com/sohilladhani/codesynapse/blob/master/TELEMETRY.md"
                    );
                }
            }
        }
    }
    Ok(())
}

fn do_module_add(
    name: &str,
    path: &std::path::Path,
    force: bool,
    global_dir: &std::path::Path,
) -> Result<()> {
    let module_graph_dir = global_dir.join("modules").join(name);
    std::fs::create_dir_all(&module_graph_dir)?;
    let out_json = module_graph_dir.join("graph.json");

    let t_start = std::time::Instant::now();
    build_graph_to_json(path, &out_json, force, false)?;
    let elapsed_ms = t_start.elapsed().as_millis() as u64;

    match global_add_force(&out_json, name, global_dir, force) {
        Ok(_) => {}
        Err(e) => return Err(e),
    }
    embed_global_graph(global_dir)?;

    // Emit index_complete lifecycle event (fire-and-forget, never fails the build).
    let _ = (|| -> Option<()> {
        let telemetry = codesynapse_core::telemetry::Telemetry::new(global_dir.to_path_buf());
        if !telemetry.is_enabled() {
            return None;
        }
        let text = std::fs::read_to_string(&out_json).ok()?;
        let graph: serde_json::Value = serde_json::from_str(&text).ok()?;
        let nodes = graph.get("nodes")?.as_array()?;
        let node_count = nodes.len();

        let mut ext_set = std::collections::HashSet::new();
        for n in nodes.iter().take(2000) {
            if let Some(sf) = n.get("source_file").and_then(|v| v.as_str()) {
                if let Some(ext) = sf.rsplit('.').next() {
                    ext_set.insert(ext_to_lang(ext));
                }
            }
        }
        let mut languages: Vec<&'static str> =
            ext_set.into_iter().filter(|s| !s.is_empty()).collect();
        languages.sort_unstable();

        let embeddings_enabled = global_dir.join("models").join("potion-code-16M").exists();

        telemetry.record_lifecycle(
            "index_complete",
            serde_json::json!({
                "node_count_bucket": codesynapse_core::telemetry::bucket_count(node_count),
                "languages": languages,
                "index_duration_bucket": codesynapse_core::telemetry::bucket_duration_ms(elapsed_ms),
                "embeddings_enabled": embeddings_enabled,
            }),
        );
        telemetry.persist_sync();
        telemetry.flush_bg();
        Some(())
    })();

    Ok(())
}

fn ext_to_lang(ext: &str) -> &'static str {
    match ext {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "go" => "go",
        "rb" => "ruby",
        "cs" => "csharp",
        "cpp" | "cc" | "cxx" | "c" | "h" | "hpp" => "c_cpp",
        "swift" => "swift",
        "php" => "php",
        "scala" => "scala",
        "ex" | "exs" => "elixir",
        "hs" => "haskell",
        "ml" | "mli" => "ocaml",
        "lua" => "lua",
        "r" => "r",
        "jl" => "julia",
        "f90" | "f95" | "f03" | "f08" | "for" => "fortran",
        "v" | "sv" => "verilog",
        "md" | "mdx" => "markdown",
        _ => "",
    }
}

fn inject_agent_instructions(
    workspace: &std::path::Path,
    global_dir: &std::path::Path,
) -> std::io::Result<()> {
    let conf_path = global_dir.join("modules.conf");
    let modules = read_modules_conf(&conf_path);
    let module_name = modules
        .into_iter()
        .next()
        .map(|(n, _)| n)
        .unwrap_or_else(|| "your-repo".into());

    let block = format!(
        "<!-- codesynapse:start -->\n\
         ## Codesynapse — Code Intelligence\n\n\
         Use these CLI commands to answer architecture questions (works in subagents without MCP):\n\
         - `codesynapse resolve \"how does X work\"` — hybrid search + source body\n\
         - `codesynapse query \"concept name\"` — find relevant symbols\n\n\
         Repository indexed as module `{name}`. Re-index: `codesynapse module add --force {name} <path>`.\n\
         <!-- codesynapse:end -->",
        name = module_name
    );

    for filename in &["CLAUDE.md", "AGENTS.md", "GEMINI.md"] {
        let p = workspace.join(filename);
        if p.exists() {
            upsert_marker_block(
                &p,
                "<!-- codesynapse:start -->",
                "<!-- codesynapse:end -->",
                &block,
            )?;
            eprintln!("✓ Injected codesynapse guidance into {}", filename);
        }
    }
    Ok(())
}

fn upsert_marker_block(
    path: &std::path::Path,
    start: &str,
    end: &str,
    block: &str,
) -> std::io::Result<()> {
    let content = std::fs::read_to_string(path)?;
    let new_content = if content.contains(start) {
        let before = content.split(start).next().unwrap_or("");
        let after = content.split(end).last().unwrap_or("");
        format!("{}{}{}", before, block, after)
    } else {
        format!("{}\n\n{}\n", content.trim_end(), block)
    };
    std::fs::write(path, new_content)
}

fn strip_marker_block(path: &std::path::Path, start: &str, end: &str) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(path)?;
    if !content.contains(start) {
        return Ok(());
    }
    let before = content.split(start).next().unwrap_or("").trim_end();
    let after_end = content.split(end).last().unwrap_or("");
    let after = after_end.trim_start_matches('\n');
    let new_content = if after.is_empty() {
        format!("{}\n", before)
    } else {
        format!("{}\n\n{}", before, after)
    };
    std::fs::write(path, new_content)
}

fn watch_all_modules(global_dir: PathBuf) -> Result<()> {
    let conf_path = global_dir.join("modules.conf");
    let modules = read_modules_conf(&conf_path);
    if modules.is_empty() {
        return Err(codesynapse_core::error::CodeSynapseError::Validation(
            "no modules registered. Run `codesynapse module add <name> <path>` first".into(),
        ));
    }

    eprintln!(
        "Watching {} module(s). Press Ctrl+C to stop.",
        modules.len()
    );

    let handles: Vec<_> = modules
        .into_iter()
        .map(|(name, path)| {
            let gd = global_dir.clone();
            std::thread::spawn(move || {
                let cfg = WatchConfig {
                    root: path.clone(),
                    debounce_ms: 500,
                };
                Watcher::new(cfg).run(move |result| {
                    eprintln!(
                        "[{}] {} file(s) changed — rebuilding...",
                        name,
                        result.changed_files.len()
                    );
                    match do_module_add(&name, &path, true, &gd) {
                        Ok(_) => eprintln!("[{}] rebuilt in {}ms", name, result.elapsed_ms),
                        Err(e) => eprintln!("[{}] rebuild failed: {}", name, e),
                    }
                })
            })
        })
        .collect();

    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

fn parse_github_owner_repo(url: &str) -> Option<(String, String)> {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    // Match https://github.com/owner/repo or git@github.com:owner/repo
    let after_gh = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    let mut parts = after_gh.splitn(2, '/');
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.trim_end_matches(".git").to_string();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner, repo))
}

pub fn clone_repo(
    url: &str,
    branch: Option<&str>,
    out: Option<&std::path::Path>,
) -> Result<PathBuf> {
    use codesynapse_core::error::CodeSynapseError;
    use std::process::{Command as Proc, Stdio};

    let url = url.trim_end_matches('/');
    let git_url = if url.ends_with(".git") {
        url.to_string()
    } else {
        format!("{}.git", url)
    };
    let base_url = url.trim_end_matches(".git");
    let repo_slug = base_url
        .trim_start_matches("https://github.com/")
        .trim_start_matches("http://github.com/")
        .trim_start_matches("git@github.com:");

    let dest: PathBuf = if let Some(out_path) = out {
        out_path.to_path_buf()
    } else {
        let (owner, repo) = parse_github_owner_repo(url).ok_or_else(|| {
            CodeSynapseError::Other(format!("not a recognised GitHub URL: {}", url))
        })?;
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(".codesynapse")
            .join("repos")
            .join(owner)
            .join(repo)
    };

    if let Some(b) = branch {
        if b.starts_with('-') {
            return Err(CodeSynapseError::Other(format!(
                "invalid branch name: {:?}",
                b
            )));
        }
    }

    if dest.exists() {
        let mut cmd = Proc::new("git");
        cmd.args(["-C", dest.to_str().unwrap_or("."), "pull", "--quiet"]);
        if let Some(b) = branch {
            cmd.args(["origin", "--", b]);
        }
        let status = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| CodeSynapseError::Other(format!("git pull failed: {}", e)))?;
        if status.success() {
            ui::ok(&format!("Up to date  {}", dest.display()));
        } else {
            ui::warn(&format!("pull failed — using cached  {}", dest.display()));
        }
    } else {
        dest.parent().map(std::fs::create_dir_all).transpose().ok();
        let mut cmd = Proc::new("git");
        cmd.args([
            "clone",
            "--quiet",
            "--depth",
            "1",
            "--single-branch",
            "--no-tags",
        ]);
        if let Some(b) = branch {
            cmd.args(["--branch", b]);
        }
        cmd.args(["--", &git_url, dest.to_str().unwrap_or(".")]);
        let status = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| CodeSynapseError::Other(format!("git clone failed: {}", e)))?;
        if !status.success() {
            ui::fail(&format!("clone failed  {}", repo_slug));
            return Err(CodeSynapseError::Other(
                "git clone failed (run with GIT_TERMINAL_PROMPT=0 git clone for details)"
                    .to_string(),
            ));
        }
        ui::ok(&format!("Cloned  {}", dest.display()));
    }

    Ok(dest)
}

const MODEL_NAME: &str = "potion-code-16M";
const MODEL_FILES: &[&str] = &["model.safetensors", "tokenizer.json", "config.json"];

fn model_is_valid(dir: &std::path::Path) -> bool {
    // Require the two essential files; modules.json is optional
    dir.join("model.safetensors").exists() && dir.join("tokenizer.json").exists()
}

fn try_copy_model(src: &std::path::Path, dst: &std::path::Path) -> bool {
    if !model_is_valid(src) {
        return false;
    }
    if std::fs::create_dir_all(dst).is_err() {
        return false;
    }
    for file in MODEL_FILES {
        let s = src.join(file);
        if s.exists() && std::fs::copy(&s, dst.join(file)).is_err() {
            return false;
        }
    }
    true
}

fn try_download_model(dst: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dst).is_err() {
        return false;
    }
    let version = env!("CARGO_PKG_VERSION");
    let gh_base = format!(
        "https://github.com/sohilladhani/codesynapse/releases/download/v{}/",
        version
    );
    let hf_base = "https://huggingface.co/minishlab/potion-code-16M/resolve/main/";

    for file in MODEL_FILES {
        let out_path = dst.join(file);
        let hf_url = format!("{}{}", hf_base, file);
        let gh_url = format!("{}{}", gh_base, file);

        let bytes = download_file(&hf_url).or_else(|| download_file(&gh_url));
        match bytes {
            Some(b) => {
                if std::fs::write(&out_path, b).is_err() {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

fn download_file(url: &str) -> Option<Vec<u8>> {
    let resp = ureq::get(url).call().ok()?;
    if resp.status() != 200 {
        return None;
    }
    let mut reader = resp.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes).ok()?;
    Some(bytes)
}

pub enum ModelStatus {
    AlreadyInstalled,
    Copied,
    Downloaded,
    NotFound,
}

pub fn setup_model() -> ModelStatus {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    let canonical = home.join(".codesynapse").join("models").join(MODEL_NAME);

    if model_is_valid(&canonical) {
        return ModelStatus::AlreadyInstalled;
    }

    let search_paths: Vec<PathBuf> = vec![std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("models")
        .join(MODEL_NAME)];

    for src in &search_paths {
        if model_is_valid(src) && try_copy_model(src, &canonical) {
            return ModelStatus::Copied;
        }
    }

    if try_download_model(&canonical) {
        return ModelStatus::Downloaded;
    }

    ModelStatus::NotFound
}

pub enum ClientStatus {
    Registered(PathBuf),
    Skipped,
    Failed,
}

/// Register `codesynapse mcp` in every detected AI client config.
/// `client_filter`: if Some, only register that client; otherwise register all detected.
/// `workspace`: used as base for VS Code `.vscode/mcp.json`.
pub fn setup_mcp_clients(
    binary: &str,
    client_filter: Option<&str>,
    workspace: &std::path::Path,
) -> Vec<(String, ClientStatus)> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    let entry_stdio = serde_json::json!({
        "type": "stdio",
        "command": binary,
        "args": ["mcp"],
        "env": {}
    });

    #[allow(clippy::type_complexity)]
    let clients: &[(&str, Box<dyn Fn() -> Option<PathBuf>>, &str)] = &[
        (
            "claude-code",
            Box::new({
                let h = home.clone();
                move || Some(h.join(".claude.json"))
            }),
            "mcpServers",
        ),
        (
            "cursor",
            Box::new({
                let h = home.clone();
                move || {
                    let p = h.join(".cursor").join("mcp.json");
                    if h.join(".cursor").exists() || p.exists() {
                        Some(p)
                    } else {
                        None
                    }
                }
            }),
            "mcpServers",
        ),
        (
            "windsurf",
            Box::new({
                let h = home.clone();
                move || {
                    let p = h.join(".codeium").join("windsurf").join("mcp_config.json");
                    if p.parent().map(|d| d.exists()).unwrap_or(false) {
                        Some(p)
                    } else {
                        None
                    }
                }
            }),
            "mcpServers",
        ),
        (
            "continue",
            Box::new({
                let h = home.clone();
                move || {
                    let p = h.join(".continue").join("config.json");
                    if p.parent().map(|d| d.exists()).unwrap_or(false) || p.exists() {
                        Some(p)
                    } else {
                        None
                    }
                }
            }),
            "mcpServers",
        ),
    ];

    let mut results: Vec<(String, ClientStatus)> = Vec::new();

    for (name, path_fn, key) in clients {
        if let Some(f) = client_filter {
            if f != *name {
                continue;
            }
        }
        match path_fn() {
            None => results.push((name.to_string(), ClientStatus::Skipped)),
            Some(path) => {
                if merge_mcp_entry(&path, key, "codesynapse", entry_stdio.clone()) {
                    results.push((name.to_string(), ClientStatus::Registered(path)));
                } else {
                    results.push((name.to_string(), ClientStatus::Failed));
                }
            }
        }
    }

    // VS Code — workspace-relative
    let vscode_name = "vscode";
    if client_filter.map(|f| f == vscode_name).unwrap_or(true) {
        let vscode_path = workspace.join(".vscode").join("mcp.json");
        let vscode_entry = serde_json::json!({
            "type": "stdio",
            "command": binary,
            "args": ["mcp"]
        });
        if merge_mcp_entry(&vscode_path, "servers", "codesynapse", vscode_entry) {
            results.push((
                vscode_name.to_string(),
                ClientStatus::Registered(vscode_path),
            ));
        } else {
            results.push((vscode_name.to_string(), ClientStatus::Skipped));
        }
    }

    // Zed — nested under context_servers with different schema
    let zed_name = "zed";
    if client_filter.map(|f| f == zed_name).unwrap_or(true) {
        let zed_path = home.join(".config").join("zed").join("settings.json");
        if zed_path.parent().map(|d| d.exists()).unwrap_or(false) || zed_path.exists() {
            let zed_entry = serde_json::json!({
                "command": {"path": binary, "args": ["mcp"]}
            });
            if merge_mcp_entry(&zed_path, "context_servers", "codesynapse", zed_entry) {
                results.push((zed_name.to_string(), ClientStatus::Registered(zed_path)));
            } else {
                results.push((zed_name.to_string(), ClientStatus::Failed));
            }
        } else {
            results.push((zed_name.to_string(), ClientStatus::Skipped));
        }
    }

    // JetBrains — glob all installed IDEs
    let jb_name = "jetbrains";
    if client_filter.map(|f| f == jb_name).unwrap_or(true) {
        let jb_base = home.join(".config").join("JetBrains");
        let mut jb_found = false;
        if jb_base.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&jb_base) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let mcp_path = entry.path().join("mcp.json");
                        if merge_mcp_entry(
                            &mcp_path,
                            "mcpServers",
                            "codesynapse",
                            entry_stdio.clone(),
                        ) {
                            results.push((jb_name.to_string(), ClientStatus::Registered(mcp_path)));
                            jb_found = true;
                        }
                    }
                }
            }
        }
        if !jb_found {
            results.push((jb_name.to_string(), ClientStatus::Skipped));
        }
    }

    // OpenCode — ~/.config/opencode/opencode.json, key "mcp", entry format differs
    let opencode_name = "opencode";
    if client_filter.map(|f| f == opencode_name).unwrap_or(true) {
        let p = home.join(".config").join("opencode").join("opencode.json");
        if p.parent().map(|d| d.exists()).unwrap_or(false) || p.exists() {
            let opencode_entry = serde_json::json!({
                "type": "local",
                "command": [binary, "mcp"],
                "enabled": true
            });
            if merge_mcp_entry(&p, "mcp", "codesynapse", opencode_entry) {
                results.push((opencode_name.to_string(), ClientStatus::Registered(p)));
            } else {
                results.push((opencode_name.to_string(), ClientStatus::Failed));
            }
        } else {
            results.push((opencode_name.to_string(), ClientStatus::Skipped));
        }
    }

    // Kiro (AWS) — ~/.kiro/settings/mcp.json
    let kiro_name = "kiro";
    if client_filter.map(|f| f == kiro_name).unwrap_or(true) {
        let kiro_dir = home.join(".kiro");
        let p = kiro_dir.join("settings").join("mcp.json");
        if kiro_dir.exists() || p.exists() {
            if merge_mcp_entry(&p, "mcpServers", "codesynapse", entry_stdio.clone()) {
                results.push((kiro_name.to_string(), ClientStatus::Registered(p)));
            } else {
                results.push((kiro_name.to_string(), ClientStatus::Failed));
            }
        } else {
            results.push((kiro_name.to_string(), ClientStatus::Skipped));
        }
    }

    // Cline (VS Code extension) — Linux/macOS paths
    let cline_name = "cline";
    if client_filter.map(|f| f == cline_name).unwrap_or(true) {
        let cline_paths = [
            home.join(".config")
                .join("Code")
                .join("User")
                .join("globalStorage")
                .join("saoudrizwan.claude-dev")
                .join("settings")
                .join("cline_mcp_settings.json"),
            home.join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("globalStorage")
                .join("saoudrizwan.claude-dev")
                .join("settings")
                .join("cline_mcp_settings.json"),
        ];
        let mut cline_found = false;
        for p in &cline_paths {
            if (p.parent().map(|d| d.exists()).unwrap_or(false) || p.exists())
                && merge_mcp_entry(p, "mcpServers", "codesynapse", entry_stdio.clone())
            {
                results.push((cline_name.to_string(), ClientStatus::Registered(p.clone())));
                cline_found = true;
                break;
            }
        }
        if !cline_found {
            results.push((cline_name.to_string(), ClientStatus::Skipped));
        }
    }

    results
}

fn merge_mcp_entry(
    config_path: &std::path::Path,
    key: &str,
    server_name: &str,
    entry: serde_json::Value,
) -> bool {
    if let Some(parent) = config_path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let mut root: serde_json::Value = std::fs::read_to_string(config_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if let Some(obj) = root.as_object_mut() {
        let bucket = obj.entry(key).or_insert_with(|| serde_json::json!({}));
        if let Some(servers) = bucket.as_object_mut() {
            servers.insert(server_name.to_string(), entry);
        }
    }

    match serde_json::to_string_pretty(&root) {
        Ok(text) => std::fs::write(config_path, text).is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_diff_command() {
        let cli = Cli::try_parse_from(["codesynapse", "diff", "/tmp/a", "--baseline", "/tmp/b"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Diff { path, baseline } => {
                assert_eq!(path, PathBuf::from("/tmp/a"));
                assert_eq!(baseline, PathBuf::from("/tmp/b"));
            }
            other => panic!("Expected Diff, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_diff_uses_compare_alias() {
        let cli = Cli::try_parse_from(["codesynapse", "compare", "/tmp/a", "--baseline", "/tmp/b"]);
        assert!(cli.is_ok(), "compare alias should work: {:?}", cli.err());
        let cli = cli.unwrap();
        match cli.command {
            Command::Diff { .. } => {} // OK — alias resolves to Diff
            other => panic!("Expected Diff via compare alias, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_ingest_default() {
        let cli = Cli::try_parse_from(["codesynapse", "ingest", "https://example.com"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Ingest { url, output } => {
                assert_eq!(url, "https://example.com");
                assert!(output.is_none());
            }
            other => panic!("Expected Ingest, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_ingest_with_output() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "ingest",
            "https://example.com",
            "--output",
            "/tmp/out.json",
        ]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Ingest { url, output } => {
                assert_eq!(url, "https://example.com");
                assert_eq!(output, Some(PathBuf::from("/tmp/out.json")));
            }
            other => panic!("Expected Ingest, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_hook_install() {
        let cli = Cli::try_parse_from(["codesynapse", "hook", "install"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Hook { action } => match action {
                HookAction::Install { path } => {
                    assert!(path.is_none());
                }
                other => panic!("Expected Hook::Install, got {:?}", other),
            },
            other => panic!("Expected Hook, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_hook_uninstall() {
        let cli = Cli::try_parse_from(["codesynapse", "hook", "uninstall"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Hook { action } => match action {
                HookAction::Uninstall { path } => {
                    assert!(path.is_none());
                }
                other => panic!("Expected Hook::Uninstall, got {:?}", other),
            },
            other => panic!("Expected Hook, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_hook_status() {
        let cli = Cli::try_parse_from(["codesynapse", "hook", "status"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Hook { action } => match action {
                HookAction::Status { path } => {
                    assert!(path.is_none());
                }
                other => panic!("Expected Hook::Status, got {:?}", other),
            },
            other => panic!("Expected Hook, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_hook_with_path() {
        let cli = Cli::try_parse_from(["codesynapse", "hook", "install", "--path", "/tmp/repo"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Hook { action } => match action {
                HookAction::Install { path } => {
                    assert_eq!(path, Some(PathBuf::from("/tmp/repo")));
                }
                other => panic!("Expected Hook::Install, got {:?}", other),
            },
            other => panic!("Expected Hook, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_init_default() {
        let cli = Cli::try_parse_from(["codesynapse", "init"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Init { path, force } => {
                assert!(path.is_none());
                assert!(!force);
            }
            other => panic!("Expected Init, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_init_with_path_and_force() {
        let cli = Cli::try_parse_from(["codesynapse", "init", "--path", "/tmp/proj", "--force"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Init { path, force } => {
                assert_eq!(path, Some(PathBuf::from("/tmp/proj")));
                assert!(force);
            }
            other => panic!("Expected Init, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_serve_default() {
        let cli = Cli::try_parse_from(["codesynapse", "serve", "/tmp/graph"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Serve { graph, path, port } => {
                assert!(graph.is_none());
                assert_eq!(path, Some(PathBuf::from("/tmp/graph")));
                assert_eq!(port, 50051);
            }
            other => panic!("Expected Serve, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_serve_with_graph_flag() {
        let cli = Cli::try_parse_from(["codesynapse", "serve", "--graph", "/tmp/store"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Serve { graph, path, port } => {
                assert_eq!(graph, Some(PathBuf::from("/tmp/store")));
                assert!(path.is_none());
                assert_eq!(port, 50051);
            }
            other => panic!("Expected Serve, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_build_default() {
        let cli = Cli::try_parse_from(["codesynapse", "build", "/tmp/proj"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Build {
                path,
                output,
                format,
                no_llm,
                llm,
                code_only,
                update,
                force,
                jobs,
            } => {
                assert_eq!(path, PathBuf::from("/tmp/proj"));
                assert!(output.is_none());
                assert!(format.is_none());
                assert!(!no_llm);
                assert!(!llm);
                assert!(!code_only);
                assert!(!update);
                assert!(!force);
                assert!(jobs.is_none());
            }
            other => panic!("Expected Build, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_build_all_flags() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "build",
            "/tmp/proj",
            "--output",
            "/tmp/out",
            "--format",
            "json,html,svg",
            "--no-llm",
            "--code-only",
            "--update",
            "--force",
            "--jobs",
            "4",
        ]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Build {
                path,
                output,
                format,
                no_llm,
                llm: _,
                code_only,
                update,
                force,
                jobs,
            } => {
                assert_eq!(path, PathBuf::from("/tmp/proj"));
                assert_eq!(output, Some(PathBuf::from("/tmp/out")));
                assert_eq!(
                    format,
                    Some(vec![
                        "json".to_string(),
                        "html".to_string(),
                        "svg".to_string()
                    ])
                );
                assert!(no_llm);
                assert!(code_only);
                assert!(update);
                assert!(force);
                assert_eq!(jobs, Some(4));
            }
            other => panic!("Expected Build, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_watch_default() {
        let cli = Cli::try_parse_from(["codesynapse", "watch"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Watch { path, output } => {
                assert!(path.is_none());
                assert!(output.is_none());
            }
            other => panic!("Expected Watch, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_watch_with_flags() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "watch",
            "--path",
            "/tmp/proj",
            "--output",
            "/tmp/out",
        ]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Watch { path, output } => {
                assert_eq!(path, Some(PathBuf::from("/tmp/proj")));
                assert_eq!(output, Some(PathBuf::from("/tmp/out")));
            }
            other => panic!("Expected Watch, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_build_format_single() {
        let cli = Cli::try_parse_from(["codesynapse", "build", "/tmp/proj", "--format", "json"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Build { format, .. } => {
                assert_eq!(format, Some(vec!["json".to_string()]));
            }
            other => panic!("Expected Build, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_completions_bash() {
        let cli = Cli::try_parse_from(["codesynapse", "completions", "bash"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Completions { shell } => {
                assert_eq!(shell, "bash");
            }
            other => panic!("Expected Completions, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_completions_zsh() {
        let cli = Cli::try_parse_from(["codesynapse", "completions", "zsh"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Completions { shell } => {
                assert_eq!(shell, "zsh");
            }
            other => panic!("Expected Completions, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_save_result_default() {
        let cli = Cli::try_parse_from(["codesynapse", "save-result"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::SaveResult { output } => {
                assert!(output.is_none());
            }
            other => panic!("Expected SaveResult, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_save_result_with_output() {
        let cli = Cli::try_parse_from(["codesynapse", "save-result", "-o", "/tmp/result.json"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::SaveResult { output } => {
                assert_eq!(output, Some(PathBuf::from("/tmp/result.json")));
            }
            other => panic!("Expected SaveResult, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_install_platform() {
        let cli = Cli::try_parse_from(["codesynapse", "install", "--platform", "cursor"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        match cli.command {
            Command::Install { platform } => {
                assert_eq!(platform, "cursor");
            }
            other => panic!("Expected Install, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_platform_commands() {
        for cmd in &[
            "claude",
            "opencode",
            "codex",
            "codebuddy",
            "claw",
            "droid",
            "cursor",
            "vscode",
            "aider",
            "copilot",
            "trae",
            "trae-cn",
            "antigravity",
            "hermes",
            "kiro",
            "pi",
            "devin",
        ] {
            for action in &["install", "uninstall"] {
                let args = vec![
                    "codesynapse".to_string(),
                    cmd.to_string(),
                    action.to_string(),
                ];
                let cli = Cli::try_parse_from(args);
                assert!(cli.is_ok(), "Failed: {} {}: {:?}", cmd, action, cli.err());
                let cli = cli.unwrap();
                match cli.command {
                    Command::Claude { .. }
                    | Command::Opencode { .. }
                    | Command::Codex { .. }
                    | Command::Codebuddy { .. }
                    | Command::Claw { .. }
                    | Command::Droid { .. }
                    | Command::Cursor { .. }
                    | Command::Vscode { .. }
                    | Command::Aider { .. }
                    | Command::Copilot { .. }
                    | Command::Trae { .. }
                    | Command::TraeCn { .. }
                    | Command::Antigravity { .. }
                    | Command::Hermes { .. }
                    | Command::Kiro { .. }
                    | Command::Pi { .. }
                    | Command::Devin { .. } => {} // OK
                    other => panic!(
                        "Expected platform command for {} {}, got {:?}",
                        cmd, action, other
                    ),
                }
            }
        }
    }

    #[test]
    fn test_affected_cli_reverse_traverses_impact_edges() {
        use codesynapse_core::affected::{format_affected, DEFAULT_AFFECTED_RELATIONS};
        use codesynapse_core::types::{Edge, GraphData, Node};
        use std::collections::HashMap;

        fn n(id: &str, label: &str, src: &str) -> Node {
            Node {
                id: id.into(),
                label: label.into(),
                file_type: "code".into(),
                source_file: src.into(),
                source_location: None,
                community: None,
                rationale: None,
                metadata: HashMap::new(),
                docstring: None,
            }
        }
        fn e(src: &str, tgt: &str, rel: &str) -> Edge {
            Edge {
                source: src.into(),
                target: tgt.into(),
                relation: rel.into(),
                confidence: "EXTRACTED".into(),
                source_file: None,
                weight: 1.0,
                context: None,
            }
        }
        let g = GraphData {
            nodes: vec![
                n("target", "Foo", "pkg/foo.py"),
                n("caller", "X()", "app.py"),
                n("barrel", "__init__.py", "pkg/__init__.py"),
                n("consumer", "app.py", "app.py"),
            ],
            edges: vec![
                e("caller", "target", "calls"),
                e("barrel", "target", "re_exports"),
                e("consumer", "target", "imports"),
            ],
            hyperedges: None,
        };
        let out = format_affected(&g, "Foo", DEFAULT_AFFECTED_RELATIONS, 2);
        assert!(
            out.contains("Affected nodes for Foo"),
            "header missing: {out}"
        );
        assert!(out.contains("X()"), "X() missing: {out}");
        assert!(out.contains("calls"), "calls missing: {out}");
        assert!(out.contains("__init__.py"), "__init__ missing: {out}");
        assert!(out.contains("re_exports"), "re_exports missing: {out}");
        assert!(out.contains("imports"), "imports missing: {out}");
    }

    #[test]
    fn test_affected_cli_relation_filter_limits_reverse_traversal() {
        use codesynapse_core::affected::format_affected;
        use codesynapse_core::types::{Edge, GraphData, Node};
        use std::collections::HashMap;

        fn n(id: &str, label: &str, src: &str) -> Node {
            Node {
                id: id.into(),
                label: label.into(),
                file_type: "code".into(),
                source_file: src.into(),
                source_location: None,
                community: None,
                rationale: None,
                metadata: HashMap::new(),
                docstring: None,
            }
        }
        fn e(src: &str, tgt: &str, rel: &str) -> Edge {
            Edge {
                source: src.into(),
                target: tgt.into(),
                relation: rel.into(),
                confidence: "EXTRACTED".into(),
                source_file: None,
                weight: 1.0,
                context: None,
            }
        }
        let g = GraphData {
            nodes: vec![
                n("target", "Foo", "pkg/foo.py"),
                n("caller", "X()", "app.py"),
                n("barrel", "__init__.py", "pkg/__init__.py"),
                n("consumer", "app.py", "app.py"),
            ],
            edges: vec![
                e("caller", "target", "calls"),
                e("barrel", "target", "re_exports"),
                e("consumer", "target", "imports"),
            ],
            hyperedges: None,
        };
        let out = format_affected(&g, "Foo", &["calls"], 2);
        assert!(
            out.contains("Relations: calls"),
            "relations header missing: {out}"
        );
        assert!(out.contains("X()"), "X() missing: {out}");
        assert!(
            !out.contains("__init__.py"),
            "__init__ should not appear: {out}"
        );
    }

    #[test]
    fn test_extract_json_output() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("foo.py"), b"def hello():\n    pass\n").unwrap();
        let json = extract_path_to_json(dir.path()).unwrap();
        let arr: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(arr.is_array(), "expected JSON array, got: {json}");
        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 1, "expected 1 fragment for 1 code file");
        let frag = &arr[0];
        assert!(frag["nodes"].is_array(), "nodes must be array");
        assert!(frag["edges"].is_array(), "edges must be array");
        let nodes = frag["nodes"].as_array().unwrap();
        assert!(
            nodes
                .iter()
                .any(|n| n["label"].as_str().unwrap_or("").contains("hello")),
            "hello fn node missing; nodes: {nodes:?}"
        );
    }

    #[test]
    fn test_extract_json_empty_dir() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let json = extract_path_to_json(dir.path()).unwrap();
        let arr: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(arr.is_array());
        assert_eq!(arr.as_array().unwrap().len(), 0, "empty dir → empty array");
    }

    #[test]
    fn test_cli_global_add_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "global", "add", "/tmp/graph.json"]);
        assert!(cli.is_ok(), "parse failed: {:?}", cli.err());
        match cli.unwrap().command {
            Command::Global {
                action: GlobalAction::Add { path, tag },
            } => {
                assert_eq!(path, PathBuf::from("/tmp/graph.json"));
                assert!(tag.is_none());
            }
            other => panic!("Expected Global::Add, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_global_add_with_tag() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "global",
            "add",
            "/tmp/graph.json",
            "--as",
            "myrepo",
        ]);
        assert!(cli.is_ok(), "parse failed: {:?}", cli.err());
        match cli.unwrap().command {
            Command::Global {
                action: GlobalAction::Add { tag, .. },
            } => {
                assert_eq!(tag, Some("myrepo".to_string()));
            }
            other => panic!("Expected Global::Add with tag, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_global_remove_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "global", "remove", "myrepo"]);
        assert!(cli.is_ok(), "parse failed: {:?}", cli.err());
        match cli.unwrap().command {
            Command::Global {
                action: GlobalAction::Remove { tag },
            } => {
                assert_eq!(tag, "myrepo");
            }
            other => panic!("Expected Global::Remove, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_global_list_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "global", "list"]);
        assert!(cli.is_ok(), "parse failed: {:?}", cli.err());
        match cli.unwrap().command {
            Command::Global {
                action: GlobalAction::List,
            } => {}
            other => panic!("Expected Global::List, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_global_path_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "global", "path"]);
        assert!(cli.is_ok(), "parse failed: {:?}", cli.err());
        match cli.unwrap().command {
            Command::Global {
                action: GlobalAction::Path,
            } => {}
            other => panic!("Expected Global::Path, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_global_add_round_trip() {
        use codesynapse_core::global_graph::{global_add, global_list, global_remove};
        use tempfile::tempdir;

        let global_dir = tempdir().unwrap();
        let src_dir = tempdir().unwrap();

        let graph = serde_json::json!({
            "directed": false, "multigraph": false, "graph": {},
            "nodes": [{"id": "n1", "label": "Main", "file_type": "code", "source_file": "main.rs",
                       "metadata": {}}],
            "links": []
        });
        let graph_path = src_dir.path().join("graph.json");
        std::fs::write(&graph_path, serde_json::to_string(&graph).unwrap()).unwrap();

        let result = global_add(&graph_path, "testrepo", global_dir.path()).unwrap();
        assert_eq!(result.repo_tag, "testrepo");
        assert!(!result.skipped);
        assert_eq!(result.nodes_added, 1);

        let repos = global_list(global_dir.path());
        assert!(repos.contains_key("testrepo"), "testrepo missing from list");
        assert_eq!(repos["testrepo"].node_count, 1);

        let removed = global_remove("testrepo", global_dir.path()).unwrap();
        assert!(removed >= 1);
        let repos_after = global_list(global_dir.path());
        assert!(!repos_after.contains_key("testrepo"));
    }

    #[test]
    fn test_cli_diagnose_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "diagnose", "--graph", "/tmp/graph.json"]);
        assert!(cli.is_ok());
        let cmd = cli.unwrap().command;
        assert!(matches!(
            cmd,
            Command::Diagnose {
                json: false,
                directed: false,
                undirected: false,
                ..
            }
        ));
    }

    #[test]
    fn test_cli_diagnose_json_flag() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "diagnose",
            "--graph",
            "/tmp/graph.json",
            "--json",
        ]);
        assert!(cli.is_ok());
        let cmd = cli.unwrap().command;
        assert!(matches!(cmd, Command::Diagnose { json: true, .. }));
    }

    #[test]
    fn test_cli_diagnose_directed_flag() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "diagnose",
            "--graph",
            "/tmp/g.json",
            "--directed",
        ]);
        assert!(cli.is_ok());
        let cmd = cli.unwrap().command;
        assert!(matches!(cmd, Command::Diagnose { directed: true, .. }));
    }

    #[test]
    fn test_cli_diagnose_integration() {
        use codesynapse_core::diagnostics::{diagnose_file, format_diagnostic_report};
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let graph = serde_json::json!({
            "directed": true, "multigraph": false, "graph": {},
            "nodes": [
                {"id": "a", "label": "A", "file_type": "code", "source_file": "a.rs", "metadata": {}},
                {"id": "b", "label": "B", "file_type": "code", "source_file": "b.rs", "metadata": {}}
            ],
            "links": [
                {"source": "a", "target": "b", "relation": "calls", "confidence": "EXTRACTED",
                 "source_file": "a.rs", "weight": 1.0}
            ]
        });
        let path = dir.path().join("graph.json");
        std::fs::write(&path, serde_json::to_string(&graph).unwrap()).unwrap();

        let summary = diagnose_file(&path, None, 3, None).unwrap();
        let report = format_diagnostic_report(&summary);
        assert!(report.contains("MultiDiGraph"));
        assert_eq!(summary.raw_edge_count, 1);
        assert_eq!(summary.node_count, 2);
    }

    #[test]
    fn test_cli_callflow_html_parses() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "callflow-html",
            "--output",
            "out.html",
            "--max-sections",
            "4",
        ]);
        assert!(cli.is_ok(), "callflow-html should parse: {:?}", cli.err());
        assert!(matches!(cli.unwrap().command, Command::CallflowHtml { .. }));
    }

    #[test]
    fn test_cli_callflow_html_positional_graph() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "callflow-html",
            "/tmp/graph.json",
            "--output",
            "out.html",
        ]);
        assert!(
            cli.is_ok(),
            "callflow-html with positional graph should parse"
        );
        if let Command::CallflowHtml { graph_path, .. } = cli.unwrap().command {
            assert!(graph_path.is_some());
        } else {
            panic!("expected CallflowHtml");
        }
    }

    #[test]
    fn test_cli_callflow_html_integration() {
        use codesynapse_core::callflow_html::write_callflow_html;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let out = dir.path().join("codesynapse-out");
        std::fs::create_dir_all(&out).unwrap();

        let graph = serde_json::json!({
            "directed": false, "multigraph": false, "graph": {},
            "nodes": [
                {"id": "a", "label": "ApiClient", "source_file": "src/api.py", "file_type": "code", "community": 0},
                {"id": "b", "label": "run()", "source_file": "src/main.py", "file_type": "code", "community": 0},
                {"id": "c", "label": "write_html()", "source_file": "src/export.py", "file_type": "code", "community": 1}
            ],
            "links": [
                {"source": "a", "target": "b", "relation": "calls", "confidence": "EXTRACTED", "confidence_score": 1.0}
            ],
            "hyperedges": []
        });
        std::fs::write(
            out.join("graph.json"),
            serde_json::to_string(&graph).unwrap(),
        )
        .unwrap();
        std::fs::write(
            out.join(".codesynapse_labels.json"),
            r#"{"0":"Runtime","1":"Export"}"#,
        )
        .unwrap();

        let html_path = write_callflow_html(
            Some(dir.path()),
            None,
            None,
            None,
            None,
            None,
            Some(&out.join("callflow.html")),
            "en",
            4,
            1.0,
            18,
            24,
            false,
        )
        .unwrap();

        let content = std::fs::read_to_string(&html_path).unwrap();
        assert!(content.contains("mermaid"), "HTML should contain mermaid");
        assert!(
            content.contains("ApiClient"),
            "HTML should contain node label"
        );
        assert!(html_path.exists());
    }

    #[test]
    fn test_cli_callflow_html_max_sections_flag() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "callflow-html",
            "--max-sections",
            "6",
            "--diagram-scale",
            "1.2",
        ]);
        assert!(cli.is_ok());
        if let Command::CallflowHtml {
            max_sections,
            diagram_scale,
            ..
        } = cli.unwrap().command
        {
            assert_eq!(max_sections, 6);
            assert!((diagram_scale - 1.2).abs() < 0.01);
        }
    }

    #[test]
    fn test_cli_callflow_html_graph_flag() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "callflow-html",
            "--graph",
            "/tmp/custom.json",
        ]);
        assert!(cli.is_ok());
        if let Command::CallflowHtml { graph, .. } = cli.unwrap().command {
            assert!(graph.is_some());
        }
    }

    #[test]
    fn test_cli_clone_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "clone", "https://github.com/owner/repo"]);
        assert!(cli.is_ok(), "parse failed: {:?}", cli.err());
        match cli.unwrap().command {
            Command::Clone { url, branch, out } => {
                assert_eq!(url, "https://github.com/owner/repo");
                assert!(branch.is_none());
                assert!(out.is_none());
            }
            other => panic!("Expected Clone, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_clone_with_branch() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "clone",
            "https://github.com/owner/repo",
            "--branch",
            "main",
        ]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Command::Clone { branch, .. } => assert_eq!(branch.as_deref(), Some("main")),
            other => panic!("Expected Clone, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_clone_with_out() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "clone",
            "https://github.com/owner/repo",
            "--out",
            "/tmp/myrepo",
        ]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Command::Clone { out, .. } => {
                assert_eq!(out, Some(PathBuf::from("/tmp/myrepo")));
            }
            other => panic!("Expected Clone, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_github_owner_repo_https() {
        let result = parse_github_owner_repo("https://github.com/owner/myrepo");
        assert_eq!(result, Some(("owner".into(), "myrepo".into())));
    }

    #[test]
    fn test_parse_github_owner_repo_git_suffix() {
        let result = parse_github_owner_repo("https://github.com/owner/myrepo.git");
        assert_eq!(result, Some(("owner".into(), "myrepo".into())));
    }

    #[test]
    fn test_parse_github_owner_repo_ssh() {
        let result = parse_github_owner_repo("git@github.com:owner/myrepo.git");
        assert_eq!(result, Some(("owner".into(), "myrepo".into())));
    }

    #[test]
    fn test_parse_github_owner_repo_invalid() {
        assert!(parse_github_owner_repo("https://gitlab.com/owner/repo").is_none());
        assert!(parse_github_owner_repo("not-a-url").is_none());
    }

    #[test]
    fn test_clone_repo_invalid_branch() {
        let result = clone_repo("https://github.com/owner/repo", Some("-bad-branch"), None);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid branch name"), "got: {}", msg);
    }

    #[test]
    fn test_clone_repo_invalid_url() {
        let result = clone_repo("https://notgithub.com/owner/repo", None, None);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not a recognised GitHub URL"), "got: {}", msg);
    }

    #[test]
    fn test_clone_repo_out_dir_path() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        let dest = tmp.path().join("clone_target");
        std::fs::create_dir_all(&dest).unwrap();
        // dest exists → triggers pull path; git pull will fail gracefully (no git repo)
        // we just verify the function returns dest without panicking
        let result = clone_repo("https://github.com/owner/repo", None, Some(&dest));
        // pull will fail (not a git repo) but we only emit a warning — result is Ok
        assert!(result.is_ok(), "unexpected err: {:?}", result.err());
        assert_eq!(result.unwrap(), dest);
    }

    #[test]
    fn test_cli_merge_driver_parses() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "merge-driver",
            "/tmp/base.json",
            "/tmp/cur.json",
            "/tmp/oth.json",
        ]);
        assert!(cli.is_ok(), "parse failed: {:?}", cli.err());
        match cli.unwrap().command {
            Command::MergeDriver {
                base,
                current,
                other,
            } => {
                assert_eq!(base, PathBuf::from("/tmp/base.json"));
                assert_eq!(current, PathBuf::from("/tmp/cur.json"));
                assert_eq!(other, PathBuf::from("/tmp/oth.json"));
            }
            other => panic!("Expected MergeDriver, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_merge_driver_union_nodes() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("base.json");
        let current = tmp.path().join("current.json");
        let other = tmp.path().join("other.json");

        let cur_json = r#"{"nodes":[{"id":"a","label":"A"}],"edges":[]}"#;
        let oth_json = r#"{"nodes":[{"id":"b","label":"B"}],"edges":[]}"#;
        std::fs::write(&base, cur_json).unwrap();
        std::fs::write(&current, cur_json).unwrap();
        std::fs::write(&other, oth_json).unwrap();

        // Run merge-driver logic directly (can't call run() — it calls process::exit)
        // Instead exercise the JSON merge logic inline.
        let val_cur: serde_json::Value = serde_json::from_str(cur_json).unwrap();
        let val_oth: serde_json::Value = serde_json::from_str(oth_json).unwrap();

        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merged_nodes: Vec<serde_json::Value> = Vec::new();
        for n in val_cur["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .chain(val_oth["nodes"].as_array().unwrap().iter())
        {
            let id = n
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() || seen_ids.insert(id) {
                merged_nodes.push(n.clone());
            }
        }
        assert_eq!(merged_nodes.len(), 2);
        let ids: Vec<&str> = merged_nodes
            .iter()
            .map(|n| n["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"b"));
        let _ = (base, current, other);
    }

    #[test]
    fn test_cli_merge_driver_dedup_nodes() {
        let cur_json = r#"{"nodes":[{"id":"a","label":"A"}],"edges":[]}"#;
        let oth_json =
            r#"{"nodes":[{"id":"a","label":"A-other"},{"id":"b","label":"B"}],"edges":[]}"#;

        let val_cur: serde_json::Value = serde_json::from_str(cur_json).unwrap();
        let val_oth: serde_json::Value = serde_json::from_str(oth_json).unwrap();

        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merged_nodes: Vec<serde_json::Value> = Vec::new();
        for n in val_cur["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .chain(val_oth["nodes"].as_array().unwrap().iter())
        {
            let id = n
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() || seen_ids.insert(id) {
                merged_nodes.push(n.clone());
            }
        }
        // "a" appears in both — only current's version kept
        assert_eq!(merged_nodes.len(), 2);
        let a_node = merged_nodes
            .iter()
            .find(|n| n["id"].as_str() == Some("a"))
            .unwrap();
        assert_eq!(a_node["label"].as_str(), Some("A"));
    }

    #[test]
    fn test_cli_merge_driver_dedup_edges() {
        let cur_json = r#"{"nodes":[],"edges":[{"source":"a","target":"b","relation":"calls"}]}"#;
        let oth_json = r#"{"nodes":[],"edges":[{"source":"a","target":"b","relation":"calls"},{"source":"b","target":"c","relation":"imports"}]}"#;

        let val_cur: serde_json::Value = serde_json::from_str(cur_json).unwrap();
        let val_oth: serde_json::Value = serde_json::from_str(oth_json).unwrap();

        let mut seen_edges: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merged_edges: Vec<serde_json::Value> = Vec::new();
        for e in val_cur["edges"]
            .as_array()
            .unwrap()
            .iter()
            .chain(val_oth["edges"].as_array().unwrap().iter())
        {
            let src = e.get("source").and_then(|v| v.as_str()).unwrap_or("");
            let tgt = e.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let rel = e.get("relation").and_then(|v| v.as_str()).unwrap_or("");
            let key = format!("{}|{}|{}", src, tgt, rel);
            if seen_edges.insert(key) {
                merged_edges.push(e.clone());
            }
        }
        assert_eq!(merged_edges.len(), 2);
    }

    #[test]
    fn test_cli_merge_driver_links_key_preserved() {
        // NetworkX format uses "links" not "edges"
        let cur_json = r#"{"directed":true,"nodes":[{"id":"a"}],"links":[{"source":"a","target":"b","relation":"calls"}]}"#;
        let oth_json = r#"{"directed":true,"nodes":[{"id":"b"}],"links":[]}"#;

        let val_cur: serde_json::Value = serde_json::from_str(cur_json).unwrap();
        let edges_key = if val_cur.get("links").is_some() {
            "links"
        } else {
            "edges"
        };
        assert_eq!(edges_key, "links");

        let get_edges = |v: &serde_json::Value| -> Vec<serde_json::Value> {
            v.get("edges")
                .or_else(|| v.get("links"))
                .and_then(|e| e.as_array())
                .cloned()
                .unwrap_or_default()
        };
        let edges_cur = get_edges(&val_cur);
        let val_oth: serde_json::Value = serde_json::from_str(oth_json).unwrap();
        let edges_oth = get_edges(&val_oth);
        assert_eq!(edges_cur.len(), 1);
        assert_eq!(edges_oth.len(), 0);

        let mut output = val_cur.clone();
        if let Some(obj) = output.as_object_mut() {
            obj.insert(
                "nodes".to_string(),
                serde_json::json!([{"id":"a"},{"id":"b"}]),
            );
            obj.insert(edges_key.to_string(), serde_json::Value::Array(edges_cur));
        }
        assert!(output.get("links").is_some(), "links key must be preserved");
        assert!(output.get("edges").is_none(), "edges key must not appear");
        let _ = oth_json;
    }

    #[test]
    fn test_cli_prs_default_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "prs"]).unwrap();
        match cli.command {
            Command::Prs {
                base,
                repo,
                limit,
                graph,
            } => {
                assert!(base.is_none());
                assert!(repo.is_none());
                assert_eq!(limit, 50);
                assert!(graph.is_none());
            }
            _ => panic!("expected Prs"),
        }
    }

    #[test]
    fn test_cli_prs_with_base_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "prs", "--base", "main"]).unwrap();
        match cli.command {
            Command::Prs { base, .. } => assert_eq!(base.as_deref(), Some("main")),
            _ => panic!("expected Prs"),
        }
    }

    #[test]
    fn test_cli_prs_with_repo_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "prs", "--repo", "owner/repo"]).unwrap();
        match cli.command {
            Command::Prs { repo, .. } => assert_eq!(repo.as_deref(), Some("owner/repo")),
            _ => panic!("expected Prs"),
        }
    }

    #[test]
    fn test_cli_prs_with_limit_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "prs", "--limit", "25"]).unwrap();
        match cli.command {
            Command::Prs { limit, .. } => assert_eq!(limit, 25),
            _ => panic!("expected Prs"),
        }
    }

    #[test]
    fn test_cli_prs_with_graph_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "prs", "--graph", "out/graph.json"]).unwrap();
        match cli.command {
            Command::Prs { graph, .. } => {
                assert_eq!(graph.unwrap().to_str().unwrap(), "out/graph.json");
            }
            _ => panic!("expected Prs"),
        }
    }

    #[test]
    fn test_cli_prs_all_flags_parses() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "prs",
            "--base",
            "v8",
            "--repo",
            "acme/myrepo",
            "--limit",
            "100",
            "--graph",
            "graph.json",
        ])
        .unwrap();
        match cli.command {
            Command::Prs {
                base,
                repo,
                limit,
                graph,
            } => {
                assert_eq!(base.as_deref(), Some("v8"));
                assert_eq!(repo.as_deref(), Some("acme/myrepo"));
                assert_eq!(limit, 100);
                assert_eq!(graph.unwrap().to_str().unwrap(), "graph.json");
            }
            _ => panic!("expected Prs"),
        }
    }

    #[test]
    fn test_cli_version_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "version"]).unwrap();
        assert!(matches!(cli.command, Command::Version));
    }

    #[test]
    fn test_cli_hook_check_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "hook-check"]).unwrap();
        assert!(matches!(cli.command, Command::HookCheck));
    }

    #[test]
    fn test_cli_check_update_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "check-update", "/some/path"]).unwrap();
        match cli.command {
            Command::CheckUpdate { path } => {
                assert_eq!(path, PathBuf::from("/some/path"));
            }
            _ => panic!("expected CheckUpdate"),
        }
    }

    #[test]
    fn test_cli_cache_check_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "cache-check", "/tmp/files.txt"]).unwrap();
        match cli.command {
            Command::CacheCheck { files_from, root } => {
                assert_eq!(files_from, PathBuf::from("/tmp/files.txt"));
                assert_eq!(root, PathBuf::from("."));
            }
            _ => panic!("expected CacheCheck"),
        }
    }

    #[test]
    fn test_cli_cache_check_with_root_parses() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "cache-check",
            "/tmp/files.txt",
            "--root",
            "/my/project",
        ])
        .unwrap();
        match cli.command {
            Command::CacheCheck { files_from, root } => {
                assert_eq!(files_from, PathBuf::from("/tmp/files.txt"));
                assert_eq!(root, PathBuf::from("/my/project"));
            }
            _ => panic!("expected CacheCheck"),
        }
    }

    #[test]
    fn test_cli_merge_chunks_parses() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "merge-chunks",
            "chunk1.json",
            "chunk2.json",
            "--out",
            "merged.json",
        ])
        .unwrap();
        match cli.command {
            Command::MergeChunks { files, out } => {
                assert_eq!(files.len(), 2);
                assert_eq!(out, PathBuf::from("merged.json"));
            }
            _ => panic!("expected MergeChunks"),
        }
    }

    #[test]
    fn test_cli_merge_semantic_parses() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "merge-semantic",
            "--cached",
            "/tmp/cached.json",
            "--new",
            "/tmp/new.json",
            "--out",
            "/tmp/merged.json",
        ])
        .unwrap();
        match cli.command {
            Command::MergeSemantic { cached, new, out } => {
                assert_eq!(cached.unwrap(), PathBuf::from("/tmp/cached.json"));
                assert_eq!(new.unwrap(), PathBuf::from("/tmp/new.json"));
                assert_eq!(out, PathBuf::from("/tmp/merged.json"));
            }
            _ => panic!("expected MergeSemantic"),
        }
    }

    #[test]
    fn test_merge_chunks_dedup_nodes_by_id() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        let chunk1 = tmp.path().join("c1.json");
        let chunk2 = tmp.path().join("c2.json");
        let out = tmp.path().join("merged.json");
        std::fs::write(
            &chunk1,
            r#"{"nodes":[{"id":"a","label":"A"}],"edges":[],"input_tokens":10,"output_tokens":5}"#,
        )
        .unwrap();
        std::fs::write(&chunk2, r#"{"nodes":[{"id":"a","label":"A-dup"},{"id":"b","label":"B"}],"edges":[],"input_tokens":8,"output_tokens":3}"#).unwrap();

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merged_nodes: Vec<serde_json::Value> = Vec::new();
        let mut input_tokens: u64 = 0;
        for cf in &[&chunk1, &chunk2] {
            let v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(cf).unwrap()).unwrap();
            for n in v["nodes"].as_array().unwrap() {
                let id = n["id"].as_str().unwrap_or("").to_string();
                if id.is_empty() || seen.insert(id) {
                    merged_nodes.push(n.clone());
                }
            }
            input_tokens += v.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        }
        std::fs::write(&out, serde_json::to_string(&serde_json::json!({"nodes": merged_nodes, "edges": [], "input_tokens": input_tokens})).unwrap()).unwrap();

        let result: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        let nodes = result["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2, "duplicate id 'a' must be deduped");
        assert_eq!(
            nodes[0]["label"].as_str().unwrap(),
            "A",
            "first writer wins"
        );
        assert_eq!(result["input_tokens"].as_u64().unwrap(), 18);
    }

    #[test]
    fn test_merge_semantic_cached_takes_priority() {
        let get_arr = |v: &serde_json::Value, key: &str| -> Vec<serde_json::Value> {
            v.get(key)
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default()
        };
        let cached: serde_json::Value =
            serde_json::json!({"nodes":[{"id":"a","label":"cached-A"}],"edges":[],"hyperedges":[]});
        let new: serde_json::Value = serde_json::json!({"nodes":[{"id":"a","label":"new-A"},{"id":"b","label":"B"}],"edges":[{"source":"a","target":"b","relation":"calls"}],"hyperedges":[]});

        let cached_nodes = get_arr(&cached, "nodes");
        let new_nodes = get_arr(&new, "nodes");
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut all_nodes: Vec<serde_json::Value> = Vec::new();
        for n in cached_nodes.iter().chain(new_nodes.iter()) {
            let id = n
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() || seen.insert(id) {
                all_nodes.push(n.clone());
            }
        }
        let all_edges: Vec<serde_json::Value> = get_arr(&cached, "edges")
            .into_iter()
            .chain(get_arr(&new, "edges"))
            .collect();

        assert_eq!(all_nodes.len(), 2);
        assert_eq!(
            all_nodes[0]["label"].as_str().unwrap(),
            "cached-A",
            "cached wins on id conflict"
        );
        assert_eq!(all_edges.len(), 1);
    }

    #[test]
    fn test_check_update_no_flag() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        let flag = tmp.path().join("codesynapse-out").join("needs_update");
        assert!(!flag.exists(), "flag should not exist");
    }

    #[test]
    fn test_check_update_flag_exists() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        let out_dir = tmp.path().join("codesynapse-out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let flag = out_dir.join("needs_update");
        std::fs::write(&flag, "1").unwrap();
        assert!(flag.exists());
    }

    #[test]
    fn test_cli_tree_html_parses() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "tree-html",
            "--graph",
            "/tmp/graph.json",
            "--output",
            "/tmp/tree.html",
        ]);
        assert!(cli.is_ok(), "tree-html should parse: {:?}", cli.err());
        assert!(matches!(cli.unwrap().command, Command::TreeHtml { .. }));
    }

    #[test]
    fn test_cli_tree_html_defaults() {
        let cli = Cli::try_parse_from(["codesynapse", "tree-html"]);
        assert!(cli.is_ok());
        if let Command::TreeHtml {
            graph,
            output,
            root,
            max_children,
            label,
        } = cli.unwrap().command
        {
            assert!(graph.is_none());
            assert!(output.is_none());
            assert!(root.is_none());
            assert_eq!(max_children, 200);
            assert!(label.is_none());
        } else {
            panic!("expected TreeHtml");
        }
    }

    #[test]
    fn test_cli_tree_html_all_flags() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "tree-html",
            "--graph",
            "/tmp/g.json",
            "--output",
            "/tmp/out.html",
            "--root",
            "/src",
            "--max-children",
            "50",
            "--label",
            "MyProject",
        ]);
        assert!(cli.is_ok());
        if let Command::TreeHtml {
            graph,
            output,
            root,
            max_children,
            label,
        } = cli.unwrap().command
        {
            assert_eq!(graph.unwrap().to_str().unwrap(), "/tmp/g.json");
            assert_eq!(output.unwrap().to_str().unwrap(), "/tmp/out.html");
            assert_eq!(root.unwrap(), "/src");
            assert_eq!(max_children, 50);
            assert_eq!(label.unwrap(), "MyProject");
        } else {
            panic!("expected TreeHtml");
        }
    }

    #[test]
    fn test_cli_tree_html_integration() {
        use codesynapse_core::tree_html::write_tree_html;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let graph = serde_json::json!({
            "nodes": [
                {"id": "a", "label": "ApiClient", "source_file": "/proj/src/api.py", "file_type": "code"},
                {"id": "b", "label": "run", "source_file": "/proj/src/main.py", "file_type": "code"}
            ]
        });
        let graph_path = dir.path().join("graph.json");
        std::fs::write(&graph_path, serde_json::to_string(&graph).unwrap()).unwrap();
        let out_path = dir.path().join("tree.html");

        let result = write_tree_html(&graph_path, &out_path, Some("/proj"), 200, Some("TestProj"));
        assert!(result.is_ok(), "write_tree_html failed: {:?}", result.err());
        let html = std::fs::read_to_string(&out_path).unwrap();
        assert!(html.contains("TestProj"));
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("d3.v7"));
    }

    #[test]
    fn test_cli_tree_html_build_tree_only() {
        use codesynapse_core::tree_html::build_tree;
        let graph = serde_json::json!({
            "nodes": [
                {"id": "x", "label": "Foo", "source_file": "/a/b.py", "file_type": "code"},
                {"id": "y", "label": "Bar", "source_file": "/a/c.py", "file_type": "code"}
            ]
        });
        let tree = build_tree(&graph, Some("/a"), 200, None);
        assert_eq!(tree["name"].as_str().unwrap(), "a");
        assert!(tree["total_count"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_cli_setup_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "setup"]);
        assert!(cli.is_ok());
        if let Command::Setup { client, workspace } = cli.unwrap().command {
            assert!(client.is_none());
            assert!(workspace.is_none());
        } else {
            panic!("expected Setup");
        }
    }

    #[test]
    fn test_cli_setup_with_client_flag() {
        let cli = Cli::try_parse_from(["codesynapse", "setup", "--client", "cursor"]);
        assert!(cli.is_ok());
        if let Command::Setup { client, .. } = cli.unwrap().command {
            assert_eq!(client.as_deref(), Some("cursor"));
        } else {
            panic!("expected Setup");
        }
    }

    #[test]
    fn test_merge_mcp_entry_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        let entry = serde_json::json!({"type": "stdio", "command": "/usr/bin/cs", "args": ["mcp"]});
        let ok = merge_mcp_entry(&path, "mcpServers", "codesynapse", entry);
        assert!(ok);
        let text = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            v["mcpServers"]["codesynapse"]["command"].as_str().unwrap(),
            "/usr/bin/cs"
        );
    }

    #[test]
    fn test_merge_mcp_entry_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        let entry = serde_json::json!({"type": "stdio", "command": "/cs", "args": ["mcp"]});
        merge_mcp_entry(&path, "mcpServers", "codesynapse", entry.clone());
        merge_mcp_entry(&path, "mcpServers", "codesynapse", entry);
        let text = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let count = v["mcpServers"].as_object().unwrap().len();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_merge_mcp_entry_preserves_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        let existing = serde_json::json!({"mcpServers": {"other": {"command": "other-tool"}}});
        std::fs::write(&path, serde_json::to_string(&existing).unwrap()).unwrap();
        let entry = serde_json::json!({"command": "/cs", "args": ["mcp"]});
        merge_mcp_entry(&path, "mcpServers", "codesynapse", entry);
        let text = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(v["mcpServers"]["other"].is_object());
        assert!(v["mcpServers"]["codesynapse"].is_object());
    }

    #[test]
    fn test_setup_vscode_writes_servers_key() {
        let tmp = tempfile::tempdir().unwrap();
        let _ = setup_mcp_clients("/usr/bin/codesynapse", Some("vscode"), tmp.path());
        let vscode_path = tmp.path().join(".vscode").join("mcp.json");
        assert!(vscode_path.exists());
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&vscode_path).unwrap()).unwrap();
        assert!(v["servers"]["codesynapse"].is_object());
        assert_eq!(
            v["servers"]["codesynapse"]["command"].as_str().unwrap(),
            "/usr/bin/codesynapse"
        );
    }

    #[test]
    fn test_setup_client_filter_only_registers_target() {
        let tmp = tempfile::tempdir().unwrap();
        let _ = setup_mcp_clients("/cs", Some("vscode"), tmp.path());
        // cursor dir does not exist in tmp → cursor config should not be created
        assert!(!tmp.path().join(".cursor").join("mcp.json").exists());
        // vscode should exist
        assert!(tmp.path().join(".vscode").join("mcp.json").exists());
    }

    #[test]
    fn test_cli_path_parses() {
        let cli =
            Cli::try_parse_from(["codesynapse", "path", "AuthService", "UserService"]).unwrap();
        match cli.command {
            Command::Path {
                source,
                target,
                path,
            } => {
                assert_eq!(source, "AuthService");
                assert_eq!(target, "UserService");
                assert!(path.is_none());
            }
            other => panic!("expected Path, got {:?}", other),
        }
    }

    #[test]
    fn test_cli_path_parses_with_path_flag() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "path",
            "AuthService",
            "UserService",
            "--path",
            "/my/project",
        ])
        .unwrap();
        match cli.command {
            Command::Path {
                source,
                target,
                path,
            } => {
                assert_eq!(source, "AuthService");
                assert_eq!(target, "UserService");
                assert_eq!(path, Some(PathBuf::from("/my/project")));
            }
            other => panic!("expected Path, got {:?}", other),
        }
    }

    #[test]
    fn test_module_add_parses() {
        let cli =
            Cli::try_parse_from(["codesynapse", "module", "add", "core/ctx", "/workspace/ctx"])
                .unwrap();
        match cli.command {
            Command::Module {
                action:
                    ModuleAction::Add {
                        name,
                        source,
                        modules_conf,
                        force,
                        ..
                    },
            } => {
                assert_eq!(name, "core/ctx");
                assert_eq!(source, PathBuf::from("/workspace/ctx"));
                assert!(modules_conf.is_none());
                assert!(!force);
            }
            other => panic!("expected Module::Add, got {:?}", other),
        }
    }

    #[test]
    fn test_module_add_with_modules_conf_and_force() {
        let cli = Cli::try_parse_from([
            "codesynapse",
            "module",
            "add",
            "mymod",
            "/src",
            "--modules-conf",
            "/tmp/custom.conf",
            "--force",
        ])
        .unwrap();
        match cli.command {
            Command::Module {
                action:
                    ModuleAction::Add {
                        modules_conf,
                        force,
                        ..
                    },
            } => {
                assert_eq!(modules_conf, Some(PathBuf::from("/tmp/custom.conf")));
                assert!(force);
            }
            other => panic!("expected Module::Add, got {:?}", other),
        }
    }

    #[test]
    fn test_module_add_llm_flag() {
        let cli = Cli::try_parse_from(["codesynapse", "module", "add", "mymod", "/src", "--llm"])
            .unwrap();
        match cli.command {
            Command::Module {
                action: ModuleAction::Add { llm, .. },
            } => assert!(llm),
            other => panic!("expected Module::Add, got {:?}", other),
        }
    }

    #[test]
    fn test_module_refresh_llm_flag() {
        let cli = Cli::try_parse_from(["codesynapse", "module", "refresh", "--llm"]).unwrap();
        match cli.command {
            Command::Module {
                action: ModuleAction::Refresh { llm, .. },
            } => assert!(llm),
            other => panic!("expected Module::Refresh, got {:?}", other),
        }
    }

    #[test]
    fn test_module_refresh_parses_all() {
        let cli = Cli::try_parse_from(["codesynapse", "module", "refresh"]).unwrap();
        match cli.command {
            Command::Module {
                action: ModuleAction::Refresh { name, .. },
            } => {
                assert!(name.is_none());
            }
            other => panic!("expected Module::Refresh, got {:?}", other),
        }
    }

    #[test]
    fn test_module_refresh_parses_named() {
        let cli = Cli::try_parse_from(["codesynapse", "module", "refresh", "core/ctx"]).unwrap();
        match cli.command {
            Command::Module {
                action: ModuleAction::Refresh { name, .. },
            } => {
                assert_eq!(name.as_deref(), Some("core/ctx"));
            }
            other => panic!("expected Module::Refresh, got {:?}", other),
        }
    }

    #[test]
    fn test_module_list_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "module", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Module {
                action: ModuleAction::List { .. }
            }
        ));
    }

    #[test]
    fn test_module_remove_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "module", "remove", "core/ctx"]).unwrap();
        match cli.command {
            Command::Module {
                action: ModuleAction::Remove { name, .. },
            } => {
                assert_eq!(name, "core/ctx");
            }
            other => panic!("expected Module::Remove, got {:?}", other),
        }
    }

    #[test]
    fn test_read_modules_conf_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("modules.conf");
        std::fs::write(
            &path,
            "core/api|/workspace/api\ncore/impl|/workspace/impl\n",
        )
        .unwrap();
        let modules = read_modules_conf(&path);
        assert_eq!(modules.len(), 2);
        assert_eq!(modules[0].0, "core/api");
        assert_eq!(modules[0].1, PathBuf::from("/workspace/api"));
        assert_eq!(modules[1].0, "core/impl");
        assert_eq!(modules[1].1, PathBuf::from("/workspace/impl"));
    }

    #[test]
    fn test_read_modules_conf_skips_comments_and_blanks() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("modules.conf");
        std::fs::write(
            &path,
            "# comment\n\ncore/api|/workspace/api\n  # indented\n",
        )
        .unwrap();
        let modules = read_modules_conf(&path);
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].0, "core/api");
    }

    #[test]
    fn test_read_modules_conf_missing_file() {
        let modules = read_modules_conf(std::path::Path::new("/nonexistent/modules.conf"));
        assert!(modules.is_empty());
    }

    #[test]
    fn test_upsert_modules_conf_new_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("modules.conf");
        upsert_modules_conf(&path, "core/api", std::path::Path::new("/workspace/api")).unwrap();
        let modules = read_modules_conf(&path);
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].0, "core/api");
        assert_eq!(modules[0].1, PathBuf::from("/workspace/api"));
    }

    #[test]
    fn test_upsert_modules_conf_updates_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("modules.conf");
        std::fs::write(&path, "core/api|/old/path\n").unwrap();
        upsert_modules_conf(&path, "core/api", std::path::Path::new("/new/path")).unwrap();
        let modules = read_modules_conf(&path);
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].1, PathBuf::from("/new/path"));
    }

    #[test]
    fn test_upsert_modules_conf_preserves_comments() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("modules.conf");
        std::fs::write(&path, "# my modules\ncore/api|/workspace/api\n").unwrap();
        upsert_modules_conf(&path, "core/impl", std::path::Path::new("/workspace/impl")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my modules"));
        assert!(text.contains("core/api|/workspace/api"));
        assert!(text.contains("core/impl|/workspace/impl"));
    }

    #[test]
    fn test_remove_from_modules_conf() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("modules.conf");
        std::fs::write(
            &path,
            "# comment\ncore/api|/workspace/api\ncore/impl|/workspace/impl\n",
        )
        .unwrap();
        remove_from_modules_conf(&path, "core/api").unwrap();
        let modules = read_modules_conf(&path);
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].0, "core/impl");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# comment"));
    }

    #[test]
    fn test_find_git_root_finds_parent() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let subdir = tmp.path().join("src").join("main");
        std::fs::create_dir_all(&subdir).unwrap();
        let root = find_git_root(&subdir).unwrap();
        assert_eq!(root, tmp.path());
    }

    #[test]
    fn test_find_git_root_none_when_no_git() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(find_git_root(tmp.path()).is_none());
    }

    #[test]
    fn test_gitignore_codesynapse_artifacts_creates_exclude() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git").join("info")).unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        gitignore_codesynapse_artifacts(&src);
        let exclude = tmp.path().join(".git").join("info").join("exclude");
        let content = std::fs::read_to_string(&exclude).unwrap();
        assert!(content.contains(".codesynapse-store"));
        assert!(content.contains("codesynapse-out"));
    }

    #[test]
    fn test_gitignore_codesynapse_artifacts_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git").join("info")).unwrap();
        let exclude = tmp.path().join(".git").join("info").join("exclude");
        std::fs::write(&exclude, ".codesynapse-store\ncodesynapse-out\n").unwrap();
        gitignore_codesynapse_artifacts(tmp.path());
        let content = std::fs::read_to_string(&exclude).unwrap();
        assert_eq!(content.matches(".codesynapse-store").count(), 1);
        assert_eq!(content.matches("codesynapse-out").count(), 1);
    }

    #[test]
    fn test_has_newer_sources_no_source_files() {
        let tmp = tempfile::tempdir().unwrap();
        let graph = tmp.path().join("graph.json");
        std::fs::write(&graph, "{}").unwrap();
        // empty dir — no code files → not newer
        assert!(!has_newer_sources(tmp.path(), &graph));
    }

    // Fix 1: build_graph_to_json produces graph.json with code nodes (llm=false)
    #[test]
    fn test_build_graph_to_json_without_llm() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(
            src.path().join("main.py"),
            b"class Foo:\n    def bar(self): pass\n",
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let out_json = out.path().join("graph.json");
        let (nodes, _edges) = build_graph_to_json(src.path(), &out_json, false, false).unwrap();
        assert!(nodes > 0, "expected nodes from code extraction");
        assert!(out_json.exists(), "graph.json should be written");
        let content = std::fs::read_to_string(&out_json).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(v["nodes"].as_array().map(|a| a.len()).unwrap_or(0) > 0);
    }

    #[test]
    fn test_watch_all_modules_empty_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        let global_dir = tmp.path().to_path_buf();
        // No modules.conf created → read_modules_conf returns empty vec
        let result = watch_all_modules(global_dir);
        assert!(result.is_err(), "expected Err when no modules registered");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("no modules"),
            "expected 'no modules' in error message, got: {msg}"
        );
    }

    // Fix 1: build_graph_to_json with llm=true but no LLM config still succeeds
    // (extractor build fails gracefully; code nodes still present in output)
    #[test]
    fn test_build_graph_to_json_llm_flag_graceful_failure() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("main.rs"), b"pub struct Foo;\n").unwrap();
        std::fs::write(src.path().join("README.md"), b"# docs\nsome content\n").unwrap();
        let out = tempfile::tempdir().unwrap();
        let out_json = out.path().join("graph.json");
        // llm=true but no OPENAI_API_KEY or config → build_extractor fails → graceful fallback
        let result = build_graph_to_json(src.path(), &out_json, false, true);
        let (nodes, _) = result
            .unwrap_or_else(|e| panic!("should not error when LLM extractor fails to build: {e}"));
        assert!(nodes > 0, "code nodes should still be extracted");
    }

    #[test]
    fn test_cli_resolve_parses() {
        let cli = Cli::try_parse_from(["codesynapse", "resolve", "how does auth work"]);
        assert!(cli.is_ok(), "resolve subcommand should parse");
        match cli.unwrap().command {
            Command::Resolve { query, limit } => {
                assert_eq!(query, "how does auth work");
                assert_eq!(limit, 5);
            }
            other => panic!("Expected Resolve, got {:?}", other),
        }
    }

    #[test]
    fn test_inject_creates_block_in_existing_claude_md() {
        let dir = tempfile::tempdir().unwrap();
        let claude_md = dir.path().join("CLAUDE.md");
        std::fs::write(&claude_md, "# My project\n\nSome existing content.\n").unwrap();

        let global_dir = dir.path().join(".codesynapse");
        std::fs::create_dir_all(&global_dir).unwrap();

        inject_agent_instructions(dir.path(), &global_dir).unwrap();

        let content = std::fs::read_to_string(&claude_md).unwrap();
        assert!(content.contains("<!-- codesynapse:start -->"));
        assert!(content.contains("codesynapse resolve"));
        assert!(content.contains("<!-- codesynapse:end -->"));
        assert!(
            content.contains("# My project"),
            "original content preserved"
        );
    }

    #[test]
    fn test_inject_upserts_existing_block() {
        let dir = tempfile::tempdir().unwrap();
        let claude_md = dir.path().join("CLAUDE.md");
        std::fs::write(
            &claude_md,
            "# My project\n\n<!-- codesynapse:start -->\nOLD BLOCK\n<!-- codesynapse:end -->\n",
        )
        .unwrap();

        let global_dir = dir.path().join(".codesynapse");
        std::fs::create_dir_all(&global_dir).unwrap();

        inject_agent_instructions(dir.path(), &global_dir).unwrap();

        let content = std::fs::read_to_string(&claude_md).unwrap();
        assert!(
            !content.contains("OLD BLOCK"),
            "old block should be replaced"
        );
        assert!(content.contains("codesynapse resolve"), "new block present");
        let count = content.matches("<!-- codesynapse:start -->").count();
        assert_eq!(count, 1, "only one block should exist");
    }

    #[test]
    fn test_strip_marker_block() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("CLAUDE.md");
        std::fs::write(
            &p,
            "# My project\n\n<!-- codesynapse:start -->\nBLOCK CONTENT\n<!-- codesynapse:end -->\n",
        )
        .unwrap();

        strip_marker_block(&p, "<!-- codesynapse:start -->", "<!-- codesynapse:end -->").unwrap();

        let content = std::fs::read_to_string(&p).unwrap();
        assert!(!content.contains("<!-- codesynapse:start -->"));
        assert!(!content.contains("BLOCK CONTENT"));
        assert!(content.contains("# My project"));
    }
}
