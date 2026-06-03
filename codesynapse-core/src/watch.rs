use crate::build::GraphBuilder;
use crate::detect::Detector;
use crate::error::{CodeSynapseError, Result};
use crate::extract::Extractor;
use crate::graph::MemoryGraphStore;
use crate::ts_extract::{
    AstroExtractor, CsprojExtractor, FortranExtractor, JsonPackageExtractor, MarkdownExtractor,
    McpConfigExtractor, ObjCExtractor, PowerShellExtractor, RazorExtractor, SlnExtractor,
    TsBashExtractor, TsCExtractor, TsCSharpExtractor, TsCppExtractor, TsElixirExtractor,
    TsGoExtractor, TsJavaExtractor, TsJavaScriptExtractor, TsJuliaExtractor, TsKotlinExtractor,
    TsPascalExtractor, TsPhpExtractor, TsPythonExtractor, TsRubyExtractor, TsRustExtractor,
    TsSqlExtractor, TsSvelteExtractor, TsSwiftExtractor, TsTypeScriptExtractor, TsVueExtractor,
    VerilogExtractor,
};
use crate::types::{Edge, Node};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::{Event, EventKind, RecursiveMode, Watcher as NotifyWatcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CODESYNAPSE_OUT: &str = "codesynapse-out";
const DEFAULT_DEBOUNCE_MS: u64 = 500;

const VCS_MARKERS: &[&str] = &[".git", ".hg", ".svn", "_darcs", ".fossil"];

pub struct WatchConfig {
    pub root: PathBuf,
    pub debounce_ms: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig {
            root: PathBuf::from("."),
            debounce_ms: DEFAULT_DEBOUNCE_MS,
        }
    }
}

pub struct RebuildResult {
    pub changed_files: Vec<PathBuf>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub elapsed_ms: u64,
}

pub struct Watcher {
    config: WatchConfig,
}

impl Watcher {
    pub fn new(config: WatchConfig) -> Self {
        Watcher { config }
    }

    pub fn run<F>(&self, on_rebuild: F) -> Result<()>
    where
        F: Fn(RebuildResult) + Send + Sync + 'static,
    {
        let root = self
            .config
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.config.root.clone());
        let debounce = Duration::from_millis(self.config.debounce_ms);

        let pending: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
        let last_event: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));

        let pending_clone = Arc::clone(&pending);
        let last_event_clone = Arc::clone(&last_event);
        let root_clone = root.clone();
        let ignore = build_ignore(&root);

        let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let _ = tx.send(res);
        })
        .map_err(|e| CodeSynapseError::Io(std::io::Error::other(e.to_string())))?;

        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| CodeSynapseError::Io(std::io::Error::other(e.to_string())))?;

        let on_rebuild = Arc::new(on_rebuild);

        loop {
            // Drain available events without blocking for long.
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(event)) => {
                    if !is_modification(&event.kind) {
                        continue;
                    }
                    let mut any = false;
                    for path in event.paths {
                        if is_watched_path(&path, &root_clone, &ignore) {
                            pending_clone.lock().unwrap().insert(path);
                            any = true;
                        }
                    }
                    if any {
                        *last_event_clone.lock().unwrap() = Some(Instant::now());
                    }
                }
                Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }

            // Fire rebuild if debounce period has elapsed.
            let should_rebuild = {
                let last = last_event.lock().unwrap();
                last.map(|t| t.elapsed() >= debounce).unwrap_or(false)
            };

            if should_rebuild {
                let changed: Vec<PathBuf> = {
                    let mut lock = pending.lock().unwrap();
                    let batch: Vec<_> = lock.drain().collect();
                    *last_event.lock().unwrap() = None;
                    batch
                };

                if !changed.is_empty() {
                    let result = rebuild(&root, changed);
                    on_rebuild(result);
                }
            }
        }

        Ok(())
    }
}

fn register_all_extractors(extractor: &mut Extractor) {
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
    extractor.register("jl", Box::new(TsJuliaExtractor));
    extractor.register("ex", Box::new(TsElixirExtractor));
    extractor.register("exs", Box::new(TsElixirExtractor));
    extractor.register("pas", Box::new(TsPascalExtractor));
    extractor.register("pp", Box::new(TsPascalExtractor));
    extractor.register("ps1", Box::new(PowerShellExtractor));
    extractor.register("psm1", Box::new(PowerShellExtractor));
    extractor.register("psd1", Box::new(PowerShellExtractor));
    extractor.register("md", Box::new(MarkdownExtractor));
    extractor.register("mdx", Box::new(MarkdownExtractor));
    extractor.register("v", Box::new(VerilogExtractor));
    extractor.register("sv", Box::new(VerilogExtractor));
    extractor.register("svh", Box::new(VerilogExtractor));
    extractor.register("f", Box::new(FortranExtractor));
    extractor.register("f90", Box::new(FortranExtractor));
    extractor.register("f95", Box::new(FortranExtractor));
    extractor.register("f03", Box::new(FortranExtractor));
    extractor.register("f08", Box::new(FortranExtractor));
    extractor.register("m", Box::new(ObjCExtractor));
    extractor.register("mm", Box::new(ObjCExtractor));
    extractor.register("astro", Box::new(AstroExtractor));
    extractor.register("sln", Box::new(SlnExtractor));
    extractor.register("csproj", Box::new(CsprojExtractor));
    extractor.register("razor", Box::new(RazorExtractor));
    extractor.register("cshtml", Box::new(RazorExtractor));
}

fn rebuild(root: &Path, changed_files: Vec<PathBuf>) -> RebuildResult {
    let start = Instant::now();

    let detector = Detector::new(root);
    let all_files = detector.discover(root).unwrap_or_default();

    let mut extractor = Extractor::new();
    register_all_extractors(&mut extractor);
    let file_data: Vec<(PathBuf, Vec<u8>)> = all_files
        .iter()
        .filter_map(|df| std::fs::read(&df.path).ok().map(|b| (df.path.clone(), b)))
        .collect();

    let pairs: Vec<(PathBuf, &[u8])> = file_data
        .iter()
        .map(|(p, b)| (p.clone(), b.as_slice()))
        .collect();

    let results = extractor.extract_all(&pairs);

    let store = Box::new(MemoryGraphStore::new());
    let builder = GraphBuilder::new(store);

    let fragments: Vec<(String, Vec<Node>, Vec<Edge>)> = results
        .into_iter()
        .filter_map(|(path, res)| {
            res.ok()
                .map(|frag| (path.to_string_lossy().to_string(), frag.nodes, frag.edges))
        })
        .collect();

    let _ = builder.build_from_fragments(fragments);

    let nodes = builder.store().get_all_nodes().unwrap_or_default();
    let edges = builder.store().get_all_edges().unwrap_or_default();
    let elapsed_ms = start.elapsed().as_millis() as u64;

    RebuildResult {
        changed_files,
        nodes,
        edges,
        elapsed_ms,
    }
}

fn is_modification(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

fn is_watched_path(path: &Path, _root: &Path, ignore: &Gitignore) -> bool {
    // Skip anything inside the output directory.
    if path.components().any(|c| c.as_os_str() == CODESYNAPSE_OUT) {
        return false;
    }

    // Skip dotfiles and dot-directories.
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        if name.starts_with('.') {
            return false;
        }
    }

    // Skip if the ignore rules say so (check path and all parents for directory patterns).
    if ignore.matched_path_or_any_parents(path, false).is_ignore() {
        return false;
    }

    // Only watch code/doc/image extensions.
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    is_tracked_extension(&ext)
}

fn is_tracked_extension(ext: &str) -> bool {
    const CODE: &[&str] = &[
        ".py", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".ejs", ".ets", ".go", ".rs", ".java",
        ".groovy", ".gradle", ".cpp", ".cc", ".cxx", ".c", ".h", ".hpp", ".rb", ".swift", ".kt",
        ".kts", ".cs", ".scala", ".php", ".lua", ".luau", ".zig", ".ps1", ".ex", ".exs", ".m",
        ".mm", ".jl", ".vue", ".svelte", ".astro", ".dart", ".sql", ".r", ".sh", ".bash", ".json",
        ".pas", ".pp",
    ];
    const DOC: &[&str] = &[".md", ".mdx", ".qmd", ".txt", ".rst"];
    const IMAGE: &[&str] = &[".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"];

    CODE.contains(&ext) || DOC.contains(&ext) || IMAGE.contains(&ext)
}

fn find_vcs_root(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    let mut current: Option<&Path> = Some(&start);
    while let Some(dir) = current {
        for marker in VCS_MARKERS {
            if dir.join(marker).exists() {
                return Some(dir.to_path_buf());
            }
        }
        current = dir.parent();
    }
    None
}

fn build_ignore(root: &Path) -> Gitignore {
    let ceiling = find_vcs_root(root).unwrap_or_else(|| root.to_path_buf());
    let mut builder = GitignoreBuilder::new(&ceiling);

    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut cur = root.to_path_buf();
    loop {
        dirs.push(cur.clone());
        if cur == ceiling {
            break;
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => break,
        }
    }
    dirs.reverse(); // ceiling-first so inner rules win

    for dir in dirs {
        let codesynapseignore = dir.join(".codesynapseignore");
        if codesynapseignore.exists() {
            let _ = builder.add(codesynapseignore);
        } else {
            let gitignore = dir.join(".gitignore");
            if gitignore.exists() {
                let _ = builder.add(gitignore);
            }
        }
    }

    builder
        .build()
        .unwrap_or_else(|_| GitignoreBuilder::new(root).build().unwrap())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    fn make_test_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("codesynapse_watch_test_{suffix}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_py(dir: &Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, content).unwrap();
        p
    }

    // ------------------------------------------------------------------
    // WatchConfig defaults
    // ------------------------------------------------------------------

    #[test]
    fn test_watch_config_defaults() {
        let cfg = WatchConfig::default();
        assert_eq!(cfg.debounce_ms, DEFAULT_DEBOUNCE_MS);
        assert_eq!(cfg.root, PathBuf::from("."));
    }

    // ------------------------------------------------------------------
    // is_watched_path
    // ------------------------------------------------------------------

    #[test]
    fn test_is_watched_path_code_file() {
        let dir = make_test_dir("code_file");
        let ignore = build_ignore(&dir);
        let p = dir.join("main.py");
        assert!(is_watched_path(&p, &dir, &ignore));
    }

    #[test]
    fn test_is_watched_path_doc_file() {
        let dir = make_test_dir("doc_file");
        let ignore = build_ignore(&dir);
        let p = dir.join("README.md");
        assert!(is_watched_path(&p, &dir, &ignore));
    }

    #[test]
    fn test_is_watched_path_dotfile_rejected() {
        let dir = make_test_dir("dotfile");
        let ignore = build_ignore(&dir);
        let p = dir.join(".hidden.py");
        assert!(!is_watched_path(&p, &dir, &ignore));
    }

    #[test]
    fn test_is_watched_path_codesynapse_out_rejected() {
        let dir = make_test_dir("gout");
        let ignore = build_ignore(&dir);
        let p = dir.join("codesynapse-out").join("graph.json");
        assert!(!is_watched_path(&p, &dir, &ignore));
    }

    #[test]
    fn test_is_watched_path_image_accepted() {
        let dir = make_test_dir("image");
        let ignore = build_ignore(&dir);
        let p = dir.join("logo.png");
        assert!(is_watched_path(&p, &dir, &ignore));
    }

    #[test]
    fn test_is_watched_path_unknown_ext_rejected() {
        let dir = make_test_dir("unknown_ext");
        let ignore = build_ignore(&dir);
        let p = dir.join("archive.zip");
        assert!(!is_watched_path(&p, &dir, &ignore));
    }

    // ------------------------------------------------------------------
    // .codesynapseignore respected
    // ------------------------------------------------------------------

    #[test]
    fn test_codesynapseignore_excludes_path() {
        let dir = make_test_dir("codesynapseignore_excludes");
        fs::write(dir.join(".codesynapseignore"), "vendor/\n").unwrap();
        let ignore = build_ignore(&dir);
        let p = dir.join("vendor").join("lib.py");
        // Directory pattern `vendor/` is matched via parents check.
        assert!(ignore.matched_path_or_any_parents(&p, false).is_ignore());
    }

    #[test]
    fn test_codesynapseignore_falls_back_to_gitignore() {
        let dir = make_test_dir("codesynapseignore_gitignore_fallback");
        fs::write(dir.join(".gitignore"), "dist/\n").unwrap();
        let ignore = build_ignore(&dir);
        let p = dir.join("dist").join("bundle.js");
        assert!(ignore.matched_path_or_any_parents(&p, false).is_ignore());
    }

    #[test]
    fn test_codesynapseignore_takes_priority_over_gitignore() {
        let dir = make_test_dir("codesynapseignore_priority");
        fs::write(dir.join(".gitignore"), "src/\n").unwrap();
        fs::write(dir.join(".codesynapseignore"), "!src/\n").unwrap();
        let ignore = build_ignore(&dir);
        let p = dir.join("src").join("main.py");
        assert!(!ignore.matched_path_or_any_parents(&p, false).is_ignore());
    }

    // ------------------------------------------------------------------
    // is_modification
    // ------------------------------------------------------------------

    #[test]
    fn test_is_modification_create() {
        assert!(is_modification(&EventKind::Create(
            notify::event::CreateKind::File
        )));
    }

    #[test]
    fn test_is_modification_modify() {
        assert!(is_modification(&EventKind::Modify(
            notify::event::ModifyKind::Data(notify::event::DataChange::Content)
        )));
    }

    #[test]
    fn test_is_modification_remove() {
        assert!(is_modification(&EventKind::Remove(
            notify::event::RemoveKind::File
        )));
    }

    #[test]
    fn test_is_modification_access_rejected() {
        assert!(!is_modification(&EventKind::Access(
            notify::event::AccessKind::Read
        )));
    }

    // ------------------------------------------------------------------
    // is_tracked_extension
    // ------------------------------------------------------------------

    #[test]
    fn test_tracked_extensions() {
        assert!(is_tracked_extension(".py"));
        assert!(is_tracked_extension(".ts"));
        assert!(is_tracked_extension(".rs"));
        assert!(is_tracked_extension(".md"));
        assert!(is_tracked_extension(".png"));
        assert!(!is_tracked_extension(".zip"));
        assert!(!is_tracked_extension(".exe"));
        assert!(!is_tracked_extension(""));
    }

    // ------------------------------------------------------------------
    // rebuild smoke test
    // ------------------------------------------------------------------

    #[test]
    fn test_rebuild_produces_nodes_for_py_file() {
        let dir = make_test_dir("rebuild_nodes");
        temp_py(
            &dir,
            "hello.py",
            "class Greeter:\n    def greet(self): pass\n",
        );

        let result = rebuild(&dir, vec![dir.join("hello.py")]);

        assert!(
            !result.nodes.is_empty(),
            "expected at least one node from hello.py"
        );
        assert_eq!(result.changed_files.len(), 1);
    }

    #[test]
    fn test_rebuild_empty_dir_no_panic() {
        let dir = make_test_dir("rebuild_empty");
        let result = rebuild(&dir, vec![]);
        assert!(result.elapsed_ms < 5000);
    }

    #[test]
    fn test_rebuild_multi_file() {
        let dir = make_test_dir("rebuild_multi");
        temp_py(&dir, "a.py", "def foo(): pass\n");
        temp_py(&dir, "b.py", "def bar(): pass\n");

        let changed = vec![dir.join("a.py"), dir.join("b.py")];
        let result = rebuild(&dir, changed.clone());

        assert!(result.nodes.len() >= 2);
        assert_eq!(result.changed_files.len(), 2);
    }

    // ------------------------------------------------------------------
    // Watcher integration — fire callback on file change
    // ------------------------------------------------------------------

    #[test]
    fn test_watcher_fires_on_file_write() {
        let root = make_test_dir("watcher_fires");

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let cfg = WatchConfig {
            root: root.clone(),
            debounce_ms: 100,
        };
        let watcher = Arc::new(Watcher::new(cfg));
        let watcher_clone = Arc::clone(&watcher);

        let handle = thread::spawn(move || {
            let _ = watcher_clone.run(move |_result| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            });
        });

        // Give the watcher time to set up.
        thread::sleep(Duration::from_millis(200));

        // Write a tracked file.
        fs::write(root.join("test.py"), "x = 1\n").unwrap();

        // Wait for debounce + rebuild.
        thread::sleep(Duration::from_millis(600));

        assert!(
            counter.load(Ordering::SeqCst) >= 1,
            "callback should have fired at least once"
        );

        drop(handle);
    }

    #[test]
    fn test_watcher_ignores_dotfiles() {
        let root = make_test_dir("watcher_dotfiles");

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let cfg = WatchConfig {
            root: root.clone(),
            debounce_ms: 100,
        };
        let watcher = Arc::new(Watcher::new(cfg));
        let watcher_clone = Arc::clone(&watcher);

        let _handle = thread::spawn(move || {
            let _ = watcher_clone.run(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            });
        });

        thread::sleep(Duration::from_millis(200));

        fs::write(root.join(".hidden.py"), "x = 1\n").unwrap();

        thread::sleep(Duration::from_millis(600));

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "dotfile should not trigger rebuild"
        );
    }

    #[test]
    fn test_watcher_ignores_codesynapse_out() {
        let root = make_test_dir("watcher_gout");
        let out = root.join("codesynapse-out");
        fs::create_dir_all(&out).unwrap();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let cfg = WatchConfig {
            root: root.clone(),
            debounce_ms: 100,
        };
        let watcher = Arc::new(Watcher::new(cfg));
        let watcher_clone = Arc::clone(&watcher);

        let _handle = thread::spawn(move || {
            let _ = watcher_clone.run(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            });
        });

        thread::sleep(Duration::from_millis(200));

        fs::write(out.join("graph.json"), "{}").unwrap();

        thread::sleep(Duration::from_millis(600));

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "codesynapse-out writes should not trigger rebuild"
        );
    }
}
