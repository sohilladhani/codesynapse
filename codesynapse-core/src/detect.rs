use crate::error::Result;
use crate::types::FileType;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Extension whitelists (mirrors codesynapse/detect.py)
// ---------------------------------------------------------------------------

const CODE_EXTENSIONS: &[&str] = &[
    ".py", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".ejs", ".ets", ".go", ".rs", ".java", ".groovy",
    ".gradle", ".cpp", ".cc", ".cxx", ".c", ".h", ".hpp", ".rb", ".swift", ".kt", ".kts", ".cs",
    ".scala", ".php", ".lua", ".luau", ".toc", ".zig", ".ps1", ".ex", ".exs", ".m", ".mm", ".jl",
    ".vue", ".svelte", ".astro", ".dart", ".v", ".sv", ".svh", ".sql", ".r", ".f", ".F", ".f90",
    ".F90", ".f95", ".F95", ".f03", ".F03", ".f08", ".F08", ".pas", ".pp", ".dpr", ".dpk", ".lpr",
    ".inc", ".dfm", ".lfm", ".lpk", ".sh", ".bash", ".json", ".dm", ".dme", ".dmi", ".dmm", ".dmf",
    ".sln", ".csproj", ".fsproj", ".vbproj", ".razor", ".cshtml", ".yml", ".yaml",
];

const DOC_EXTENSIONS: &[&str] = &[".md", ".mdx", ".qmd", ".txt", ".rst"];

const IMAGE_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".ico", ".bmp",
];

const VIDEO_EXTENSIONS: &[&str] = &[
    ".mp4", ".mp3", ".mov", ".wav", ".webm", ".m4a", ".avi", ".mkv", ".flac", ".ogg", ".opus",
];

const OFFICE_EXTENSIONS: &[&str] = &[".docx", ".xlsx"];

const WORKSPACE_EXTENSIONS: &[&str] = &[".gdoc", ".gsheet", ".gslides", ".gform"];

// ---------------------------------------------------------------------------
// Hard-coded skip lists (mirrors _SKIP_DIRS / _SKIP_FILES, minus "build")
// ---------------------------------------------------------------------------

const SKIP_DIRS: &[&str] = &[
    "venv",
    ".venv",
    "env",
    ".env",
    "node_modules",
    "__pycache__",
    ".git",
    "dist",
    "target",
    "out",
    "site-packages",
    "lib64",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".eggs",
    "codesynapse-out",
    "coverage",
    "lcov-report",
    "visual-tests",
    "visual-test",
    "__snapshots__",
    "snapshots",
    "storybook-static",
    "dist-protected",
    ".next",
    ".nuxt",
    ".turbo",
    ".angular",
    ".idea",
    ".cache",
    ".parcel-cache",
    ".svelte-kit",
    ".terraform",
    ".serverless",
    ".codesynapse",
    ".worktrees",
    "worktrees",
];

const SKIP_FILES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "poetry.lock",
    "Gemfile.lock",
    "composer.lock",
    "go.sum",
    "go.work.sum",
];

// ---------------------------------------------------------------------------
// Paper signal heuristics (mirrors _PAPER_SIGNALS)
// ---------------------------------------------------------------------------

static PAPER_COMPILED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)\barxiv\b").unwrap(),
        Regex::new(r"(?i)\bdoi\s*:").unwrap(),
        Regex::new(r"(?i)\babstract\b").unwrap(),
        Regex::new(r"(?i)\bproceedings\b").unwrap(),
        Regex::new(r"(?i)\bjournal\b").unwrap(),
        Regex::new(r"(?i)\bpreprint\b").unwrap(),
        Regex::new(r"\\cite\{").unwrap(),
        Regex::new(r"\[\d+\]").unwrap(),
        Regex::new(r"(?i)eq\.\s*\d+|equation\s+\d+").unwrap(),
        Regex::new(r"\d{4}\.\d{4,5}").unwrap(),
        Regex::new(r"(?i)\bwe propose\b").unwrap(),
        Regex::new(r"(?i)\bliterature\b").unwrap(),
    ]
});

const PAPER_SIGNAL_THRESHOLD: usize = 3;

static ASSET_DIR_MARKERS: LazyLock<Vec<&str>> = LazyLock::new(|| {
    vec![
        ".imageset",
        ".xcassets",
        ".appiconset",
        ".colorset",
        ".launchimage",
    ]
});

// ---------------------------------------------------------------------------
// Sensitive file patterns (mirrors _SENSITIVE_PATTERNS + _SENSITIVE_DIRS)
// ---------------------------------------------------------------------------

const SENSITIVE_DIRS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".gcloud",
    "secrets",
    ".secrets",
    "credentials",
];

static SENSITIVE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"^\.(env|envrc)(\.|$)").unwrap(),
        Regex::new(r"\.(pem|key|p12|pfx|cert|crt|der|p8)$").unwrap(),
        Regex::new(r"(?i)(^|[._-])(credential|secret|passwd|password|private_key)s?([._-]|$)")
            .unwrap(),
        Regex::new(r"(?i)(^|[._-])tokens?([._-]|$)").unwrap(),
        Regex::new(r"(id_rsa|id_dsa|id_ecdsa|id_ed25519)(\.pub)?$").unwrap(),
        Regex::new(r"(\.netrc|\.pgpass|\.htpasswd)$").unwrap(),
        Regex::new(r"(?i)(aws_credentials|gcloud_credentials|service\.account)").unwrap(),
    ]
});

// ---------------------------------------------------------------------------
// Shebang code interpreters (mirrors _SHEBANG_CODE_INTERPRETERS)
// ---------------------------------------------------------------------------

const SHEBANG_CODE_INTERPRETERS: &[&str] = &[
    "python", "python3", "python2", "ruby", "perl", "node", "nodejs", "bash", "sh", "dash", "zsh",
    "fish", "ksh", "tcsh", "lua", "php", "julia", "Rscript",
];

// ---------------------------------------------------------------------------
// VCS root markers
// ---------------------------------------------------------------------------

const VCS_MARKERS: &[&str] = &[".git", ".hg", ".svn", "_darcs", ".fossil"];

// needs_graph threshold: corpus must have this many files to be graph-worthy
const NEEDS_GRAPH_MIN: usize = 5;

// ---------------------------------------------------------------------------
// DiscoveredFile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub file_type: FileType,
    pub relative_path: String,
}

// ---------------------------------------------------------------------------
// DetectResult
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DetectResult {
    pub files: HashMap<String, Vec<String>>,
    pub total_files: usize,
    pub total_words: usize,
    pub needs_graph: bool,
    pub warning: Option<String>,
    pub codesynapseignore_patterns: usize,
    pub skipped_sensitive: Vec<String>,
}

// ---------------------------------------------------------------------------
// Detector
// ---------------------------------------------------------------------------

pub struct Detector {
    pub follow_symlinks: bool,
    ignore: Gitignore,
    pub codesynapseignore_patterns: usize,
}

impl Detector {
    pub fn new(root: &Path) -> Self {
        Self::new_with_excludes(root, &[])
    }

    pub fn new_with_excludes(root: &Path, extra_excludes: &[&str]) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let (ignore, codesynapseignore_patterns) = load_ignore_patterns(&root, extra_excludes);
        Detector {
            follow_symlinks: false,
            ignore,
            codesynapseignore_patterns,
        }
    }

    pub fn discover(&self, root: &Path) -> Result<Vec<DiscoveredFile>> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut files = Vec::new();
        let mut seen = std::collections::HashSet::new();

        let walk = WalkDir::new(&root)
            .follow_links(self.follow_symlinks)
            .sort_by(|a, b| a.file_name().cmp(b.file_name()));

        for entry in walk
            .into_iter()
            .filter_entry(|e| self.filter_entry(e, &root))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path().to_path_buf();

            if !seen.insert(path.clone()) {
                continue;
            }

            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if SKIP_FILES.contains(&name.as_str()) {
                continue;
            }

            if is_sensitive(&path) {
                continue;
            }

            if self.ignore.matched(&path, false).is_ignore() {
                continue;
            }

            let Some(file_type) = classify_file(&path) else {
                continue;
            };

            let relative = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            files.push(DiscoveredFile {
                path,
                file_type,
                relative_path: relative,
            });
        }

        Ok(files)
    }

    fn filter_entry(&self, entry: &walkdir::DirEntry, _root: &Path) -> bool {
        let name = entry.file_name().to_string_lossy();
        if entry.file_type().is_dir() {
            if is_noise_dir(&name, Some(entry.path())) {
                return false;
            }
            if self.ignore.matched(entry.path(), true).is_ignore() {
                return false;
            }
            true
        } else {
            true
        }
    }

    pub fn detect_incremental(
        &self,
        root: &Path,
        previous_manifest: &[String],
    ) -> Result<Vec<DiscoveredFile>> {
        let all = self.discover(root)?;
        if previous_manifest.is_empty() {
            return Ok(all);
        }
        let previous: std::collections::HashSet<&str> =
            previous_manifest.iter().map(|s| s.as_str()).collect();
        Ok(all
            .into_iter()
            .filter(|f| !previous.contains(f.relative_path.as_str()))
            .collect())
    }
}

impl Default for Detector {
    fn default() -> Self {
        let root = Path::new(".");
        let (ignore, codesynapseignore_patterns) = load_ignore_patterns(root, &[]);
        Detector {
            follow_symlinks: false,
            ignore,
            codesynapseignore_patterns,
        }
    }
}

// ---------------------------------------------------------------------------
// detect() — high-level function returning DetectResult
// ---------------------------------------------------------------------------

pub fn detect(root: &Path, follow_symlinks: Option<bool>, extra_excludes: &[&str]) -> DetectResult {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    let fs = match follow_symlinks {
        Some(v) => v,
        None => auto_detect_follow_symlinks(&root),
    };

    let mut detector = Detector::new_with_excludes(&root, extra_excludes);
    detector.follow_symlinks = fs;

    let discovered = detector.discover(&root).unwrap_or_default();
    let codesynapseignore_patterns = detector.codesynapseignore_patterns;

    let mut files: HashMap<String, Vec<String>> = HashMap::new();
    for key in &["code", "document", "paper", "image", "video"] {
        files.insert(key.to_string(), Vec::new());
    }

    let mut total_words = 0usize;
    let mut skipped_sensitive = Vec::new();

    for f in discovered {
        let path_str = f.path.to_string_lossy().to_string();

        if is_workspace_shortcut(&f.path) {
            skipped_sensitive.push(format!("Google Workspace shortcut skipped: {}", path_str));
            continue;
        }

        let key = match f.file_type {
            FileType::Code => "code",
            FileType::Document => "document",
            FileType::Paper => "paper",
            FileType::Image => "image",
            FileType::Video => "video",
            _ => continue,
        };

        if matches!(
            f.file_type,
            FileType::Code | FileType::Document | FileType::Paper
        ) {
            total_words += count_words(&f.path);
        }

        files.get_mut(key).unwrap().push(path_str);
    }

    let total_files: usize = files.values().map(|v| v.len()).sum();
    let needs_graph = total_files >= NEEDS_GRAPH_MIN;
    let warning = if !needs_graph {
        Some(format!(
            "Corpus has only {} file(s) — a graph may not be meaningful. Add more source files.",
            total_files
        ))
    } else {
        None
    };

    DetectResult {
        files,
        total_files,
        total_words,
        needs_graph,
        warning,
        codesynapseignore_patterns,
        skipped_sensitive,
    }
}

fn auto_detect_follow_symlinks(root: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.path().symlink_metadata() {
                if meta.file_type().is_symlink() {
                    if let Ok(target) = std::fs::metadata(entry.path()) {
                        if target.is_dir() {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// count_words
// ---------------------------------------------------------------------------

pub fn count_words(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|s| s.split_whitespace().count())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Ignore-pattern loading
// ---------------------------------------------------------------------------

fn find_vcs_root(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    let home = dirs_home();
    let mut current = Some(start.as_path());
    while let Some(dir) = current {
        for marker in VCS_MARKERS {
            if dir.join(marker).exists() {
                return Some(dir.to_path_buf());
            }
        }
        let parent = dir.parent()?;
        if parent == dir || Some(parent) == home.as_deref() {
            return None;
        }
        current = Some(parent);
    }
    None
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn path_ancestors(from: &Path, to: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut current = Some(to.to_path_buf());
    let ceiling = from.to_path_buf();
    while let Some(d) = current {
        dirs.push(d.clone());
        if d == ceiling {
            break;
        }
        current = d.parent().map(|p| p.to_path_buf());
    }
    dirs.reverse();
    dirs
}

fn load_ignore_patterns(root: &Path, extra_excludes: &[&str]) -> (Gitignore, usize) {
    let ceiling = find_vcs_root(root).unwrap_or_else(|| root.to_path_buf());
    let mut builder = GitignoreBuilder::new(&ceiling);
    let mut codesynapseignore_count = 0usize;
    let mut found_codesynapseignore = false;

    for ancestor in path_ancestors(&ceiling, root) {
        let ignore_file = ancestor.join(".codesynapseignore");
        if ignore_file.exists() {
            found_codesynapseignore = true;
            if let Ok(content) = std::fs::read_to_string(&ignore_file) {
                codesynapseignore_count += content
                    .lines()
                    .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
                    .count();
            }
            let _ = builder.add(&ignore_file);
        } else if !found_codesynapseignore {
            let gitignore_file = ancestor.join(".gitignore");
            if gitignore_file.exists() {
                let _ = builder.add(&gitignore_file);
            }
        }
    }

    for pattern in extra_excludes {
        let _ = builder.add_line(None, pattern);
    }

    let ignore = builder
        .build()
        .unwrap_or_else(|_| GitignoreBuilder::new(root).build().unwrap());
    (ignore, codesynapseignore_count)
}

// ---------------------------------------------------------------------------
// Noise / sensitive helpers
// ---------------------------------------------------------------------------

fn is_noise_dir(name: &str, path: Option<&std::path::Path>) -> bool {
    let lower = name.to_lowercase();
    if lower.ends_with("_venv") || lower.ends_with("_env") || lower.ends_with(".egg-info") {
        return true;
    }
    // "target" and "out" are legitimate Java package name segments under
    // src/{main,test}/java — don't skip them in that context.
    if lower == "target" || lower == "out" {
        if let Some(p) = path {
            let s = p.to_string_lossy();
            if s.contains("src/main/java") || s.contains("src/test/java") {
                return false;
            }
        }
    }
    SKIP_DIRS.contains(&lower.as_str())
}

pub fn is_sensitive(path: &Path) -> bool {
    // Stage 1: any PARENT directory (not the file itself) is a known sensitive dir
    if let Some(parents) = path.parent() {
        for part in parents.components() {
            if let std::path::Component::Normal(name) = part {
                let name = name.to_string_lossy();
                if SENSITIVE_DIRS.contains(&name.as_ref()) {
                    return true;
                }
            }
        }
    }
    // Stage 2: filename pattern match
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    SENSITIVE_PATTERNS.iter().any(|re| re.is_match(&name))
}

pub fn is_workspace_shortcut(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    WORKSPACE_EXTENSIONS.contains(&ext.as_str())
}

// ---------------------------------------------------------------------------
// File classification
// ---------------------------------------------------------------------------

pub fn classify_file(path: &Path) -> Option<FileType> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    if name.ends_with(".blade.php") {
        return Some(FileType::Code);
    }

    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    if ext.is_empty() {
        return shebang_file_type(path);
    }

    if CODE_EXTENSIONS.contains(&ext.as_str()) {
        return Some(FileType::Code);
    }

    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Some(FileType::Image);
    }

    if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        return Some(FileType::Video);
    }

    if WORKSPACE_EXTENSIONS.contains(&ext.as_str()) {
        return Some(FileType::Document);
    }

    if ext == ".pdf" {
        if let Some(parents) = path.parent() {
            for part in parents.components() {
                if let std::path::Component::Normal(name) = part {
                    let name = name.to_string_lossy();
                    if ASSET_DIR_MARKERS.iter().any(|m| name.ends_with(m)) {
                        return None;
                    }
                }
            }
        }
        return Some(FileType::Paper);
    }

    if DOC_EXTENSIONS.contains(&ext.as_str()) {
        if looks_like_paper(path) {
            return Some(FileType::Paper);
        }
        return Some(FileType::Document);
    }

    if OFFICE_EXTENSIONS.contains(&ext.as_str()) {
        return Some(FileType::Document);
    }

    None
}

fn looks_like_paper(path: &Path) -> bool {
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut byte_end = content.len().min(3000);
    while byte_end > 0 && !content.is_char_boundary(byte_end) {
        byte_end -= 1;
    }
    let head = &content[..byte_end];
    let hits = PAPER_COMPILED.iter().filter(|re| re.is_match(head)).count();
    hits >= PAPER_SIGNAL_THRESHOLD
}

pub fn shebang_interpreter(path: &Path) -> Option<String> {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return None,
    };
    use std::io::Read;
    let mut buf = [0u8; 256];
    let n = f.read(&mut buf).ok()?;
    let first = &buf[..n];
    if !first.starts_with(b"#!") {
        return None;
    }
    let line = match first.splitn(2, |b| *b == b'\n').next() {
        Some(l) => String::from_utf8_lossy(l).to_string(),
        None => return None,
    };
    let line = line[2..].trim().to_string();
    let parts = shlex_split(&line);
    let interp = Path::new(parts.first()?)
        .file_name()?
        .to_string_lossy()
        .to_string();
    if interp == "env" {
        let argv = env_command_args(&parts[1..]);
        Some(
            Path::new(argv.first()?)
                .file_name()?
                .to_string_lossy()
                .to_string(),
        )
    } else {
        Some(interp)
    }
}

fn shebang_file_type(path: &Path) -> Option<FileType> {
    let interp = shebang_interpreter(path)?;
    if SHEBANG_CODE_INTERPRETERS.contains(&interp.as_str()) {
        Some(FileType::Code)
    } else {
        None
    }
}

fn shlex_split(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote_char = ' ';
    let mut escaped = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if escaped {
            current.push(c);
            escaped = false;
            i += 1;
            continue;
        }
        if c == '\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if in_quote {
            if c == quote_char {
                in_quote = false;
            } else {
                current.push(c);
            }
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            in_quote = true;
            quote_char = c;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
            i += 1;
            continue;
        }
        current.push(c);
        i += 1;
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn env_command_args(args: &[String]) -> Vec<String> {
    env_command_args_inner(args, true)
}

fn env_command_args_inner(args: &[String], allow_split: bool) -> Vec<String> {
    let mut i = 0;
    'outer: while i < args.len() {
        let arg = args[i].as_str();

        // End-of-options marker
        if arg == "--" {
            return args[i + 1..].to_vec();
        }

        // Long options
        if arg.starts_with("--") {
            // --split-string with = inline operand
            if let Some(payload) = arg.strip_prefix("--split-string=") {
                if !allow_split {
                    return vec![];
                }
                let sub = shlex_split(payload);
                return env_command_args_inner(&sub, false);
            }
            // --split-string separate operand
            if arg == "--split-string" {
                if !allow_split {
                    return vec![];
                }
                if i + 1 < args.len() {
                    let packed = args[i + 1..].join(" ");
                    let sub = shlex_split(&packed);
                    return env_command_args_inner(&sub, false);
                }
                return vec![];
            }

            // No-operand long flags
            if matches!(
                arg,
                "--ignore-environment" | "--null" | "--debug" | "--list-signal-handling"
            ) {
                i += 1;
                continue;
            }

            // Signal-handling flags (always include = or standalone)
            if arg.starts_with("--default-signal")
                || arg.starts_with("--ignore-signal")
                || arg.starts_with("--block-signal")
            {
                i += 1;
                continue;
            }

            // Long flags with operand (separate or inline = form)
            for &flag in &["--unset", "--chdir", "--argv0"] {
                if arg == flag {
                    i += 2;
                    continue 'outer;
                }
                let flag_eq = flag.len();
                if arg.len() > flag_eq && arg.starts_with(flag) && arg.as_bytes()[flag_eq] == b'=' {
                    i += 1;
                    continue 'outer;
                }
            }

            // Unknown long flag
            return vec![];
        }

        // Short option cluster (e.g. "-i", "-vS", "-uPATH")
        if arg.starts_with('-') && arg.len() > 1 {
            let cluster = &arg[1..];
            let cluster_bytes = cluster.as_bytes();
            let mut j = 0;

            loop {
                if j >= cluster_bytes.len() {
                    i += 1;
                    continue 'outer;
                }
                match cluster_bytes[j] as char {
                    // No-operand short flags
                    '-' | 'i' | '0' | 'v' => {
                        j += 1;
                    }
                    // Short flags that take an operand
                    'u' | 'C' | 'P' | 'a' => {
                        if j + 1 < cluster_bytes.len() {
                            // Clumped: -uVAR → operand is rest of cluster, skip 1 arg
                            i += 1;
                        } else {
                            // Separate: skip flag arg + next arg (the operand)
                            i += 2;
                        }
                        continue 'outer;
                    }
                    // -S / --split-string equivalent
                    'S' => {
                        if !allow_split {
                            return vec![];
                        }
                        if j + 1 < cluster_bytes.len() {
                            // Compact: -Spayload
                            let payload = &cluster[j + 1..];
                            let sub = shlex_split(payload);
                            return env_command_args_inner(&sub, false);
                        } else {
                            // Separate: rest of args is the split-string payload
                            if i + 1 < args.len() {
                                let packed = args[i + 1..].join(" ");
                                let sub = shlex_split(&packed);
                                return env_command_args_inner(&sub, false);
                            }
                            return vec![];
                        }
                    }
                    // Unknown short flag
                    _ => {
                        return vec![];
                    }
                }
            }
        }

        // Single dash (equivalent to --ignore-environment)
        if arg == "-" {
            i += 1;
            continue;
        }

        // Assignment NAME=VALUE
        if arg.contains('=') {
            i += 1;
            continue;
        }

        // First non-option token is the interpreter (start of argv)
        return args[i..].to_vec();
    }
    vec![]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("codesynapse_detect_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn teardown(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    // -----------------------------------------------------------------------
    // classify_file — basic extension tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_classify_python() {
        assert_eq!(classify_file(Path::new("foo.py")), Some(FileType::Code));
    }

    #[test]
    fn test_classify_typescript() {
        assert_eq!(classify_file(Path::new("bar.ts")), Some(FileType::Code));
    }

    #[test]
    fn test_classify_markdown() {
        assert_eq!(
            classify_file(Path::new("README.md")),
            Some(FileType::Document)
        );
    }

    #[test]
    fn test_classify_pdf() {
        assert_eq!(classify_file(Path::new("paper.pdf")), Some(FileType::Paper));
    }

    #[test]
    fn test_classify_pdf_in_xcassets_skipped() {
        let p = Path::new("MyApp/Images.xcassets/icon.imageset/icon.pdf");
        assert_eq!(classify_file(p), None);
    }

    #[test]
    fn test_classify_pdf_in_xcassets_root_skipped() {
        let p = Path::new("Pods/HXPHPicker/Assets.xcassets/photo.pdf");
        assert_eq!(classify_file(p), None);
    }

    #[test]
    fn test_classify_unknown_returns_none() {
        assert_eq!(classify_file(Path::new("archive.zip")), None);
    }

    #[test]
    fn test_classify_image() {
        assert_eq!(
            classify_file(Path::new("screenshot.png")),
            Some(FileType::Image)
        );
        assert_eq!(
            classify_file(Path::new("design.jpg")),
            Some(FileType::Image)
        );
        assert_eq!(
            classify_file(Path::new("diagram.webp")),
            Some(FileType::Image)
        );
    }

    #[test]
    fn test_classify_video_extensions() {
        assert_eq!(
            classify_file(Path::new("lecture.mp4")),
            Some(FileType::Video)
        );
        assert_eq!(
            classify_file(Path::new("podcast.mp3")),
            Some(FileType::Video)
        );
        assert_eq!(classify_file(Path::new("talk.mov")), Some(FileType::Video));
        assert_eq!(
            classify_file(Path::new("recording.wav")),
            Some(FileType::Video)
        );
        assert_eq!(
            classify_file(Path::new("webinar.webm")),
            Some(FileType::Video)
        );
        assert_eq!(classify_file(Path::new("audio.m4a")), Some(FileType::Video));
    }

    #[test]
    fn test_classify_google_workspace_shortcuts() {
        assert_eq!(
            classify_file(Path::new("notes.gdoc")),
            Some(FileType::Document)
        );
        assert_eq!(
            classify_file(Path::new("budget.gsheet")),
            Some(FileType::Document)
        );
        assert_eq!(
            classify_file(Path::new("deck.gslides")),
            Some(FileType::Document)
        );
    }

    #[test]
    fn test_classify_md_paper_by_signals() {
        let dir = setup_test_dir("classify_md_paper");
        let paper = dir.join("paper.md");
        fs::write(
            &paper,
            "# Abstract\n\nWe propose a new method. See [1] and [23].\n\
             This work was published in the Journal of AI. ArXiv preprint.\n\
             See Equation 3 for details. \\cite{vaswani2017}.\n",
        )
        .unwrap();
        assert_eq!(classify_file(&paper), Some(FileType::Paper));
        teardown(&dir);
    }

    #[test]
    fn test_classify_md_doc_without_signals() {
        let dir = setup_test_dir("classify_md_doc");
        let doc = dir.join("notes.md");
        fs::write(
            &doc,
            "# My Notes\n\nHere are some notes about the project.\n",
        )
        .unwrap();
        assert_eq!(classify_file(&doc), Some(FileType::Document));
        teardown(&dir);
    }

    // -----------------------------------------------------------------------
    // count_words
    // -----------------------------------------------------------------------

    #[test]
    fn test_count_words_simple() {
        let dir = setup_test_dir("count_words");
        fs::write(dir.join("doc.md"), "hello world foo bar baz").unwrap();
        assert_eq!(count_words(&dir.join("doc.md")), 5);
        teardown(&dir);
    }

    #[test]
    fn test_count_words_multiline() {
        let dir = setup_test_dir("count_words_multi");
        fs::write(dir.join("doc.txt"), "one two\nthree four\nfive").unwrap();
        assert_eq!(count_words(&dir.join("doc.txt")), 5);
        teardown(&dir);
    }

    // -----------------------------------------------------------------------
    // detect() function
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_finds_files() {
        let dir = setup_test_dir("detect_find_files");
        fs::write(dir.join("main.py"), "print('hi')").unwrap();
        fs::write(dir.join("utils.js"), "const x = 1;").unwrap();
        fs::write(dir.join("readme.md"), "# Docs").unwrap();

        let result = detect(&dir, Some(false), &[]);
        assert!(result.files["code"].len() >= 2);
        assert!(!result.files["document"].is_empty());
        teardown(&dir);
    }

    #[test]
    fn test_detect_warns_small_corpus() {
        let dir = setup_test_dir("detect_small_corpus");
        fs::write(dir.join("main.py"), "x = 1").unwrap();
        fs::write(dir.join("utils.py"), "y = 2").unwrap();

        let result = detect(&dir, Some(false), &[]);
        assert!(
            !result.needs_graph,
            "fewer than 5 files → needs_graph=false"
        );
        assert!(result.warning.is_some(), "small corpus → warning present");
        teardown(&dir);
    }

    #[test]
    fn test_detect_needs_graph_true() {
        let dir = setup_test_dir("detect_needs_graph");
        for i in 0..6 {
            fs::write(dir.join(format!("f{i}.py")), "x = 1").unwrap();
        }
        let result = detect(&dir, Some(false), &[]);
        assert!(result.needs_graph);
        assert!(result.warning.is_none());
        teardown(&dir);
    }

    #[test]
    fn test_detect_skips_noise_dot_dirs() {
        let dir = setup_test_dir("detect_noise_dot");
        fs::create_dir_all(dir.join(".next").join("cache")).unwrap();
        fs::write(dir.join(".next").join("cache").join("build.js"), "x").unwrap();
        fs::create_dir_all(dir.join(".codesynapse").join("cache")).unwrap();
        fs::write(
            dir.join(".codesynapse").join("cache").join("data.json"),
            "{}",
        )
        .unwrap();
        fs::write(dir.join("app.py"), "def go(): pass").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let all: Vec<&str> = result
            .files
            .values()
            .flatten()
            .map(|s| s.as_str())
            .collect();
        assert!(all.iter().any(|f| f.contains("app.py")));
        assert!(!all.iter().any(|f| f.contains(".next")));
        assert!(!all.iter().any(|f| f.contains(".codesynapse")));
        teardown(&dir);
    }

    #[test]
    fn test_detect_includes_video_key() {
        let dir = setup_test_dir("detect_video_key");
        fs::write(dir.join("main.py"), "x = 1").unwrap();

        let result = detect(&dir, Some(false), &[]);
        assert!(result.files.contains_key("video"));
        teardown(&dir);
    }

    #[test]
    fn test_detect_finds_video_files() {
        let dir = setup_test_dir("detect_video_files");
        fs::write(dir.join("lecture.mp4"), b"fake video").unwrap();
        fs::write(dir.join("notes.md"), "# Notes\nSome content here.").unwrap();

        let result = detect(&dir, Some(false), &[]);
        assert_eq!(result.files["video"].len(), 1);
        assert!(result.files["video"][0].contains("lecture.mp4"));
        teardown(&dir);
    }

    #[test]
    fn test_detect_video_not_in_words() {
        let dir = setup_test_dir("detect_video_words");
        fs::write(dir.join("clip.mp4"), &[0u8; 100][..]).unwrap();

        let result = detect(&dir, Some(false), &[]);
        assert_eq!(result.total_words, 0);
        teardown(&dir);
    }

    #[test]
    fn test_detect_skips_google_workspace_shortcuts_by_default() {
        let dir = setup_test_dir("detect_workspace_skip");
        fs::write(dir.join("notes.gdoc"), r#"{"doc_id":"doc-1"}"#).unwrap();

        let result = detect(&dir, Some(false), &[]);
        assert!(result.files["document"].is_empty());
        assert!(result
            .skipped_sensitive
            .iter()
            .any(|s| s.contains("Google Workspace shortcut skipped")));
        teardown(&dir);
    }

    // -----------------------------------------------------------------------
    // codesynapseignore tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_codesynapseignore_excludes_file() {
        let dir = setup_test_dir("codesynapseignore_excludes");
        fs::write(dir.join(".codesynapseignore"), "vendor/\n*.generated.py\n").unwrap();
        fs::create_dir_all(dir.join("vendor")).unwrap();
        fs::write(dir.join("vendor").join("lib.py"), "x = 1").unwrap();
        fs::write(dir.join("main.py"), "print('hi')").unwrap();
        fs::write(dir.join("schema.generated.py"), "x = 1").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("main.py")));
        assert!(!code.iter().any(|f| f.contains("vendor")));
        assert!(!code.iter().any(|f| f.contains("generated")));
        assert_eq!(result.codesynapseignore_patterns, 2);
        teardown(&dir);
    }

    #[test]
    fn test_codesynapseignore_missing_is_fine() {
        let dir = setup_test_dir("codesynapseignore_missing");
        fs::write(dir.join("main.py"), "x = 1").unwrap();

        let result = detect(&dir, Some(false), &[]);
        assert_eq!(result.codesynapseignore_patterns, 0);
        teardown(&dir);
    }

    #[test]
    fn test_codesynapseignore_comments_ignored() {
        let dir = setup_test_dir("codesynapseignore_comments");
        fs::write(
            dir.join(".codesynapseignore"),
            "# this is a comment\n\nmain.py\n",
        )
        .unwrap();
        fs::write(dir.join("main.py"), "x = 1").unwrap();
        fs::write(dir.join("other.py"), "x = 2").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let code = &result.files["code"];
        assert!(!code.iter().any(|f| f.contains("main.py")));
        assert!(code.iter().any(|f| f.contains("other.py")));
        teardown(&dir);
    }

    #[test]
    fn test_codesynapseignore_hermetic_without_vcs() {
        let dir = setup_test_dir("codesynapseignore_hermetic");
        fs::write(dir.join(".codesynapseignore"), "vendor/\n").unwrap();
        let sub = dir.join("packages").join("mylib");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("main.py"), "x = 1").unwrap();
        let vendor = sub.join("vendor");
        fs::create_dir_all(&vendor).unwrap();
        fs::write(vendor.join("dep.py"), "y = 2").unwrap();

        let result = detect(&sub, Some(false), &[]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("main.py")));
        // parent .codesynapseignore must NOT leak (no VCS root)
        assert!(code.iter().any(|f| f.contains("vendor")));
        assert_eq!(result.codesynapseignore_patterns, 0);
        teardown(&dir);
    }

    #[test]
    fn test_codesynapseignore_discovered_from_parent_in_vcs() {
        let dir = setup_test_dir("codesynapseignore_parent_vcs");
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".codesynapseignore"), "vendor/\n").unwrap();
        let sub = dir.join("packages").join("mylib");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("main.py"), "x = 1").unwrap();
        let vendor = sub.join("vendor");
        fs::create_dir_all(&vendor).unwrap();
        fs::write(vendor.join("dep.py"), "y = 2").unwrap();

        let result = detect(&sub, Some(false), &[]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("main.py")));
        assert!(!code.iter().any(|f| f.contains("vendor")));
        assert!(result.codesynapseignore_patterns >= 1);
        teardown(&dir);
    }

    #[test]
    fn test_codesynapseignore_stops_at_git_boundary() {
        let dir = setup_test_dir("codesynapseignore_git_boundary");
        fs::write(dir.join(".codesynapseignore"), "main.py\n").unwrap();
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let sub = repo.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("main.py"), "x = 1").unwrap();

        let result = detect(&sub, Some(false), &[]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("main.py")));
        assert_eq!(result.codesynapseignore_patterns, 0);
        teardown(&dir);
    }

    #[test]
    fn test_codesynapseignore_at_git_root_is_included() {
        let dir = setup_test_dir("codesynapseignore_git_root");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".codesynapseignore"), "vendor/\n").unwrap();
        let sub = repo.join("packages").join("mylib");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("main.py"), "x = 1").unwrap();
        let vendor = sub.join("vendor");
        fs::create_dir_all(&vendor).unwrap();
        fs::write(vendor.join("dep.py"), "y = 2").unwrap();

        let result = detect(&sub, Some(false), &[]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("main.py")));
        assert!(!code.iter().any(|f| f.contains("vendor")));
        assert_eq!(result.codesynapseignore_patterns, 1);
        teardown(&dir);
    }

    // -----------------------------------------------------------------------
    // gitignore fallback tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_gitignore_fallback_when_no_codesynapseignore() {
        let dir = setup_test_dir("gitignore_fallback");
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".gitignore"), "vendor/\n*.generated.py\n").unwrap();
        fs::create_dir_all(dir.join("vendor")).unwrap();
        fs::write(dir.join("vendor").join("lib.py"), "x = 1").unwrap();
        fs::write(dir.join("main.py"), "print('hi')").unwrap();
        fs::write(dir.join("schema.generated.py"), "x = 1").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("main.py")));
        assert!(!code.iter().any(|f| f.contains("vendor")));
        assert!(!code.iter().any(|f| f.contains("generated")));
        teardown(&dir);
    }

    #[test]
    fn test_codesynapseignore_takes_precedence_over_gitignore() {
        let dir = setup_test_dir("codesynapseignore_precedence");
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".gitignore"), "main.py\n").unwrap();
        fs::write(dir.join(".codesynapseignore"), "other.py\n").unwrap();
        fs::write(dir.join("main.py"), "x = 1").unwrap();
        fs::write(dir.join("other.py"), "x = 2").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("main.py")));
        assert!(!code.iter().any(|f| f.contains("other.py")));
        teardown(&dir);
    }

    // -----------------------------------------------------------------------
    // Symlink tests
    // -----------------------------------------------------------------------

    #[test]
    #[cfg(unix)]
    fn test_detect_follows_symlinked_directory() {
        let dir = setup_test_dir("detect_symlink_dir");
        let real_dir = dir.join("real_lib");
        fs::create_dir_all(&real_dir).unwrap();
        fs::write(real_dir.join("util.py"), "x = 1").unwrap();
        std::os::unix::fs::symlink(&real_dir, dir.join("linked_lib")).unwrap();

        let result_no = detect(&dir, Some(false), &[]);
        let result_yes = detect(&dir, Some(true), &[]);

        assert!(result_no.files["code"]
            .iter()
            .any(|f| f.contains("real_lib")));
        assert!(!result_no.files["code"]
            .iter()
            .any(|f| f.contains("linked_lib")));
        assert!(result_yes.files["code"]
            .iter()
            .any(|f| f.contains("linked_lib")));
        teardown(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn test_detect_follows_symlinked_file() {
        let dir = setup_test_dir("detect_symlink_file");
        fs::write(dir.join("real.py"), "x = 1").unwrap();
        std::os::unix::fs::symlink(dir.join("real.py"), dir.join("link.py")).unwrap();

        let result = detect(&dir, Some(true), &[]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("real.py")));
        assert!(code.iter().any(|f| f.contains("link.py")));
        teardown(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn test_detect_handles_circular_symlinks() {
        let dir = setup_test_dir("detect_circular_symlink");
        let sub = dir.join("a");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("main.py"), "x = 1").unwrap();
        std::os::unix::fs::symlink(&dir, sub.join("loop")).unwrap();

        let result = detect(&dir, Some(true), &[]);
        assert!(result.files["code"].iter().any(|f| f.contains("main.py")));
        teardown(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn test_detect_auto_detects_direct_symlink_child() {
        let dir = setup_test_dir("detect_auto_symlink");
        let real_dir = dir.join("real_lib");
        fs::create_dir_all(&real_dir).unwrap();
        fs::write(real_dir.join("util.py"), "x = 1").unwrap();
        std::os::unix::fs::symlink(&real_dir, dir.join("linked_lib")).unwrap();

        // Default (None) → auto-detect → follows because of linked_lib symlink
        let result = detect(&dir, None, &[]);
        assert!(result.files["code"]
            .iter()
            .any(|f| f.contains("linked_lib")));
        teardown(&dir);
    }

    #[test]
    fn test_detect_default_does_not_follow_when_no_symlinks() {
        let dir = setup_test_dir("detect_no_symlinks");
        fs::write(dir.join("main.py"), "x = 1").unwrap();
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("other.py"), "y = 2").unwrap();

        let result = detect(&dir, None, &[]);
        assert!(result.files["code"].iter().any(|f| f.contains("main.py")));
        assert!(result.files["code"].iter().any(|f| f.contains("other.py")));
        teardown(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn test_detect_explicit_false_overrides_auto_detect() {
        let dir = setup_test_dir("detect_explicit_false");
        let real_dir = dir.join("real_lib");
        fs::create_dir_all(&real_dir).unwrap();
        fs::write(real_dir.join("util.py"), "x = 1").unwrap();
        std::os::unix::fs::symlink(&real_dir, dir.join("linked_lib")).unwrap();

        // Explicit false overrides auto-detect
        let result = detect(&dir, Some(false), &[]);
        assert!(!result.files["code"]
            .iter()
            .any(|f| f.contains("linked_lib")));
        teardown(&dir);
    }

    // -----------------------------------------------------------------------
    // Noise dir skip tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_skips_coverage_dir() {
        let dir = setup_test_dir("detect_cvg");
        let cov = dir.join("coverage").join("lcov-report");
        fs::create_dir_all(&cov).unwrap();
        fs::write(cov.join("index.html"), "<html>coverage</html>").unwrap();
        fs::write(dir.join("main.py"), "def hello(): pass").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let all: Vec<&str> = result
            .files
            .values()
            .flatten()
            .map(|s| s.as_str())
            .collect();
        assert!(all.iter().any(|f| f.contains("main.py")));
        let cov_prefix = dir.join("coverage").to_string_lossy().to_string();
        assert!(!all.iter().any(|f| f.starts_with(&cov_prefix)));
        teardown(&dir);
    }

    #[test]
    fn test_detect_skips_visual_tests_dir() {
        let dir = setup_test_dir("detect_visual_tests");
        let vt = dir.join("visual-tests");
        fs::create_dir_all(&vt).unwrap();
        fs::write(vt.join("bundle.js"), "var u3=function(){};").unwrap();
        fs::write(dir.join("app.py"), "def main(): pass").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let all: Vec<&str> = result
            .files
            .values()
            .flatten()
            .map(|s| s.as_str())
            .collect();
        assert!(!all.iter().any(|f| f.contains("visual-tests")));
        assert!(all.iter().any(|f| f.contains("app.py")));
        teardown(&dir);
    }

    #[test]
    fn test_detect_skips_snapshots_dir() {
        let dir = setup_test_dir("detect_snapshots");
        let snaps = dir.join("__snapshots__");
        fs::create_dir_all(&snaps).unwrap();
        fs::write(snaps.join("app.test.ts.snap"), "// Snapshot").unwrap();
        fs::write(dir.join("app.ts"), "export function greet() {}").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let all: Vec<&str> = result
            .files
            .values()
            .flatten()
            .map(|s| s.as_str())
            .collect();
        assert!(!all.iter().any(|f| f.contains("__snapshots__")));
        assert!(all.iter().any(|f| f.contains("app.ts")));
        teardown(&dir);
    }

    #[test]
    fn test_detect_skips_storybook_static_dir() {
        let dir = setup_test_dir("detect_storybook");
        let sb = dir.join("storybook-static");
        fs::create_dir_all(&sb).unwrap();
        fs::write(sb.join("main.js"), "(function(){})()").unwrap();
        fs::write(
            dir.join("Button.tsx"),
            "export const Button = () => <button/>",
        )
        .unwrap();

        let result = detect(&dir, Some(false), &[]);
        let all: Vec<&str> = result
            .files
            .values()
            .flatten()
            .map(|s| s.as_str())
            .collect();
        assert!(!all.iter().any(|f| f.contains("storybook-static")));
        assert!(all.iter().any(|f| f.contains("Button.tsx")));
        teardown(&dir);
    }

    #[test]
    fn test_detect_allows_github_dir() {
        let dir = setup_test_dir("detect_github_dir");
        let gh = dir.join(".github").join("workflows");
        fs::create_dir_all(&gh).unwrap();
        fs::write(gh.join("ci.yml"), "name: CI\non: push\n").unwrap();
        fs::write(dir.join("main.py"), "def run(): pass").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let all: Vec<&str> = result
            .files
            .values()
            .flatten()
            .map(|s| s.as_str())
            .collect();
        assert!(
            all.iter().any(|f| f.contains(".github")),
            ".github/ci.yml should be detected"
        );
        teardown(&dir);
    }

    #[test]
    fn test_detect_skips_next_cache() {
        let dir = setup_test_dir("detect_next_cache");
        let next_dir = dir.join(".next").join("cache");
        fs::create_dir_all(&next_dir).unwrap();
        fs::write(next_dir.join("build.js"), "(function(){})()").unwrap();
        let pages = dir.join("pages");
        fs::create_dir_all(&pages).unwrap();
        fs::write(pages.join("index.tsx"), "export default function Home() {}").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let all: Vec<&str> = result
            .files
            .values()
            .flatten()
            .map(|s| s.as_str())
            .collect();
        assert!(!all.iter().any(|f| f.contains(".next")));
        assert!(all.iter().any(|f| f.contains("index.tsx")));
        teardown(&dir);
    }

    #[test]
    fn test_detect_skips_codesynapse_own_cache() {
        let dir = setup_test_dir("detect_codesynapse_cache");
        let cache = dir.join(".codesynapse").join("cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("abc.json"), r#"{"nodes":[]}"#).unwrap();
        fs::write(dir.join("app.py"), "def go(): pass").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let all: Vec<&str> = result
            .files
            .values()
            .flatten()
            .map(|s| s.as_str())
            .collect();
        assert!(!all.iter().any(|f| f.contains(".codesynapse")));
        assert!(all.iter().any(|f| f.contains("app.py")));
        teardown(&dir);
    }

    #[test]
    fn test_detect_skips_worktrees_dir() {
        let dir = setup_test_dir("detect_worktrees");
        let wt = dir.join(".worktrees").join("feature-branch");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join("main.py"), "x = 1").unwrap();
        fs::write(dir.join("app.py"), "y = 2").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("app.py")));
        assert!(!code.iter().any(|f| f.contains(".worktrees")));
        teardown(&dir);
    }

    #[test]
    fn test_detect_skips_nested_worktrees_dir() {
        let dir = setup_test_dir("detect_nested_wt");
        let wt = dir.join(".claude").join("worktrees").join("feature-branch");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join("main.py"), "x = 1").unwrap();
        fs::write(dir.join("app.py"), "y = 2").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("app.py")));
        let wt_prefix = dir
            .join(".claude")
            .join("worktrees")
            .to_string_lossy()
            .to_string();
        assert!(!code.iter().any(|f| f.starts_with(&wt_prefix)));
        teardown(&dir);
    }

    #[test]
    fn test_detect_extra_excludes_pattern() {
        let dir = setup_test_dir("detect_extra_excludes");
        fs::write(dir.join("main.py"), "x = 1").unwrap();
        fs::write(dir.join("secret.py"), "API_KEY = 'abc'").unwrap();
        let legacy = dir.join("legacy");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("old.py"), "y = 2").unwrap();

        let result = detect(&dir, Some(false), &["secret.py", "legacy/"]);
        let code = &result.files["code"];
        assert!(code.iter().any(|f| f.contains("main.py")));
        assert!(!code.iter().any(|f| f.contains("secret.py")));
        assert!(!code.iter().any(|f| f.contains("legacy")));
        teardown(&dir);
    }

    // -----------------------------------------------------------------------
    // Negation tests (gitignore ! re-include semantics)
    // -----------------------------------------------------------------------

    #[test]
    fn test_negation_cannot_rescue_file_under_excluded_dir() {
        let dir = setup_test_dir("negation_no_rescue");
        let android = dir.join("android").join("app").join("src");
        fs::create_dir_all(&android).unwrap();
        fs::write(android.join("Main.kt"), "fun main() {}").unwrap();
        fs::write(dir.join(".codesynapseignore"), "android/\n!src/\n").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let kt_found = result.files["code"].iter().any(|f| f.contains("Main.kt"));
        assert!(
            !kt_found,
            "Main.kt under android/ must remain ignored even with !src/"
        );
        teardown(&dir);
    }

    #[test]
    fn test_negation_works_when_no_ancestor_excluded() {
        let dir = setup_test_dir("negation_works");
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("keep.py"), "x = 1").unwrap();
        fs::write(dir.join(".codesynapseignore"), "*.py\n!src/keep.py\n").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let found = result.files["code"].iter().any(|f| f.contains("keep.py"));
        assert!(found, "src/keep.py should be un-ignored by !src/keep.py");
        teardown(&dir);
    }

    #[test]
    fn test_negation_ancestor_itself_reincluded() {
        let dir = setup_test_dir("negation_ancestor_reincluded");
        let vendor_lib = dir.join("vendor").join("lib");
        fs::create_dir_all(&vendor_lib).unwrap();
        fs::write(vendor_lib.join("utils.py"), "x = 1").unwrap();
        fs::write(dir.join(".codesynapseignore"), "vendor/\n!vendor/\n").unwrap();

        let result = detect(&dir, Some(false), &[]);
        let found = result.files["code"].iter().any(|f| f.contains("utils.py"));
        assert!(
            found,
            "vendor/ excluded then re-included → file should be visible"
        );
        teardown(&dir);
    }

    // -----------------------------------------------------------------------
    // Sensitive file tests (individual is_sensitive checks)
    // -----------------------------------------------------------------------

    #[test]
    fn test_sensitive_flags_api_token_txt() {
        assert!(is_sensitive(Path::new("api_token.txt")));
    }

    #[test]
    fn test_sensitive_flags_oauth_token_json() {
        assert!(is_sensitive(Path::new("oauth_token.json")));
    }

    #[test]
    fn test_sensitive_flags_underscore_secret() {
        assert!(is_sensitive(Path::new("app_secret.yaml")));
    }

    #[test]
    fn test_sensitive_does_not_flag_tokenizer_py() {
        assert!(!is_sensitive(Path::new("tokenizer.py")));
    }

    #[test]
    fn test_sensitive_does_not_flag_tokenize_py() {
        assert!(!is_sensitive(Path::new("tokenize.py")));
    }

    #[test]
    fn test_sensitive_flags_passwords_py() {
        assert!(is_sensitive(Path::new("passwords.py")));
    }

    #[test]
    fn test_sensitive_flags_ssh_dir() {
        assert!(is_sensitive(Path::new("/home/user/.ssh/id_rsa")));
    }

    #[test]
    fn test_sensitive_flags_secrets_dir() {
        assert!(is_sensitive(Path::new("config/secrets/db.json")));
    }

    #[test]
    fn test_sensitive_flags_token_txt() {
        assert!(is_sensitive(Path::new("token.txt")));
    }

    #[test]
    fn test_sensitive_flags_credentials_json() {
        assert!(is_sensitive(Path::new("credentials.json")));
    }

    #[test]
    fn test_sensitive_does_not_flag_root_file_named_credentials() {
        // Root-level "credentials" has no parent dir named credentials;
        // Stage 1 (dir check) passes. Stage 2 (name pattern) catches it.
        let p = Path::new("credentials");
        assert!(is_sensitive(p));
    }

    #[test]
    fn test_sensitive_secret_handler_txt() {
        // "secret_handler.txt" — "secret" followed by "_" → flagged
        assert!(is_sensitive(Path::new("secret_handler.txt")));
    }

    #[test]
    fn test_sensitive_token_config_yaml() {
        // "token_config.yaml" — "token" followed by "_" → flagged
        assert!(is_sensitive(Path::new("token_config.yaml")));
    }

    // -----------------------------------------------------------------------
    // Shebang interpreter tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_shebang_interpreter_plain() {
        let dir = setup_test_dir("shebang_plain");
        let script = dir.join("plain");
        fs::write(&script, b"#!/usr/bin/python3\nprint('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_single_arg() {
        let dir = setup_test_dir("shebang_env_single");
        let script = dir.join("env_single");
        fs::write(&script, b"#!/usr/bin/env python3\nprint('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_dash_s() {
        let dir = setup_test_dir("shebang_env_dashs");
        let script = dir.join("env_dashs");
        fs::write(&script, b"#!/usr/bin/env -S python3 -u\nprint('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_with_flags() {
        let dir = setup_test_dir("shebang_env_flags");
        let script = dir.join("env_flags");
        fs::write(&script, b"#!/usr/bin/env -i bash\necho hi\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("bash"));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_with_assignment() {
        let dir = setup_test_dir("shebang_env_assign");
        let script = dir.join("env_assign");
        fs::write(&script, b"#!/usr/bin/env DEBUG=1 python3\nprint('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_no_shebang() {
        let dir = setup_test_dir("shebang_none");
        let script = dir.join("no_shebang");
        fs::write(&script, b"print('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script), None);
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_quoted_path() {
        let dir = setup_test_dir("shebang_quoted");
        let script = dir.join("quoted");
        fs::write(&script, b"#!\"/usr/local/bin/python3\"\nprint(\"x\")\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_file_type_classifies_via_interpreter() {
        let dir = setup_test_dir("shebang_file_type");
        let script = dir.join("tool");
        fs::write(&script, b"#!/usr/bin/env -S python3 -u\nprint('x')\n").unwrap();
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_unreadable_returns_none() {
        let dir = setup_test_dir("shebang_missing");
        let missing = dir.join("does_not_exist");
        assert_eq!(shebang_interpreter(&missing), None);
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_unset_with_operand() {
        let dir = setup_test_dir("shebang_env_unset");
        let script = dir.join("env_unset");
        fs::write(
            &script,
            b"#!/usr/bin/env -u PYTHONPATH python3\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_chdir_with_operand() {
        let dir = setup_test_dir("shebang_env_chdir");
        let script = dir.join("env_chdir");
        fs::write(&script, b"#!/usr/bin/env -C /tmp python3\nprint('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_path_with_operand() {
        let dir = setup_test_dir("shebang_env_path");
        let script = dir.join("env_path");
        fs::write(&script, b"#!/usr/bin/env -P /bin python3\nprint('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_dash_s_after_flag() {
        let dir = setup_test_dir("shebang_env_flag_dashs");
        let script = dir.join("env_flag_dash_s");
        fs::write(
            &script,
            b"#!/usr/bin/env -i -S \"python3 -u\"\nprint(\"x\")\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_clumped_u_operand() {
        let dir = setup_test_dir("shebang_env_clumped");
        let script = dir.join("env_clumped");
        fs::write(
            &script,
            b"#!/usr/bin/env -uPYTHONPATH python3\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_missing_operand_returns_none() {
        let dir = setup_test_dir("shebang_env_missing_op");
        let script = dir.join("env_missing_op");
        fs::write(&script, b"#!/usr/bin/env -u\n").unwrap();
        assert_eq!(shebang_interpreter(&script), None);
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_gnu_split_string_equals() {
        let dir = setup_test_dir("shebang_split_eq");
        let script = dir.join("env_split_eq");
        fs::write(
            &script,
            b"#!/usr/bin/env --split-string='python3 -u'\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_gnu_split_string_separate() {
        let dir = setup_test_dir("shebang_split_sep");
        let script = dir.join("env_split_sep");
        fs::write(
            &script,
            b"#!/usr/bin/env --split-string \"python3 -u\"\nprint(\"x\")\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_gnu_argv0_operand() {
        let dir = setup_test_dir("shebang_env_argv0");
        let script = dir.join("env_argv0");
        fs::write(&script, b"#!/usr/bin/env -a alias python3\nprint('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_compact_dash_s() {
        let dir = setup_test_dir("shebang_compact_s");
        let script = dir.join("env_compact_dash_s");
        fs::write(&script, b"#!/usr/bin/env -Spython3 -u\nprint('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_compact_v_then_s() {
        let dir = setup_test_dir("shebang_compact_vs");
        let script = dir.join("env_compact_vs");
        fs::write(&script, b"#!/usr/bin/env -vSpython3 -u\nprint('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_long_unset_separate_operand() {
        let dir = setup_test_dir("shebang_long_unset");
        let script = dir.join("env_long_unset");
        fs::write(
            &script,
            b"#!/usr/bin/env --unset PYTHONPATH python3\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_long_unset_equals() {
        let dir = setup_test_dir("shebang_long_unset_eq");
        let script = dir.join("env_long_unset_eq");
        fs::write(
            &script,
            b"#!/usr/bin/env --unset=PYTHONPATH python3\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_long_chdir_separate_operand() {
        let dir = setup_test_dir("shebang_long_chdir");
        let script = dir.join("env_long_chdir");
        fs::write(
            &script,
            b"#!/usr/bin/env --chdir /tmp python3\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_long_chdir_equals() {
        let dir = setup_test_dir("shebang_long_chdir_eq");
        let script = dir.join("env_long_chdir_eq");
        fs::write(
            &script,
            b"#!/usr/bin/env --chdir=/tmp python3\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_signal_flags() {
        let dir = setup_test_dir("shebang_signal");
        let script = dir.join("env_signal");
        fs::write(
            &script,
            b"#!/usr/bin/env --default-signal=TERM --ignore-signal=PIPE python3\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_unknown_option_returns_none() {
        let dir = setup_test_dir("shebang_unknown");
        let script = dir.join("env_unknown");
        fs::write(&script, b"#!/usr/bin/env --no-such-flag python3\n").unwrap();
        assert_eq!(shebang_interpreter(&script), None);
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_dash_s_assignment_before_interpreter() {
        let dir = setup_test_dir("shebang_s_assign");
        let script = dir.join("env_s_assignment");
        fs::write(
            &script,
            b"#!/usr/bin/env -S PYTHONPATH=/opt/custom:${PYTHONPATH} python3\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_dash_s_flag_before_interpreter() {
        let dir = setup_test_dir("shebang_s_flag");
        let script = dir.join("env_s_flag");
        fs::write(
            &script,
            b"#!/usr/bin/env -S -i OLDUSER=${USER} python3\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_long_split_assignment_before_interpreter() {
        let dir = setup_test_dir("shebang_long_split_assign");
        let script = dir.join("env_long_split_assignment");
        fs::write(
            &script,
            b"#!/usr/bin/env --split-string='PYTHONPATH=/opt/custom:${PYTHONPATH} python3 -u'\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_long_split_flag_before_interpreter() {
        let dir = setup_test_dir("shebang_long_split_flag");
        let script = dir.join("env_long_split_flag");
        fs::write(
            &script,
            b"#!/usr/bin/env --split-string='-i python3 -u'\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_nested_split_string_rejected() {
        let dir = setup_test_dir("shebang_nested_split");
        let script = dir.join("env_nested_split");
        fs::write(&script, b"#!/usr/bin/env -S -S python3 -u\nprint('x')\n").unwrap();
        assert_eq!(shebang_interpreter(&script), None);
        teardown(&dir);
    }

    #[test]
    fn test_shebang_interpreter_env_vs_assignment_before_interpreter() {
        let dir = setup_test_dir("shebang_vs_assign");
        let script = dir.join("env_vs_assignment");
        fs::write(
            &script,
            b"#!/usr/bin/env -vS DEBUG=1 python3 -u\nprint('x')\n",
        )
        .unwrap();
        assert_eq!(shebang_interpreter(&script).as_deref(), Some("python3"));
        assert_eq!(classify_file(&script), Some(FileType::Code));
        teardown(&dir);
    }

    // -----------------------------------------------------------------------
    // Legacy Detector tests (kept from original 22)
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_basic() {
        let dir = setup_test_dir("detect_basic");
        fs::write(dir.join("main.py"), "print('hello')").unwrap();
        fs::write(dir.join("utils.js"), "const x = 1;").unwrap();
        fs::write(dir.join("readme.md"), "# Docs").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        let code_count = files
            .iter()
            .filter(|f| f.file_type == FileType::Code)
            .count();
        let doc_count = files
            .iter()
            .filter(|f| f.file_type == FileType::Document)
            .count();

        assert_eq!(code_count, 2);
        assert_eq!(doc_count, 1);
        teardown(&dir);
    }

    #[test]
    fn test_detect_gitignore() {
        let dir = setup_test_dir("detect_gitignore");
        fs::create_dir_all(dir.join("node_modules")).unwrap();
        fs::write(dir.join("node_modules/package.js"), "// ignored").unwrap();
        fs::write(dir.join("main.py"), "print('ok')").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "main.py");
        teardown(&dir);
    }

    #[test]
    fn test_detect_sensitive() {
        let dir = setup_test_dir("detect_sensitive");
        fs::write(dir.join(".env"), "SECRET=1").unwrap();
        fs::write(dir.join("main.py"), "print('ok')").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "main.py");
        teardown(&dir);
    }

    #[test]
    fn test_detect_noise_dirs() {
        let dir = setup_test_dir("detect_noise_dirs");
        fs::create_dir_all(dir.join("__pycache__")).unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("__pycache__/foo.pyc"), "ignored").unwrap();
        fs::write(dir.join("src/main.py"), "print('ok')").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "src/main.py");
        teardown(&dir);
    }

    #[test]
    fn test_detect_empty_dir() {
        let dir = setup_test_dir("detect_empty");
        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();
        assert!(files.is_empty());
        teardown(&dir);
    }

    #[test]
    fn test_detect_paper_signal() {
        let dir = setup_test_dir("detect_paper");
        let content = "# A Novel Approach\n\n\
                       arXiv: 1706.03762\n\n\
                       Abstract: We propose a new method.\n\n\
                       Our literature review shows that this journal has \
                       published many proceedings on this topic.\n";
        fs::write(dir.join("paper.md"), content).unwrap();
        fs::write(dir.join("notes.txt"), "Just some notes").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        let paper_count = files
            .iter()
            .filter(|f| f.file_type == FileType::Paper)
            .count();
        assert_eq!(paper_count, 1);
        let doc_count = files
            .iter()
            .filter(|f| f.file_type == FileType::Document)
            .count();
        assert_eq!(doc_count, 1);
        teardown(&dir);
    }

    #[test]
    fn test_detect_shebang() {
        let dir = setup_test_dir("detect_shebang");
        fs::write(dir.join("script"), "#!/usr/bin/env python3\nprint('hi')").unwrap();
        fs::write(dir.join("runner"), "#!/usr/bin/bash\necho ok").unwrap();
        fs::write(dir.join("no_shebang"), "just text").unwrap();
        fs::write(dir.join("main.py"), "print('ok')").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        let code_count = files
            .iter()
            .filter(|f| f.file_type == FileType::Code)
            .count();
        assert_eq!(code_count, 3);
        teardown(&dir);
    }

    #[test]
    fn test_detect_build_not_ignored() {
        let dir = setup_test_dir("detect_build_not_ignored");
        fs::create_dir_all(dir.join("src").join("build")).unwrap();
        fs::write(
            dir.join("src").join("build").join("Main.java"),
            "class Main {}",
        )
        .unwrap();
        fs::write(dir.join("main.py"), "print('ok')").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        let src_count = files
            .iter()
            .filter(|f| f.relative_path.starts_with("src"))
            .count();
        assert_eq!(src_count, 1);
        assert_eq!(files.len(), 2);
        teardown(&dir);
    }

    #[test]
    fn test_detect_symlink_follow() {
        let dir = setup_test_dir("detect_symlink");
        fs::create_dir_all(dir.join("real")).unwrap();
        fs::write(dir.join("real").join("target.rs"), "fn main() {}").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
        }

        let mut detector = Detector::new(&dir);
        detector.follow_symlinks = true;
        let files = detector.discover(&dir).unwrap();
        assert!(!files.is_empty());
        teardown(&dir);
    }

    #[test]
    fn test_detect_office_conversion() {
        let dir = setup_test_dir("detect_office");
        fs::write(dir.join("report.docx"), "fake docx content").unwrap();
        fs::write(dir.join("data.xlsx"), "fake xlsx content").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        let doc_count = files
            .iter()
            .filter(|f| f.file_type == FileType::Document)
            .count();
        assert_eq!(doc_count, 2);
        teardown(&dir);
    }

    #[test]
    fn test_detect_incremental() {
        let dir = setup_test_dir("detect_incremental");
        fs::write(dir.join("main.py"), "v1").unwrap();
        fs::write(dir.join("utils.py"), "v1").unwrap();

        let detector = Detector::new(&dir);
        let all = detector.discover(&dir).unwrap();
        assert_eq!(all.len(), 2);

        fs::write(dir.join("new.py"), "new").unwrap();

        let prev: Vec<String> = all.iter().map(|f| f.relative_path.clone()).collect();
        let incremental = detector.detect_incremental(&dir, &prev).unwrap();
        assert_eq!(incremental.len(), 1);
        assert!(incremental[0].relative_path.ends_with("new.py"));
        teardown(&dir);
    }

    #[test]
    fn test_detect_unknown_extension_skipped() {
        let dir = setup_test_dir("detect_unknown_ext");
        fs::write(dir.join("data.bin"), "\x00\x01\x02").unwrap();
        fs::write(dir.join("main.py"), "print('ok')").unwrap();
        fs::write(dir.join("image.png"), "PNG").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 2);
        let types: Vec<FileType> = files.iter().map(|f| f.file_type).collect();
        assert!(types.contains(&FileType::Code));
        assert!(types.contains(&FileType::Image));
        teardown(&dir);
    }

    #[test]
    fn test_detect_lock_files_skipped() {
        let dir = setup_test_dir("detect_lock_files");
        fs::write(dir.join("package-lock.json"), "{}").unwrap();
        fs::write(dir.join("Cargo.lock"), "").unwrap();
        fs::write(dir.join("main.py"), "print('ok')").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "main.py");
        teardown(&dir);
    }

    #[test]
    fn test_detect_skip_files_constants() {
        assert!(SKIP_FILES.contains(&"package-lock.json"));
        assert!(SKIP_FILES.contains(&"yarn.lock"));
        assert!(SKIP_FILES.contains(&"pnpm-lock.yaml"));
        assert!(SKIP_FILES.contains(&"Cargo.lock"));
        assert!(SKIP_FILES.contains(&"poetry.lock"));
        assert!(SKIP_FILES.contains(&"Gemfile.lock"));
        assert!(SKIP_FILES.contains(&"composer.lock"));
        assert!(SKIP_FILES.contains(&"go.sum"));
        assert!(SKIP_FILES.contains(&"go.work.sum"));
    }

    #[test]
    fn test_detect_sensitive_patterns() {
        let dir = setup_test_dir("detect_sensitive_patterns");
        fs::write(dir.join("id_rsa"), "key").unwrap();
        fs::write(dir.join("credentials.txt"), "user:pass").unwrap();
        fs::write(dir.join(".env.local"), "SECRET=1").unwrap();
        fs::write(dir.join("cert.pem"), "cert").unwrap();
        fs::write(dir.join("main.py"), "ok").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "main.py");
        teardown(&dir);
    }

    #[test]
    fn test_detect_build_noise_removed() {
        assert!(
            !SKIP_DIRS.contains(&"build"),
            "build should NOT be in SKIP_DIRS"
        );
    }

    // ---------------------------------------------------------------------------
    // Java package allowlist: target / out under src/{main,test}/java
    // ---------------------------------------------------------------------------

    #[test]
    fn test_is_noise_dir_target_no_context_is_noise() {
        assert!(is_noise_dir("target", None));
    }

    #[test]
    fn test_is_noise_dir_out_no_context_is_noise() {
        assert!(is_noise_dir("out", None));
    }

    #[test]
    fn test_is_noise_dir_target_under_src_main_java_not_noise() {
        let p = std::path::Path::new("src/main/java/com/example/target");
        assert!(!is_noise_dir("target", Some(p)));
    }

    #[test]
    fn test_is_noise_dir_out_under_src_main_java_not_noise() {
        let p = std::path::Path::new("src/main/java/com/example/out");
        assert!(!is_noise_dir("out", Some(p)));
    }

    #[test]
    fn test_is_noise_dir_target_under_src_test_java_not_noise() {
        let p = std::path::Path::new("src/test/java/com/example/target");
        assert!(!is_noise_dir("target", Some(p)));
    }

    #[test]
    fn test_is_noise_dir_out_under_src_test_java_not_noise() {
        let p = std::path::Path::new("src/test/java/com/example/out");
        assert!(!is_noise_dir("out", Some(p)));
    }

    #[test]
    fn test_is_noise_dir_target_outside_java_root_is_noise() {
        let p = std::path::Path::new("build/target");
        assert!(is_noise_dir("target", Some(p)));
    }

    #[test]
    fn test_is_noise_dir_out_outside_java_root_is_noise() {
        let p = std::path::Path::new("generated/out");
        assert!(is_noise_dir("out", Some(p)));
    }

    #[test]
    fn test_detect_target_package_under_java_discovered() {
        let dir = setup_test_dir("detect_java_target_pkg");
        let pkg = dir.join("src/main/java/com/example/target");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join("Foo.java"),
            "package com.example.target; public class Foo {}",
        )
        .unwrap();
        fs::write(dir.join("main.py"), "print('ok')").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        let java_files: Vec<_> = files
            .iter()
            .filter(|f| f.relative_path.ends_with(".java"))
            .collect();
        assert_eq!(
            java_files.len(),
            1,
            "Foo.java inside target package must be discovered"
        );
        assert_eq!(
            java_files[0].relative_path,
            "src/main/java/com/example/target/Foo.java"
        );
        teardown(&dir);
    }

    #[test]
    fn test_detect_out_package_under_java_discovered() {
        let dir = setup_test_dir("detect_java_out_pkg");
        let pkg = dir.join("src/main/java/com/example/out");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join("Bar.java"),
            "package com.example.out; public class Bar {}",
        )
        .unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        let java_files: Vec<_> = files
            .iter()
            .filter(|f| f.relative_path.ends_with(".java"))
            .collect();
        assert_eq!(
            java_files.len(),
            1,
            "Bar.java inside out package must be discovered"
        );
        teardown(&dir);
    }

    #[test]
    fn test_detect_target_package_under_test_java_discovered() {
        let dir = setup_test_dir("detect_java_target_test_pkg");
        let pkg = dir.join("src/test/java/com/example/target");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join("FooTest.java"),
            "package com.example.target; public class FooTest {}",
        )
        .unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        let java_files: Vec<_> = files
            .iter()
            .filter(|f| f.relative_path.ends_with(".java"))
            .collect();
        assert_eq!(
            java_files.len(),
            1,
            "FooTest.java inside target package under src/test/java must be discovered"
        );
        teardown(&dir);
    }

    #[test]
    fn test_detect_bare_target_dir_still_skipped() {
        let dir = setup_test_dir("detect_bare_target");
        fs::create_dir_all(dir.join("target/classes")).unwrap();
        fs::write(dir.join("target/classes/Foo.class"), "compiled").unwrap();
        fs::write(dir.join("main.py"), "print('ok')").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "main.py");
        teardown(&dir);
    }

    #[test]
    fn test_detect_bare_out_dir_still_skipped() {
        let dir = setup_test_dir("detect_bare_out");
        fs::create_dir_all(dir.join("out/production")).unwrap();
        fs::write(dir.join("out/production/Main.class"), "compiled").unwrap();
        fs::write(dir.join("main.py"), "print('ok')").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "main.py");
        teardown(&dir);
    }

    #[test]
    fn test_detect_git_dir_ignored() {
        let dir = setup_test_dir("detect_git");
        fs::create_dir_all(dir.join(".git/objects")).unwrap();
        fs::write(dir.join(".git/config"), "[core]").unwrap();
        fs::write(dir.join("main.py"), "ok").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "main.py");
        teardown(&dir);
    }

    #[test]
    fn test_detect_hidden_dirs_skipped() {
        let dir = setup_test_dir("detect_hidden");
        fs::create_dir_all(dir.join(".venv")).unwrap();
        fs::write(dir.join(".venv/lib.py"), "ignored").unwrap();
        fs::create_dir_all(dir.join(".cache")).unwrap();
        fs::write(dir.join(".cache/data"), "ignored").unwrap();
        fs::write(dir.join("main.py"), "ok").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "main.py");
        teardown(&dir);
    }

    #[test]
    fn test_detect_idea_dir_skipped() {
        let dir = setup_test_dir("detect_idea");
        fs::create_dir_all(dir.join(".idea")).unwrap();
        fs::write(dir.join(".idea/workspace.xml"), "ignored").unwrap();
        fs::write(dir.join("main.py"), "ok").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "main.py");
        teardown(&dir);
    }

    #[test]
    fn test_detect_deeply_nested() {
        let dir = setup_test_dir("detect_deep");
        let mut current = dir.clone();
        for i in 0..20 {
            current = current.join(format!("nested_{i}"));
            fs::create_dir_all(&current).unwrap();
        }
        fs::write(current.join("deep.py"), "x = 1").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        teardown(&dir);
    }

    #[test]
    fn test_detect_only_hidden_dirs() {
        let dir = setup_test_dir("detect_only_hidden");
        fs::create_dir_all(dir.join(".hidden")).unwrap();
        fs::write(dir.join(".hidden/secret.py"), "x = 1").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert!(files.is_empty());
        teardown(&dir);
    }

    #[test]
    fn test_detect_dotfiles_visible() {
        let dir = setup_test_dir("detect_dotfiles");
        fs::write(dir.join(".gitignore"), "*.log").unwrap();
        fs::write(dir.join("main.py"), "ok").unwrap();

        let detector = Detector::new(&dir);
        let files = detector.discover(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_type, FileType::Code);
        teardown(&dir);
    }
}
