use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static JS_RESOLVE_EXTS: &[&str] = &[".ts", ".tsx", ".svelte", ".js", ".jsx", ".mjs"];
static JS_INDEX_FILES: &[&str] = &[
    "index.ts",
    "index.tsx",
    "index.svelte",
    "index.js",
    "index.jsx",
    "index.mjs",
];

/// Resolve a JS/TS/Svelte import candidate path to an existing local file.
///
/// Resolution order:
/// 1. The path itself (if it's already a file)
/// 2. `.js` → `.ts` and `.jsx` → `.tsx` (TypeScript ESM convention)
/// 3. Append known JS/TS extensions to the filename
/// 4. Directory index files (only if the candidate is a directory)
pub fn resolve_js_import_path(candidate: &Path) -> Option<PathBuf> {
    let candidate = normalize_path(candidate);
    if candidate.is_file() {
        return Some(candidate);
    }

    // TS ESM convention: .js → .ts, .jsx → .tsx
    if let Some(ext) = candidate.extension().and_then(|e| e.to_str()) {
        match ext {
            "js" => {
                let ts = candidate.with_extension("ts");
                if ts.is_file() {
                    return Some(ts);
                }
            }
            "jsx" => {
                let tsx = candidate.with_extension("tsx");
                if tsx.is_file() {
                    return Some(tsx);
                }
            }
            _ => {}
        }
    }

    // Try appending extensions to the full name
    if let Some(name) = candidate.file_name().and_then(|n| n.to_str()) {
        let parent = candidate.parent().unwrap_or(Path::new("."));
        for &ext in JS_RESOLVE_EXTS {
            let with_ext = parent.join(format!("{}{}", name, ext));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }

    // Directory index fallback
    if candidate.is_dir() {
        for &index_name in JS_INDEX_FILES {
            let index = candidate.join(index_name);
            if index.is_file() {
                return Some(index);
            }
        }
    }

    None
}

/// Resolve a JS/TS module specifier to a local file.
///
/// - Relative paths (starting with `.`) are resolved against `start_dir`.
/// - tsconfig.json `paths` aliases are applied.
/// - pnpm workspace packages are resolved via package.json.
/// - Non-relative bare specifiers that don't match any alias/workspace return None.
pub fn resolve_js_module_path(raw: &str, start_dir: &Path) -> Option<PathBuf> {
    if raw.starts_with('.') {
        return resolve_js_import_path(&start_dir.join(raw));
    }

    let aliases = load_tsconfig_aliases(start_dir);
    for (alias_prefix, alias_base) in &aliases {
        if raw == alias_prefix || raw.starts_with(&format!("{}/", alias_prefix)) {
            let rest = raw[alias_prefix.len()..].trim_start_matches('/');
            let resolved = normalize_path(&Path::new(alias_base).join(rest));
            return resolve_js_import_path(&resolved);
        }
    }

    resolve_workspace_import(raw, start_dir)
}

fn normalize_path(path: &Path) -> PathBuf {
    // Simple normalization: collapse . and ..
    let mut components = Vec::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                components.pop();
            }
            other => components.push(other),
        }
    }
    components.iter().collect()
}

/// Read tsconfig.json compilerOptions.paths aliases from a directory hierarchy.
///
/// Walks up from `start_dir` until a `tsconfig.json` is found.
/// Handles simple `extends` chains (one level deep) and JSONC comments.
pub fn load_tsconfig_aliases(start_dir: &Path) -> HashMap<String, String> {
    let start = match std::fs::canonicalize(start_dir) {
        Ok(p) => p,
        Err(_) => start_dir.to_path_buf(),
    };
    let mut current = Some(start.as_path());
    while let Some(dir) = current {
        let tsconfig = dir.join("tsconfig.json");
        if tsconfig.is_file() {
            return read_tsconfig_aliases(&tsconfig, dir, &mut std::collections::HashSet::new());
        }
        current = dir.parent();
    }
    HashMap::new()
}

fn read_tsconfig_aliases(
    tsconfig_path: &Path,
    base_dir: &Path,
    seen: &mut std::collections::HashSet<PathBuf>,
) -> HashMap<String, String> {
    if !seen.insert(tsconfig_path.to_path_buf()) {
        return HashMap::new();
    }
    let text = match std::fs::read_to_string(tsconfig_path) {
        Ok(t) => t,
        Err(_) => return HashMap::new(),
    };
    let stripped = strip_jsonc(&text);
    let data: serde_json::Value = match serde_json::from_str(&stripped) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    let mut aliases = HashMap::new();

    // Handle `extends` — may be a string or an array of strings
    if let Some(extends_field) = data.get("extends") {
        // Collect all extends paths into a list
        let extend_strs: Vec<&str> = if let Some(s) = extends_field.as_str() {
            vec![s]
        } else if let Some(arr) = extends_field.as_array() {
            arr.iter().filter_map(|v| v.as_str()).collect()
        } else {
            vec![]
        };
        for extends_val in extend_strs {
            let extended_path = base_dir.join(extends_val);
            let extended_path = normalize_path(&extended_path);
            if extended_path.is_file() {
                let parent = extended_path.parent().unwrap_or(Path::new("."));
                aliases.extend(read_tsconfig_aliases(&extended_path, parent, seen));
            }
        }
    }

    if let Some(paths) = data
        .get("compilerOptions")
        .and_then(|co| co.get("paths"))
        .and_then(|p| p.as_object())
    {
        for (alias, targets) in paths {
            let targets = match targets.as_array() {
                Some(t) if !t.is_empty() => t,
                _ => continue,
            };
            let target_base = match targets[0].as_str() {
                Some(t) => t.trim_end_matches("/*"),
                None => continue,
            };
            let alias_prefix = alias.trim_end_matches("/*").to_string();
            let resolved_base = normalize_path(&base_dir.join(target_base));
            aliases.insert(alias_prefix, resolved_base.to_string_lossy().into_owned());
        }
    }

    aliases
}

/// Strip JSONC comments and trailing commas so serde_json can parse it.
pub fn strip_jsonc(text: &str) -> String {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let re =
        PATTERN.get_or_init(|| Regex::new(r#""(?:\\.|[^"\\])*"|/\*[\s\S]*?\*/|//[^\n]*"#).unwrap());
    let result = re.replace_all(text, |caps: &regex::Captures| {
        let m = caps.get(0).unwrap().as_str();
        if m.starts_with('"') {
            m.to_string()
        } else {
            String::new()
        }
    });
    // Remove trailing commas before } or ]
    static TRAILING_COMMA: OnceLock<Regex> = OnceLock::new();
    let re2 = TRAILING_COMMA.get_or_init(|| Regex::new(r",(\s*[}\]])").unwrap());
    re2.replace_all(&result, "$1").into_owned()
}

fn resolve_workspace_import(raw: &str, start_dir: &Path) -> Option<PathBuf> {
    let packages = load_workspace_packages(start_dir);
    for (package_name, package_dir) in &packages {
        let subpath = if raw == package_name {
            ""
        } else if raw.starts_with(&format!("{}/", package_name)) {
            &raw[package_name.len() + 1..]
        } else {
            continue;
        };
        for candidate in package_entry_candidates(package_dir, subpath) {
            if let Some(resolved) = resolve_js_import_path(&candidate) {
                return Some(resolved);
            }
        }
    }
    None
}

fn load_workspace_packages(start_dir: &Path) -> HashMap<String, PathBuf> {
    let root = match find_workspace_root(start_dir) {
        Some(r) => r,
        None => return HashMap::new(),
    };
    let workspace_file = root.join("pnpm-workspace.yaml");
    let mut packages = HashMap::new();
    for pattern in workspace_globs(&workspace_file) {
        for entry in root.glob_iter(&pattern) {
            let package_dir = match entry {
                Ok(p) => p,
                Err(_) => continue,
            };
            let manifest = package_dir.join("package.json");
            if !manifest.is_file() {
                continue;
            }
            let data: serde_json::Value = match std::fs::read_to_string(&manifest)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
            {
                Some(v) => v,
                None => continue,
            };
            if let Some(name) = data.get("name").and_then(|v| v.as_str()) {
                if !name.is_empty() {
                    packages.insert(name.to_string(), package_dir);
                }
            }
        }
    }
    packages
}

fn find_workspace_root(start_dir: &Path) -> Option<PathBuf> {
    let start = std::fs::canonicalize(start_dir).unwrap_or_else(|_| start_dir.to_path_buf());
    let mut current = Some(start.as_path());
    while let Some(dir) = current {
        if dir.join("pnpm-workspace.yaml").is_file() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

fn workspace_globs(workspace_file: &Path) -> Vec<String> {
    let text = match std::fs::read_to_string(workspace_file) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut globs = Vec::new();
    let mut in_packages = false;
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        if stripped.starts_with("packages:") {
            in_packages = true;
            continue;
        }
        if in_packages && stripped.starts_with('-') {
            let value = stripped[1..].trim().trim_matches(|c| c == '\'' || c == '"');
            if !value.is_empty() && !value.starts_with('!') {
                globs.push(value.to_string());
            }
        } else if in_packages && !line.starts_with(' ') && !line.starts_with('\t') {
            break;
        }
    }
    globs
}

fn package_entry_candidates(package_dir: &Path, subpath: &str) -> Vec<PathBuf> {
    if !subpath.is_empty() {
        return vec![package_dir.join(subpath)];
    }
    let manifest = package_dir.join("package.json");
    let data: serde_json::Value = std::fs::read_to_string(&manifest)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::Value::Null);

    // Check exports field
    if let Some(exports) = data.get("exports") {
        if let Some(s) = exports.as_str() {
            return vec![package_dir.join(s)];
        }
        if let Some(obj) = exports.as_object() {
            if let Some(dot) = obj.get(".") {
                if let Some(s) = dot.as_str() {
                    return vec![package_dir.join(s)];
                }
                if let Some(dot_obj) = dot.as_object() {
                    for key in &["types", "import", "default", "svelte"] {
                        if let Some(s) = dot_obj.get(*key).and_then(|v| v.as_str()) {
                            return vec![package_dir.join(s)];
                        }
                    }
                }
            }
        }
    }

    let mut candidates = Vec::new();
    for key in &["svelte", "module", "main", "types"] {
        if let Some(s) = data.get(*key).and_then(|v| v.as_str()) {
            candidates.push(package_dir.join(s));
        }
    }
    candidates.push(package_dir.join("src/index"));
    candidates.push(package_dir.join("index"));
    candidates
}

// Internal glob helper on PathBuf so we don't need an extra trait
trait GlobIter {
    fn glob_iter(&self, pattern: &str) -> Vec<std::result::Result<PathBuf, glob::GlobError>>;
}

impl GlobIter for PathBuf {
    fn glob_iter(&self, pattern: &str) -> Vec<std::result::Result<PathBuf, glob::GlobError>> {
        let full_pattern = self.join(pattern);
        let pattern_str = full_pattern.to_string_lossy();
        match glob::glob(&pattern_str) {
            Ok(paths) => paths.collect(),
            Err(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::TempDir::new().unwrap()
    }

    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }

    // --- resolve_js_import_path: direct file ---

    #[test]
    fn test_exact_file_returned() {
        let dir = tmp();
        let p = write(dir.path(), "foo.ts", "");
        assert_eq!(resolve_js_import_path(&p), Some(p));
    }

    #[test]
    fn test_nonexistent_returns_none() {
        let dir = tmp();
        assert_eq!(resolve_js_import_path(&dir.path().join("nope.ts")), None);
    }

    // --- .js → .ts rewrite ---

    #[test]
    fn test_js_to_ts_rewrite() {
        let dir = tmp();
        let ts = write(dir.path(), "helper.ts", "");
        let js_path = dir.path().join("helper.js");
        assert_eq!(resolve_js_import_path(&js_path), Some(ts));
    }

    #[test]
    fn test_jsx_to_tsx_rewrite() {
        let dir = tmp();
        let tsx = write(dir.path(), "component.tsx", "");
        let jsx_path = dir.path().join("component.jsx");
        assert_eq!(resolve_js_import_path(&jsx_path), Some(tsx));
    }

    // --- extension appending ---

    #[test]
    fn test_extensionless_resolves_to_ts() {
        let dir = tmp();
        let ts = write(dir.path(), "utils.ts", "");
        let bare = dir.path().join("utils");
        assert_eq!(resolve_js_import_path(&bare), Some(ts));
    }

    #[test]
    fn test_extensionless_resolves_to_tsx() {
        let dir = tmp();
        let tsx = write(dir.path(), "comp.tsx", "");
        let bare = dir.path().join("comp");
        assert_eq!(resolve_js_import_path(&bare), Some(tsx));
    }

    #[test]
    fn test_extensionless_resolves_to_js_when_no_ts() {
        let dir = tmp();
        let js = write(dir.path(), "legacy.js", "");
        let bare = dir.path().join("legacy");
        assert_eq!(resolve_js_import_path(&bare), Some(js));
    }

    #[test]
    fn test_extensionless_resolves_to_svelte() {
        let dir = tmp();
        let svelte = write(dir.path(), "Button.svelte", "");
        let bare = dir.path().join("Button");
        assert_eq!(resolve_js_import_path(&bare), Some(svelte));
    }

    #[test]
    fn test_extensionless_resolves_to_mjs() {
        let dir = tmp();
        let mjs = write(dir.path(), "esm.mjs", "");
        let bare = dir.path().join("esm");
        assert_eq!(resolve_js_import_path(&bare), Some(mjs));
    }

    // --- directory index fallback ---

    #[test]
    fn test_dir_index_ts() {
        let dir = tmp();
        let idx = write(dir.path(), "mymod/index.ts", "");
        let bare = dir.path().join("mymod");
        assert_eq!(resolve_js_import_path(&bare), Some(idx));
    }

    #[test]
    fn test_dir_index_js_when_no_ts() {
        let dir = tmp();
        let idx = write(dir.path(), "mymod/index.js", "");
        let bare = dir.path().join("mymod");
        assert_eq!(resolve_js_import_path(&bare), Some(idx));
    }

    // Extension appending takes priority over directory index
    #[test]
    fn test_extension_appended_before_directory_index() {
        let dir = tmp();
        let _ = write(dir.path(), "mod/index.ts", "");
        let ts_file = write(dir.path(), "mod.ts", "");
        let bare = dir.path().join("mod");
        assert_eq!(resolve_js_import_path(&bare), Some(ts_file));
    }

    // --- resolve_js_module_path: relative path ---

    #[test]
    fn test_relative_path_resolution() {
        let dir = tmp();
        let ts = write(dir.path(), "lib/helper.ts", "");
        let result = resolve_js_module_path("./helper", &dir.path().join("lib"));
        assert_eq!(result, Some(ts));
    }

    #[test]
    fn test_relative_path_parent_dir() {
        let dir = tmp();
        let ts = write(dir.path(), "shared.ts", "");
        let result = resolve_js_module_path("../shared", &dir.path().join("subdir"));
        assert_eq!(result, Some(ts));
    }

    // --- tsconfig alias resolution ---

    #[test]
    fn test_tsconfig_alias_resolution() {
        let dir = tmp();
        let ts = write(dir.path(), "src/utils/helper.ts", "");
        let tsconfig =
            r#"{"compilerOptions": {"paths": {"@utils/*": ["src/utils/*"]}}}"#.to_string();
        write(dir.path(), "tsconfig.json", &tsconfig);
        let result = resolve_js_module_path("@utils/helper", dir.path());
        assert_eq!(result, Some(ts));
    }

    #[test]
    fn test_tsconfig_alias_exact_match() {
        let dir = tmp();
        let ts = write(dir.path(), "src/lib.ts", "");
        let tsconfig = r#"{"compilerOptions": {"paths": {"@lib": ["src/lib"]}}}"#.to_string();
        write(dir.path(), "tsconfig.json", &tsconfig);
        let result = resolve_js_module_path("@lib", dir.path());
        assert_eq!(result, Some(ts));
    }

    #[test]
    fn test_no_tsconfig_bare_import_returns_none() {
        let dir = tmp();
        let result = resolve_js_module_path("some-external-package", dir.path());
        assert!(result.is_none());
    }

    // --- strip_jsonc ---

    #[test]
    fn test_strip_jsonc_line_comment() {
        let input = r#"{"key": "value" // comment
}"#;
        let stripped = strip_jsonc(input);
        let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(parsed["key"], "value");
    }

    #[test]
    fn test_strip_jsonc_block_comment() {
        let input = r#"{"key": /* block comment */ "value"}"#;
        let stripped = strip_jsonc(input);
        let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(parsed["key"], "value");
    }

    #[test]
    fn test_strip_jsonc_trailing_comma() {
        let input = r#"{"a": 1, "b": 2,}"#;
        let stripped = strip_jsonc(input);
        let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(parsed["a"], 1);
        assert_eq!(parsed["b"], 2);
    }

    #[test]
    fn test_strip_jsonc_preserves_url_in_string() {
        let input = r#"{"url": "https://example.com/path"}"#;
        let stripped = strip_jsonc(input);
        let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(parsed["url"], "https://example.com/path");
    }

    // --- load_tsconfig_aliases ---

    #[test]
    fn test_load_tsconfig_from_parent_dir() {
        let dir = tmp();
        let tsconfig = r#"{"compilerOptions": {"paths": {"@app/*": ["app/*"]}}}"#;
        write(dir.path(), "tsconfig.json", tsconfig);
        let subdir = dir.path().join("src");
        fs::create_dir_all(&subdir).unwrap();
        let aliases = load_tsconfig_aliases(&subdir);
        assert!(aliases.contains_key("@app"));
    }

    #[test]
    fn test_load_tsconfig_no_paths_empty() {
        let dir = tmp();
        let tsconfig = r#"{"compilerOptions": {}}"#;
        write(dir.path(), "tsconfig.json", tsconfig);
        let aliases = load_tsconfig_aliases(dir.path());
        assert!(aliases.is_empty());
    }

    #[test]
    fn test_no_tsconfig_returns_empty() {
        let dir = tmp();
        let aliases = load_tsconfig_aliases(dir.path());
        assert!(aliases.is_empty());
    }

    // --- workspace_globs ---

    #[test]
    fn test_workspace_globs_parsed() {
        let dir = tmp();
        let yaml = "packages:\n  - 'packages/*'\n  - 'apps/*'\n";
        write(dir.path(), "pnpm-workspace.yaml", yaml);
        let globs = workspace_globs(&dir.path().join("pnpm-workspace.yaml"));
        assert_eq!(globs, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn test_workspace_globs_ignores_negations() {
        let dir = tmp();
        let yaml = "packages:\n  - 'packages/*'\n  - '!packages/internal'\n";
        write(dir.path(), "pnpm-workspace.yaml", yaml);
        let globs = workspace_globs(&dir.path().join("pnpm-workspace.yaml"));
        assert_eq!(globs, vec!["packages/*"]);
    }
}
