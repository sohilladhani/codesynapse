//! Tree-sitter based language extractors.
//!
//! Each language gets its own submodule. Shared helpers live here.

use crate::error::{CodeSynapseError, Result};
use crate::extract::path_to_file_id;
use crate::types::{Edge, ExtractionFragment, Node, NodeId};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Language, Node as TsNode, Parser, Query, QueryCursor, StreamingIterator};

fn node_text<'a>(source: &'a [u8], node: &TsNode) -> Result<&'a str> {
    let bytes = &source[node.start_byte()..node.end_byte()];
    std::str::from_utf8(bytes).map_err(|e| CodeSynapseError::Parse(format!("invalid UTF-8: {}", e)))
}

fn make_file_node(path: &Path) -> (NodeId, String, Node) {
    let file_label = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let file_id = path_to_file_id(path);
    let node = Node {
        id: file_id.clone(),
        label: file_label.clone(),
        file_type: "code".to_string(),
        source_file: path.to_string_lossy().to_string(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    };
    (file_id, file_label, node)
}

fn add_contains_edge(
    fragment: &mut ExtractionFragment,
    file_id: &NodeId,
    target_id: NodeId,
    path: &Path,
) {
    fragment.edges.push(Edge {
        source: file_id.clone(),
        target: target_id,
        relation: "contains".to_string(),
        confidence: "EXTRACTED".to_string(),
        source_file: Some(path.to_string_lossy().to_string()),
        weight: 1.0,
        context: None,
    });
}

fn add_node_if_missing(fragment: &mut ExtractionFragment, node: Node) {
    if !fragment.nodes.iter().any(|n| n.id == node.id) {
        fragment.nodes.push(node);
    }
}

fn run_query_named(
    source: &[u8],
    language: &Language,
    query_str: &str,
) -> Result<HashMap<String, Vec<String>>> {
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .map_err(|e| CodeSynapseError::Parse(format!("failed to set language: {}", e)))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| CodeSynapseError::Parse("failed to parse source".to_string()))?;

    let query = Query::new(language, query_str)
        .map_err(|e| CodeSynapseError::Parse(format!("invalid query: {}", e)))?;

    let capture_names = query.capture_names().to_vec();
    let mut cursor = QueryCursor::new();
    let root = tree.root_node();

    let mut results: HashMap<String, Vec<String>> = HashMap::new();
    for name in &capture_names {
        results.insert(name.to_string(), Vec::new());
    }

    let mut matches = cursor.matches(&query, root, source);
    matches.advance();
    while let Some(m) = matches.get() {
        for capture in m.captures.iter() {
            let idx = capture.index as usize;
            if idx < capture_names.len() {
                let name = &capture_names[idx];
                if let Some(texts) = results.get_mut(&name[..]) {
                    if let Ok(text) = node_text(source, &capture.node) {
                        texts.push(text.to_string());
                    }
                }
            }
        }
        matches.advance();
    }
    Ok(results)
}

#[allow(dead_code)]
fn run_query_matches(
    source: &[u8],
    language: &Language,
    query_str: &str,
) -> Result<Vec<HashMap<String, String>>> {
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .map_err(|e| CodeSynapseError::Parse(format!("failed to set language: {}", e)))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| CodeSynapseError::Parse("failed to parse source".to_string()))?;

    let query = Query::new(language, query_str)
        .map_err(|e| CodeSynapseError::Parse(format!("invalid query: {}", e)))?;

    let capture_names = query.capture_names().to_vec();
    let mut cursor = QueryCursor::new();
    let root = tree.root_node();

    let mut all_matches: Vec<HashMap<String, String>> = Vec::new();

    let mut matches = cursor.matches(&query, root, source);
    matches.advance();
    while let Some(m) = matches.get() {
        let mut match_map: HashMap<String, String> = HashMap::new();
        for capture in m.captures.iter() {
            let idx = capture.index as usize;
            if idx < capture_names.len() {
                let name = capture_names[idx].to_string();
                if let Ok(text) = node_text(source, &capture.node) {
                    match_map.insert(name, text.to_string());
                }
            }
        }
        all_matches.push(match_map);
        matches.advance();
    }
    Ok(all_matches)
}

type RangedMatch = (HashMap<String, String>, HashMap<String, (usize, usize)>);

fn run_query_matches_ranged(
    source: &[u8],
    language: &Language,
    query_str: &str,
) -> Result<Vec<RangedMatch>> {
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .map_err(|e| CodeSynapseError::Parse(format!("failed to set language: {}", e)))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| CodeSynapseError::Parse("failed to parse source".to_string()))?;

    let query = Query::new(language, query_str)
        .map_err(|e| CodeSynapseError::Parse(format!("invalid query: {}", e)))?;

    let capture_names = query.capture_names().to_vec();
    let mut cursor = QueryCursor::new();
    let root = tree.root_node();

    let mut all_matches: Vec<RangedMatch> = Vec::new();

    let mut matches = cursor.matches(&query, root, source);
    matches.advance();
    while let Some(m) = matches.get() {
        let mut text_map: HashMap<String, String> = HashMap::new();
        let mut range_map: HashMap<String, (usize, usize)> = HashMap::new();
        for capture in m.captures.iter() {
            let idx = capture.index as usize;
            if idx < capture_names.len() {
                let name = capture_names[idx].to_string();
                range_map.insert(
                    name.clone(),
                    (capture.node.start_byte(), capture.node.end_byte()),
                );
                if let Ok(text) = node_text(source, &capture.node) {
                    text_map.insert(name, text.to_string());
                }
            }
        }
        all_matches.push((text_map, range_map));
        matches.advance();
    }
    Ok(all_matches)
}

/// Walk CST to find preceding `/** */` Javadoc/JSDoc comment for each named item.
/// Returns map of item_name → cleaned docstring text.
/// For items with duplicate names, last occurrence wins.
pub(super) fn run_tree_walk_docstrings(
    source: &[u8],
    language: &Language,
    item_kinds: &[&str],
) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();

    let mut parser = Parser::new();
    if parser.set_language(language).is_err() {
        return out;
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return out,
    };

    fn walk_node(
        node: TsNode<'_>,
        source: &[u8],
        item_kinds: &[&str],
        out: &mut HashMap<String, String>,
    ) {
        if item_kinds.contains(&node.kind()) {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) =
                    std::str::from_utf8(&source[name_node.start_byte()..name_node.end_byte()])
                {
                    let doc = node
                        .prev_named_sibling()
                        .filter(|s| s.kind() == "comment")
                        .and_then(|s| {
                            let text =
                                std::str::from_utf8(&source[s.start_byte()..s.end_byte()]).ok()?;
                            if text.starts_with("/**") {
                                strip_docstring(text)
                            } else {
                                None
                            }
                        });
                    if let Some(d) = doc {
                        out.insert(name.trim().to_string(), d);
                    }
                }
            }
        }
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                walk_node(cursor.node(), source, item_kinds, out);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    walk_node(tree.root_node(), source, item_kinds, &mut out);
    out
}

/// Strip delimiter characters from a raw docstring capture and dedent.
/// Returns None if the result is empty.
pub(super) fn strip_docstring(raw: &str) -> Option<String> {
    let s = raw.trim();

    let inner = if s.starts_with("\"\"\"") || s.starts_with("'''") {
        let delim = &s[..3];
        let body = s
            .strip_prefix(delim)
            .and_then(|t| t.strip_suffix(delim))
            .unwrap_or_else(|| s.strip_prefix(delim).unwrap_or(s));
        body.to_string()
    } else if s.starts_with("/**") {
        let body = s
            .strip_prefix("/**")
            .and_then(|t| t.strip_suffix("*/"))
            .unwrap_or_else(|| s.strip_prefix("/**").unwrap_or(s));
        body.lines()
            .map(|l| {
                let t = l.trim();
                if let Some(rest) = t.strip_prefix("* ") {
                    rest
                } else if let Some(rest) = t.strip_prefix('*') {
                    rest
                } else {
                    t
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else if s.starts_with('"') || s.starts_with('\'') {
        let delim = &s[..1];
        s.strip_prefix(delim)
            .and_then(|t| t.strip_suffix(delim))
            .unwrap_or(s)
            .to_string()
    } else if s.starts_with("///") {
        s.lines()
            .map(|l| {
                l.trim()
                    .strip_prefix("///")
                    .unwrap_or(l.trim())
                    .trim_start()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        s.to_string()
    };

    let lines: Vec<&str> = inner.lines().collect();
    let min_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    let dedented: String = lines
        .iter()
        .map(|l| {
            if l.len() >= min_indent {
                &l[min_indent..]
            } else {
                l.trim()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    if dedented.is_empty() {
        None
    } else {
        Some(dedented)
    }
}

pub mod astro;
pub mod bash;
pub mod c;
pub mod cmake;
pub mod cpp;
pub mod csharp;
pub mod csproj;
pub mod dart;
pub mod dm;
pub mod elixir;
pub mod fortran;
pub mod go;
pub mod groovy;
pub mod haskell;
pub mod java;
pub mod javascript;
pub mod json_package;
pub mod julia;
pub mod kotlin;
pub mod lua;
pub mod markdown;
pub mod mcp_config;
pub mod objc;
pub mod pascal;
pub mod php;
pub mod powershell;
pub mod python;
pub mod racket;
pub mod razor;
pub mod ruby;
pub mod rust;
pub mod scala;
pub mod sln;
pub mod sql;
pub mod svelte;
pub mod swift;
pub mod typescript;
pub mod verilog;
pub mod vue;
pub mod zig;

pub use astro::AstroExtractor;
pub use bash::TsBashExtractor;
pub use c::TsCExtractor;
pub use cmake::TsCmakeExtractor;
pub use cpp::TsCppExtractor;
pub use csharp::TsCSharpExtractor;
pub use csproj::CsprojExtractor;
pub use dart::TsDartExtractor;
pub use dm::{DmExtractor, DmfExtractor, DmiExtractor, DmmExtractor};
pub use elixir::TsElixirExtractor;
pub use fortran::FortranExtractor;
pub use go::TsGoExtractor;
pub use groovy::TsGroovyExtractor;
pub use haskell::TsHaskellExtractor;
pub use java::TsJavaExtractor;
pub use javascript::TsJavaScriptExtractor;
pub use json_package::JsonPackageExtractor;
pub use julia::TsJuliaExtractor;
pub use kotlin::TsKotlinExtractor;
pub use lua::TsLuaExtractor;
pub use markdown::MarkdownExtractor;
pub use mcp_config::McpConfigExtractor;
pub use objc::ObjCExtractor;
pub use pascal::TsPascalExtractor;
pub use php::TsPhpExtractor;
pub use powershell::PowerShellExtractor;
pub use python::TsPythonExtractor;
pub use racket::TsRacketExtractor;
pub use razor::RazorExtractor;
pub use ruby::TsRubyExtractor;
pub use rust::TsRustExtractor;
pub use scala::TsScalaExtractor;
pub use sln::SlnExtractor;
pub use sql::TsSqlExtractor;
pub use svelte::TsSvelteExtractor;
pub use swift::TsSwiftExtractor;
pub use typescript::TsTypeScriptExtractor;
pub use verilog::VerilogExtractor;
pub use vue::TsVueExtractor;
pub use zig::TsZigExtractor;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{make_id, normalize_id};

    // --- run_query_matches_ranged ---

    #[test]
    fn test_run_query_matches_ranged_byte_ranges() {
        let source = b"def foo(x):\n    pass\n";
        let lang = tree_sitter_python::LANGUAGE.into();
        let query = r#"(function_definition) @func.node"#;
        let matches = run_query_matches_ranged(source, &lang, query).unwrap();
        assert_eq!(matches.len(), 1);
        let (_, ranges) = &matches[0];
        let (start, end) = ranges["func.node"];
        assert_eq!(start, 0);
        assert!(end > start);
        assert!(source[start..end].starts_with(b"def"));
    }

    #[test]
    fn test_run_query_matches_ranged_name_and_node() {
        let source = b"def bar():\n    return 1\n";
        let lang = tree_sitter_python::LANGUAGE.into();
        let query = r#"(function_definition name: (identifier) @fn.name) @fn.node"#;
        let matches = run_query_matches_ranged(source, &lang, query).unwrap();
        assert_eq!(matches.len(), 1);
        let (texts, ranges) = &matches[0];
        assert_eq!(texts["fn.name"], "bar");
        let (node_start, node_end) = ranges["fn.node"];
        let (name_start, name_end) = ranges["fn.name"];
        assert!(node_start <= name_start);
        assert!(name_end <= node_end);
    }

    // --- Python source_location ---

    #[test]
    fn test_python_fn_source_location_set() {
        let source = b"def foo(x):\n    pass\n";
        let path = Path::new("test.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();
        let fn_node = result.nodes.iter().find(|n| n.label == "foo()").unwrap();
        assert!(fn_node.source_location.is_some());
        let loc = fn_node.source_location.as_ref().unwrap();
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert_eq!(start, 0);
        assert!(end > start);
        assert!(source[start..end].starts_with(b"def"));
    }

    #[test]
    fn test_python_file_node_source_location_none() {
        let source = b"def foo(x):\n    pass\n";
        let path = Path::new("mymod.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();
        let file_node = result.nodes.iter().find(|n| n.label == "mymod.py").unwrap();
        assert!(file_node.source_location.is_none());
    }

    #[test]
    fn test_python_class_source_location_set() {
        let source = b"class Foo:\n    pass\n";
        let path = Path::new("test.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();
        let class_node = result.nodes.iter().find(|n| n.label == "Foo").unwrap();
        assert!(class_node.source_location.is_some());
        let loc = class_node.source_location.as_ref().unwrap();
        let (start, end) = {
            let parts: Vec<&str> = loc.split(':').collect();
            let s: usize = parts[0].parse().unwrap();
            let e: usize = parts[1].parse().unwrap();
            (s, e)
        };
        assert_eq!(start, 0);
        assert!(end > start);
    }

    // --- Python tree-sitter extractor tests ---

    #[test]
    fn test_ts_extract_python_class() {
        let source = b"class Foo(Bar):\n    pass\n";
        let path = Path::new("test.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();

        let foo_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(foo_node.is_some(), "expected Foo node");
        assert!(result.nodes.len() >= 2, "expected at least 2 nodes");

        let contains_edge = result.edges.iter().find(|e| e.relation == "contains");
        assert!(contains_edge.is_some(), "expected contains edge");

        let inherits_edge = result.edges.iter().find(|e| e.relation == "inherits");
        assert!(inherits_edge.is_some(), "expected inherits edge");
    }

    #[test]
    fn test_ts_extract_python_function() {
        let source = b"def foo(x: int) -> str:\n    pass\n";
        let path = Path::new("test.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();

        let fn_node = result.nodes.iter().find(|n| n.label == "foo()");
        assert!(fn_node.is_some(), "expected foo() node");
    }

    #[test]
    fn test_ts_extract_python_import() {
        let source = b"import os\n";
        let path = Path::new("test.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();

        let import_edge = result.edges.iter().find(|e| e.relation == "imports");
        assert!(import_edge.is_some(), "expected imports edge");
    }

    #[test]
    fn test_ts_extract_python_from_import() {
        let source = b"from .helper import transform\n";
        let path = Path::new("test.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();

        let import_edge = result.edges.iter().find(|e| e.relation == "imports");
        assert!(import_edge.is_some(), "expected imports edge");
    }

    #[test]
    fn test_ts_extract_recursion_limit() {
        let source = b"# deeply nested\n";
        let path = Path::new("test.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();
        assert!(!result.nodes.is_empty(), "should at least have file node");
    }

    #[test]
    fn test_ts_extract_syntax_error() {
        let source = b"def foo( bar : \n";
        let path = Path::new("test.py");
        let result = TsPythonExtractor::extract(source, path);
        assert!(result.is_ok(), "should gracefully handle syntax errors");
    }

    #[test]
    fn test_python_class_file_type() {
        let source = b"class Authenticator:\n    pass\n";
        let path = Path::new("auth.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "Authenticator")
            .unwrap();
        assert_eq!(node.file_type, "class");
    }

    #[test]
    fn test_python_module_function_file_type() {
        let source = b"def authenticate(req):\n    pass\n";
        let path = Path::new("auth.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "authenticate()")
            .unwrap();
        assert_eq!(node.file_type, "function");
    }

    #[test]
    fn test_python_method_file_type() {
        let source = b"class Auth:\n    def login(self):\n        pass\n";
        let path = Path::new("auth.py");
        let result = TsPythonExtractor::extract(source, path).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "login()").unwrap();
        assert_eq!(node.file_type, "method");
    }

    #[test]
    fn test_ts_make_id() {
        assert_eq!(make_id(&["foo", "bar"]), "foo_bar");
        assert_eq!(make_id(&["Foo", "Bar"]), "foo_bar");
        assert_eq!(make_id(&["foo-bar"]), "foo_bar");
    }

    #[test]
    fn test_ts_normalize_id() {
        assert_eq!(normalize_id("Foo-Bar"), "foo_bar");
        assert_eq!(
            normalize_id("Session ValidateToken"),
            "session_validatetoken"
        );
    }

    // --- JavaScript extractor tests ---

    #[test]
    fn test_ts_extract_js_class() {
        let source = b"class Foo extends Bar {}";
        let path = Path::new("class_def.js");
        let result = TsJavaScriptExtractor::extract(source, path).unwrap();

        let foo_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(foo_node.is_some(), "expected Foo node");

        let inherits_edge = result.edges.iter().find(|e| e.relation == "inherits");
        assert!(inherits_edge.is_some(), "expected inherits edge");

        let bar_node = result.nodes.iter().find(|n| n.label == "Bar");
        assert!(bar_node.is_some(), "expected Bar node");
    }

    #[test]
    fn test_ts_extract_js_import() {
        let source = b"import { x } from './mod'";
        let path = Path::new("imports.js");
        let result = TsJavaScriptExtractor::extract(source, path).unwrap();

        let imports_from_edge = result.edges.iter().find(|e| e.relation == "imports_from");
        assert!(imports_from_edge.is_some(), "expected imports_from edge");

        let imports_edge = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .count();
        assert!(imports_edge > 0, "expected at least one imports edge");
    }

    #[test]
    fn test_ts_extract_js_dynamic_import() {
        let source = b"const mod = await import('./mod')";
        let path = Path::new("imports.js");
        let result = TsJavaScriptExtractor::extract(source, path).unwrap();

        let imports_from_edge = result.edges.iter().find(|e| e.relation == "imports_from");
        assert!(imports_from_edge.is_some(), "expected imports_from edge");
    }

    #[test]
    fn test_ts_extract_js_require() {
        let source = b"const m = require('./mod')";
        let path = Path::new("imports.js");
        let result = TsJavaScriptExtractor::extract(source, path).unwrap();

        let imports_from_edge = result.edges.iter().find(|e| e.relation == "imports_from");
        assert!(imports_from_edge.is_some(), "expected imports_from edge");
    }

    // --- TypeScript extractor tests ---

    #[test]
    fn test_ts_interface_file_type() {
        let source = b"interface AuthProvider {\n  login(): void\n}";
        let path = Path::new("auth.ts");
        let result = TsTypeScriptExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "AuthProvider")
            .unwrap();
        assert_eq!(node.file_type, "interface");
    }

    #[test]
    fn test_ts_type_alias_file_type() {
        let source = b"type UserId = User;";
        let path = Path::new("types.ts");
        let result = TsTypeScriptExtractor::extract(source, path).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "UserId").unwrap();
        assert_eq!(node.file_type, "class");
    }

    #[test]
    fn test_ts_function_declaration_file_type() {
        let source = b"function authenticate(req: Request): boolean { return true; }";
        let path = Path::new("auth.ts");
        let result = TsTypeScriptExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "authenticate()")
            .unwrap();
        assert_eq!(node.file_type, "function");
    }

    #[test]
    fn test_ts_method_definition_file_type() {
        let source = b"class AuthService {\n  login() {}\n}";
        let path = Path::new("auth.ts");
        let result = TsTypeScriptExtractor::extract(source, path).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "login()").unwrap();
        assert_eq!(node.file_type, "method");
    }

    #[test]
    fn test_ts_arrow_function_file_type() {
        let source = b"const validate = (token: string) => token.length > 0;";
        let path = Path::new("auth.ts");
        let result = TsTypeScriptExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "validate()")
            .unwrap();
        assert_eq!(node.file_type, "function");
    }

    #[test]
    fn test_ts_extract_ts_interface() {
        let source = b"interface Foo {\n  bar(): void\n}";
        let path = Path::new("interface.ts");
        let result = TsTypeScriptExtractor::extract(source, path).unwrap();

        let iface_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(iface_node.is_some(), "expected Foo interface node");
        if let Some(n) = iface_node {
            assert_eq!(
                n.metadata.get("kind").map(|s| s.as_str()),
                Some("interface")
            );
        }
    }

    #[test]
    fn test_ts_extract_ts_type_alias() {
        let source = b"type Foo = Bar<string>";
        let path = Path::new("interface.ts");
        let result = TsTypeScriptExtractor::extract(source, path).unwrap();

        let alias_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(alias_node.is_some(), "expected Foo type alias node");

        let type_ref_edge = result.edges.iter().find(|e| e.relation == "type_ref");
        assert!(type_ref_edge.is_some(), "expected type_ref edge");
    }

    // --- Go extractor tests ---

    #[test]
    fn test_go_struct_file_type() {
        let source = b"type Conn struct { addr string }";
        let path = Path::new("conn.go");
        let result = TsGoExtractor::extract(source, path).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "Conn").unwrap();
        assert_eq!(node.file_type, "struct");
    }

    #[test]
    fn test_go_function_file_type() {
        let source = b"func HandleRequest() {}";
        let path = Path::new("handler.go");
        let result = TsGoExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "HandleRequest()")
            .unwrap();
        assert_eq!(node.file_type, "function");
    }

    #[test]
    fn test_go_method_file_type() {
        let source = b"type Engine struct{}\nfunc (e *Engine) ServeHTTP() {}";
        let path = Path::new("engine.go");
        let result = TsGoExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "ServeHTTP()")
            .unwrap();
        assert_eq!(node.file_type, "method");
    }

    #[test]
    fn test_ts_extract_go_struct() {
        let source = b"type Foo struct {\n\tBar string\n}";
        let path = Path::new("struct.go");
        let result = TsGoExtractor::extract(source, path).unwrap();

        let struct_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(struct_node.is_some(), "expected Foo struct node");
    }

    #[test]
    fn test_go_struct_source_location() {
        let source = b"type Foo struct {\n\tBar string\n}";
        let path = Path::new("struct.go");
        let result = TsGoExtractor::extract(source, path).unwrap();
        let struct_node = result.nodes.iter().find(|n| n.label == "Foo").unwrap();
        assert!(
            struct_node.source_location.is_some(),
            "expected source_location on struct node"
        );
        let loc = struct_node.source_location.as_ref().unwrap();
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert!(end > start);
    }

    #[test]
    fn test_go_top_level_function_extracted() {
        let source = b"func HandleRequest(w http.ResponseWriter, r *http.Request) {}";
        let path = Path::new("handler.go");
        let result = TsGoExtractor::extract(source, path).unwrap();
        let fn_node = result.nodes.iter().find(|n| n.label == "HandleRequest()");
        assert!(fn_node.is_some(), "expected HandleRequest() node");
        assert!(
            fn_node.unwrap().source_location.is_some(),
            "expected source_location on function"
        );
    }

    #[test]
    fn test_go_pointer_receiver_method_extracted() {
        let source = b"package gin\ntype Engine struct{}\nfunc (e *Engine) ServeHTTP(w http.ResponseWriter, r *http.Request) {}";
        let path = Path::new("gin.go");
        let result = TsGoExtractor::extract(source, path).unwrap();
        let method_node = result.nodes.iter().find(|n| n.label == "ServeHTTP()");
        assert!(method_node.is_some(), "expected ServeHTTP() node");
        let nodes: HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let has_edge = result.edges.iter().any(|e| {
            e.relation == "contains"
                && nodes.get(&e.source).map(|s| s == "Engine").unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t == "ServeHTTP()")
                    .unwrap_or(false)
        });
        assert!(has_edge, "expected Engine -contains-> ServeHTTP() edge");
    }

    #[test]
    fn test_go_value_receiver_method_extracted() {
        let source =
            b"package gin\ntype Router struct{}\nfunc (r Router) Use(middleware ...HandlerFunc) {}";
        let path = Path::new("router.go");
        let result = TsGoExtractor::extract(source, path).unwrap();
        let method_node = result.nodes.iter().find(|n| n.label == "Use()");
        assert!(method_node.is_some(), "expected Use() node");
        let nodes: HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let has_edge = result.edges.iter().any(|e| {
            e.relation == "contains"
                && nodes.get(&e.source).map(|s| s == "Router").unwrap_or(false)
                && nodes.get(&e.target).map(|t| t == "Use()").unwrap_or(false)
        });
        assert!(has_edge, "expected Router -contains-> Use() edge");
    }

    // --- Rust extractor tests ---

    #[test]
    fn test_rust_struct_file_type() {
        let source = b"struct Foo { bar: u32 }";
        let path = Path::new("lib.rs");
        let result = TsRustExtractor::extract(source, path).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "Foo").unwrap();
        assert_eq!(node.file_type, "struct");
    }

    #[test]
    fn test_rust_function_file_type() {
        let source = b"fn process(x: u32) -> u32 { x }";
        let path = Path::new("lib.rs");
        let result = TsRustExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "process()")
            .unwrap();
        assert_eq!(node.file_type, "function");
    }

    #[test]
    fn test_ts_extract_rs_struct() {
        let source = b"struct Foo<T> {\n    bar: T,\n}";
        let path = Path::new("generic.rs");
        let result = TsRustExtractor::extract(source, path).unwrap();

        let struct_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(struct_node.is_some(), "expected Foo struct node");

        let generic_edge = result.edges.iter().find(|e| e.relation == "generic");
        assert!(generic_edge.is_some(), "expected generic edge");
    }

    #[test]
    fn test_ts_extract_rs_import() {
        let source = b"use crate::mod::Foo;";
        let path = Path::new("generic.rs");
        let result = TsRustExtractor::extract(source, path).unwrap();

        let imports = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .count();
        assert!(imports > 0, "expected at least one imports edge");
    }

    #[test]
    fn test_ts_extract_rs_function() {
        let source = b"fn foo(x: u32) -> u32 {\n    x + 1\n}\n";
        let path = Path::new("lib.rs");
        let result = TsRustExtractor::extract(source, path).unwrap();

        let fn_node = result.nodes.iter().find(|n| n.label == "foo()");
        assert!(fn_node.is_some(), "expected foo() function node");
    }

    #[test]
    fn test_ts_extract_rs_function_source_location() {
        let source = b"fn bar() {\n    let x = 1;\n}\n";
        let path = Path::new("lib.rs");
        let result = TsRustExtractor::extract(source, path).unwrap();

        let fn_node = result.nodes.iter().find(|n| n.label == "bar()").unwrap();
        assert!(
            fn_node.source_location.is_some(),
            "expected source_location on function node"
        );
        let loc = fn_node.source_location.as_ref().unwrap();
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert!(end > start);
        assert!(source[start..end].starts_with(b"fn"));
    }

    #[test]
    fn test_ts_extract_rs_impl_method_contains_edge() {
        let source = b"struct Foo;\nimpl Foo {\n    fn new() -> Self { Foo }\n}\n";
        let path = Path::new("lib.rs");
        let result = TsRustExtractor::extract(source, path).unwrap();

        let fn_node = result.nodes.iter().find(|n| n.label == "new()");
        assert!(fn_node.is_some(), "expected new() method node");

        let nodes: HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let impl_contains = result.edges.iter().any(|e| {
            e.relation == "contains"
                && nodes.get(&e.source).map(|s| s == "Foo").unwrap_or(false)
                && nodes.get(&e.target).map(|t| t == "new()").unwrap_or(false)
        });
        assert!(impl_contains, "expected Foo -contains-> new() edge");
    }

    // --- Java extractor tests (25-26) ---

    #[test]
    fn test_ts_extract_java_method() {
        let source = b"@Override\nvoid foo() {\n}\n";
        let path = Path::new("method.java");
        let result = TsJavaExtractor::extract(source, path).unwrap();

        let method_node = result.nodes.iter().find(|n| n.label == "foo()");
        assert!(method_node.is_some(), "expected foo() method node");

        let annot_node = result.nodes.iter().find(|n| n.label == "@Override");
        assert!(annot_node.is_some(), "expected @Override annotation node");
    }

    #[test]
    fn test_ts_extract_java_import() {
        let source = b"import java.util.List;\n";
        let path = Path::new("method.java");
        let result = TsJavaExtractor::extract(source, path).unwrap();

        let import_edge = result.edges.iter().find(|e| e.relation == "imports");
        assert!(import_edge.is_some(), "expected imports edge");
    }

    #[test]
    fn test_java_class_file_type() {
        let source = b"class UserService {\n}\n";
        let path = Path::new("UserService.java");
        let result = TsJavaExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "UserService")
            .unwrap();
        assert_eq!(node.file_type, "class");
    }

    #[test]
    fn test_java_method_file_type() {
        let source = b"class Svc {\n    void handle() {}\n}\n";
        let path = Path::new("Svc.java");
        let result = TsJavaExtractor::extract(source, path).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "handle()").unwrap();
        assert_eq!(node.file_type, "method");
    }

    #[test]
    fn test_java_interface_file_type() {
        let source = b"interface AuthProvider {\n    boolean login();\n}\n";
        let path = Path::new("AuthProvider.java");
        let result = TsJavaExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "AuthProvider")
            .unwrap();
        assert_eq!(node.file_type, "interface");
    }

    // --- C extractor tests (27-28) ---

    #[test]
    fn test_ts_extract_c_function() {
        let source = b"int foo(int x) {\n    return bar(x);\n}\n";
        let path = Path::new("func.c");
        let result = TsCExtractor::extract(source, path).unwrap();

        let fn_node = result.nodes.iter().find(|n| n.label == "foo()");
        assert!(fn_node.is_some(), "expected foo() function node");
    }

    #[test]
    fn test_ts_extract_c_include() {
        let source = b"#include \"helper.h\"\n";
        let path = Path::new("func.c");
        let result = TsCExtractor::extract(source, path).unwrap();

        let include_edge = result.edges.iter().find(|e| e.relation == "imports");
        assert!(include_edge.is_some(), "expected imports edge for include");
    }

    // --- C++ extractor test (29) ---

    #[test]
    fn test_ts_extract_cpp_method() {
        let source = b"class Foo {\n    void bar();\n};\n";
        let path = Path::new("class.cpp");
        let result = TsCppExtractor::extract(source, path).unwrap();

        let class_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(class_node.is_some(), "expected Foo class node");

        let method_node = result.nodes.iter().find(|n| n.label == "bar()");
        assert!(method_node.is_some(), "expected bar() method node");
    }

    // --- C# extractor tests (30-31) ---

    #[test]
    fn test_ts_extract_csharp_class() {
        let source = b"class Foo : IBar, Baz { }\n";
        let path = Path::new("class.cs");
        let result = TsCSharpExtractor::extract(source, path).unwrap();

        let class_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(class_node.is_some(), "expected Foo class node");

        let inherits_edge = result.edges.iter().find(|e| e.relation == "inherits");
        assert!(inherits_edge.is_some(), "expected inherits edge");
    }

    #[test]
    fn test_ts_extract_csharp_namespace() {
        let source = b"namespace X {\n    class Y { }\n}\n";
        let path = Path::new("class.cs");
        let result = TsCSharpExtractor::extract(source, path).unwrap();

        let ns_node = result.nodes.iter().find(|n| n.label == "X namespace");
        assert!(ns_node.is_some(), "expected X namespace node");

        let contains_ns = result.edges.iter().any(|e| e.relation == "contains");
        assert!(contains_ns, "expected contains edges");
    }

    #[test]
    fn test_csharp_class_file_type() {
        let source = b"class UserService { }\n";
        let path = Path::new("UserService.cs");
        let result = TsCSharpExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "UserService")
            .unwrap();
        assert_eq!(node.file_type, "class");
    }

    #[test]
    fn test_csharp_method_file_type() {
        let source = b"class Svc {\n    void Handle() {}\n}\n";
        let path = Path::new("Svc.cs");
        let result = TsCSharpExtractor::extract(source, path).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "Handle()").unwrap();
        assert_eq!(node.file_type, "method");
    }

    #[test]
    fn test_csharp_interface_file_type() {
        let source = b"interface IAuthProvider { }\n";
        let path = Path::new("IAuthProvider.cs");
        let result = TsCSharpExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "IAuthProvider")
            .unwrap();
        assert_eq!(node.file_type, "interface");
    }

    #[test]
    fn test_csharp_namespace_file_type() {
        let source = b"namespace MyApp { }\n";
        let path = Path::new("MyApp.cs");
        let result = TsCSharpExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "MyApp namespace")
            .unwrap();
        assert_eq!(node.file_type, "namespace");
    }

    // --- Kotlin extractor test (32) ---

    #[test]
    fn test_ts_extract_kotlin_class() {
        let source = b"class Foo : Bar()\n";
        let path = Path::new("class.kt");
        let result = TsKotlinExtractor::extract(source, path).unwrap();

        let class_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(class_node.is_some(), "expected Foo class node");

        let inherits_edge = result.edges.iter().find(|e| e.relation == "inherits");
        assert!(inherits_edge.is_some(), "expected inherits edge");
    }

    // --- Swift extractor test (33) ---

    #[test]
    fn test_ts_extract_swift_struct() {
        let source = b"struct Foo: Bar, Baz {}\n";
        let path = Path::new("struct.swift");
        let result = TsSwiftExtractor::extract(source, path).unwrap();

        let struct_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(struct_node.is_some(), "expected Foo struct node");

        let inherits_edges = result
            .edges
            .iter()
            .filter(|e| e.relation == "inherits")
            .count();
        assert_eq!(
            inherits_edges, 2,
            "expected 2 inherits edges for Bar and Baz"
        );
    }

    // --- PHP extractor test (34) ---

    #[test]
    fn test_ts_extract_php_class() {
        let source = b"class Foo extends Bar {}\n";
        let path = Path::new("class.php");
        let result = TsPhpExtractor::extract(source, path).unwrap();

        let class_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(class_node.is_some(), "expected Foo class node");

        let inherits_edge = result.edges.iter().find(|e| e.relation == "inherits");
        assert!(inherits_edge.is_some(), "expected inherits edge");
    }

    // --- Ruby extractor tests (35+) ---

    #[test]
    fn test_ts_extract_ruby_class() {
        let source = b"class Foo < Bar\nend\n";
        let path = Path::new("class.rb");
        let result = TsRubyExtractor::extract(source, path).unwrap();

        let class_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(class_node.is_some(), "expected Foo class node");

        let inherits_edge = result.edges.iter().find(|e| e.relation == "inherits");
        assert!(inherits_edge.is_some(), "expected inherits edge");
    }

    #[test]
    fn test_ruby_class_source_location() {
        let source = b"class Foo < Bar\nend\n";
        let path = Path::new("class.rb");
        let result = TsRubyExtractor::extract(source, path).unwrap();
        let class_node = result.nodes.iter().find(|n| n.label == "Foo").unwrap();
        assert!(
            class_node.source_location.is_some(),
            "expected source_location on class node"
        );
        let loc = class_node.source_location.as_ref().unwrap();
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert!(end > start);
    }

    #[test]
    fn test_ruby_instance_method_extracted() {
        let source = b"class UsersController\n  def create\n  end\nend\n";
        let path = Path::new("users_controller.rb");
        let result = TsRubyExtractor::extract(source, path).unwrap();
        let method_node = result.nodes.iter().find(|n| n.label == "create()");
        assert!(method_node.is_some(), "expected create() node");
        assert!(
            method_node.unwrap().source_location.is_some(),
            "expected source_location on method"
        );
    }

    #[test]
    fn test_ruby_class_method_extracted() {
        let source = b"class User\n  def self.authenticate(token)\n  end\nend\n";
        let path = Path::new("user.rb");
        let result = TsRubyExtractor::extract(source, path).unwrap();
        let method_node = result
            .nodes
            .iter()
            .find(|n| n.label == "self.authenticate()");
        assert!(method_node.is_some(), "expected self.authenticate() node");
    }

    #[test]
    fn test_ruby_class_contains_method_edge() {
        let source = b"class ArticlesController\n  def index\n  end\nend\n";
        let path = Path::new("articles_controller.rb");
        let result = TsRubyExtractor::extract(source, path).unwrap();
        let nodes: HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let has_edge = result.edges.iter().any(|e| {
            e.relation == "contains"
                && nodes
                    .get(&e.source)
                    .map(|s| s == "ArticlesController")
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t == "index()")
                    .unwrap_or(false)
        });
        assert!(
            has_edge,
            "expected ArticlesController -contains-> index() edge"
        );
    }

    #[test]
    fn test_ruby_class_contains_singleton_method_edge() {
        let source = b"class User\n  def self.find_by_token(t)\n  end\nend\n";
        let path = Path::new("user.rb");
        let result = TsRubyExtractor::extract(source, path).unwrap();
        let nodes: HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let has_edge = result.edges.iter().any(|e| {
            e.relation == "contains"
                && nodes.get(&e.source).map(|s| s == "User").unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t == "self.find_by_token()")
                    .unwrap_or(false)
        });
        assert!(
            has_edge,
            "expected User -contains-> self.find_by_token() edge"
        );
    }

    #[test]
    fn test_ruby_class_file_type() {
        let source = b"class AuthController\nend\n";
        let path = Path::new("auth_controller.rb");
        let result = TsRubyExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "AuthController")
            .unwrap();
        assert_eq!(node.file_type, "class");
    }

    #[test]
    fn test_ruby_instance_method_file_type() {
        let source = b"class User\n  def authenticate\n  end\nend\n";
        let path = Path::new("user.rb");
        let result = TsRubyExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "authenticate()")
            .unwrap();
        assert_eq!(node.file_type, "method");
    }

    #[test]
    fn test_ruby_singleton_method_file_type() {
        let source = b"class User\n  def self.find_by_email(email)\n  end\nend\n";
        let path = Path::new("user.rb");
        let result = TsRubyExtractor::extract(source, path).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "self.find_by_email()")
            .unwrap();
        assert_eq!(node.file_type, "method");
    }

    // --- SQL extractor tests (36-37) ---

    #[test]
    fn test_ts_extract_sql_table() {
        let source = b"CREATE TABLE foo (id INT)\n";
        let path = Path::new("schema.sql");
        let result = TsSqlExtractor::extract(source, path).unwrap();

        let table_node = result.nodes.iter().find(|n| n.label == "foo");
        assert!(table_node.is_some(), "expected foo table node");
    }

    #[test]
    fn test_ts_extract_sql_foreign_key() {
        let source = b"ALTER TABLE foo ADD FOREIGN KEY (a) REFERENCES bar(b)\n";
        let path = Path::new("schema.sql");
        let result = TsSqlExtractor::extract(source, path).unwrap();

        let ref_edge = result.edges.iter().find(|e| e.relation == "references");
        assert!(ref_edge.is_some(), "expected references edge");
    }

    // --- Vue SFC test (38) ---

    #[test]
    fn test_ts_extract_vue_sfc() {
        let source = b"<script>\nexport default {\n  name: 'MyComponent'\n}\n</script>\n<template>\n  <div>{{ message }}</div>\n</template>\n";
        let path = Path::new("component.vue");
        let result = TsVueExtractor::extract(source, path).unwrap();

        let comp_node = result.nodes.iter().find(|n| n.label == "MyComponent");
        assert!(comp_node.is_some(), "expected MyComponent node");
    }

    // --- Svelte SFC test (39) ---

    #[test]
    fn test_ts_extract_svelte_sfc() {
        let source = b"<script>\n  let count = $state(0)\n</script>\n<h1>{count}</h1>\n";
        let path = Path::new("component.svelte");
        let result = TsSvelteExtractor::extract(source, path).unwrap();

        let state_node = result.nodes.iter().find(|n| n.label == "count");
        assert!(state_node.is_some(), "expected count state node");
    }

    // --- Bash extractor tests (40-41) ---

    #[test]
    fn test_ts_extract_bash_function() {
        let source = b"function foo() {\n    bar\n}\n";
        let path = Path::new("func.sh");
        let result = TsBashExtractor::extract(source, path).unwrap();

        let fn_node = result.nodes.iter().find(|n| n.label == "foo()");
        assert!(fn_node.is_some(), "expected foo() function node");
    }

    #[test]
    fn test_ts_extract_bash_source() {
        let source = b"source ./lib.sh\n";
        let path = Path::new("func.sh");
        let result = TsBashExtractor::extract(source, path).unwrap();

        let imports_from_edge = result.edges.iter().find(|e| e.relation == "imports_from");
        assert!(imports_from_edge.is_some(), "expected imports_from edge");
    }

    // --- Package.json test (42) ---

    #[test]
    fn test_ts_extract_json_package() {
        let source = b"{\n  \"name\": \"test-package\",\n  \"dependencies\": {\n    \"express\": \"^4.18.0\",\n    \"lodash\": \"^4.17.21\"\n  }\n}\n";
        let path = Path::new("package.json");
        let result = JsonPackageExtractor::extract(source, path).unwrap();

        let dep_nodes = result
            .nodes
            .iter()
            .filter(|n| n.label == "express" || n.label == "lodash")
            .count();
        assert_eq!(dep_nodes, 2, "expected express and lodash dep nodes");
    }

    // --- MCP config test (43) ---

    #[test]
    fn test_ts_extract_mcp_config() {
        let source = b"{\n  \"servers\": [\n    {\n      \"name\": \"test-server\",\n      \"command\": \"python\",\n      \"env\": {\n        \"API_KEY\": \"env:API_KEY\"\n      }\n    }\n  ]\n}\n";
        let path = Path::new("mcp.json");
        let result = McpConfigExtractor::extract(source, path).unwrap();

        let server_node = result.nodes.iter().find(|n| n.label == "test-server");
        assert!(server_node.is_some(), "expected test-server server node");
    }

    // --- CMake extractor test (44) ---

    #[test]
    fn test_ts_extract_cmake_function() {
        let source = b"function(my_func)\n  message(hello)\nendfunction()\n";
        let path = Path::new("CMakeLists.txt");
        let result = TsCmakeExtractor::extract(source, path).unwrap();
        let fn_node = result.nodes.iter().find(|n| n.label == "my_func()");
        assert!(fn_node.is_some(), "expected my_func() function node");
    }

    // --- Dart extractor test (45) ---

    #[test]
    fn test_ts_extract_dart_class() {
        let source = b"class MyClass { }\n";
        let path = Path::new("main.dart");
        let result = TsDartExtractor::extract(source, path).unwrap();
        let class_node = result.nodes.iter().find(|n| n.label == "MyClass");
        assert!(class_node.is_some(), "expected MyClass class node");
    }

    #[test]
    fn test_ts_extract_dart_method() {
        let source = b"class MyClass {\n  void myMethod() {}\n}\n";
        let path = Path::new("main.dart");
        let result = TsDartExtractor::extract(source, path).unwrap();
        let method_node = result.nodes.iter().find(|n| n.label == "myMethod()");
        assert!(method_node.is_some(), "expected myMethod() method node");
    }

    // --- Groovy extractor test (46) ---

    #[test]
    fn test_ts_extract_groovy_class() {
        let source = b"class MyGroovyClass {\n  def myMethod() { }\n}\n";
        let path = Path::new("MyGroovyClass.groovy");
        let result = TsGroovyExtractor::extract(source, path).unwrap();
        let class_node = result.nodes.iter().find(|n| n.label == "MyGroovyClass");
        assert!(class_node.is_some(), "expected MyGroovyClass class node");
    }

    #[test]
    fn test_ts_extract_groovy_method() {
        let source = b"class MyGroovyClass {\n  def myMethod() { }\n}\n";
        let path = Path::new("MyGroovyClass.groovy");
        let result = TsGroovyExtractor::extract(source, path).unwrap();
        let method_node = result.nodes.iter().find(|n| n.label == "myMethod()");
        assert!(method_node.is_some(), "expected myMethod() method node");
    }

    // --- Haskell extractor test (47) ---

    #[test]
    fn test_ts_extract_haskell_module() {
        let source = b"module MyModule where\n\nmyFunc = 42\n";
        let path = Path::new("MyModule.hs");
        let result = TsHaskellExtractor::extract(source, path).unwrap();
        let module_node = result.nodes.iter().find(|n| n.label == "MyModule");
        assert!(module_node.is_some(), "expected MyModule module node");
    }

    #[test]
    fn test_ts_extract_haskell_function() {
        let source = b"myFunc = 42\n";
        let path = Path::new("MyModule.hs");
        let result = TsHaskellExtractor::extract(source, path).unwrap();
        let func_node = result.nodes.iter().find(|n| n.label == "myFunc()");
        assert!(func_node.is_some(), "expected myFunc() function node");
    }

    // --- Lua extractor test (48) ---

    #[test]
    fn test_ts_extract_lua_function() {
        let source = b"function my_func()\nend\n";
        let path = Path::new("test.lua");
        let result = TsLuaExtractor::extract(source, path).unwrap();
        let fn_node = result.nodes.iter().find(|n| n.label == "my_func()");
        assert!(fn_node.is_some(), "expected my_func() function node");
    }

    #[test]
    fn test_ts_extract_lua_require() {
        let source = b"require(\"my_module\")\n";
        let path = Path::new("test.lua");
        let result = TsLuaExtractor::extract(source, path).unwrap();
        let import_edge = result.edges.iter().find(|e| e.relation == "imports");
        assert!(import_edge.is_some(), "expected imports edge for require");
    }

    // --- Racket extractor test (49) ---

    #[test]
    fn test_ts_extract_racket_define() {
        let source = b"(define my-func 42)\n";
        let path = Path::new("test.rkt");
        let result = TsRacketExtractor::extract(source, path).unwrap();
        let fn_node = result.nodes.iter().find(|n| n.label == "my-func()");
        assert!(fn_node.is_some(), "expected my-func() function node");
    }

    #[test]
    fn test_ts_extract_racket_require() {
        let source = b"(require racket/base)\n";
        let path = Path::new("test.rkt");
        let result = TsRacketExtractor::extract(source, path).unwrap();
        let import_edge = result.edges.iter().find(|e| e.relation == "imports");
        assert!(import_edge.is_some(), "expected imports edge for require");
    }

    // --- Scala extractor test (50) ---

    #[test]
    fn test_ts_extract_scala_class() {
        let source = b"class MyScalaClass {\n  def myMethod(): Unit = {}\n}\n";
        let path = Path::new("MyScalaClass.scala");
        let result = TsScalaExtractor::extract(source, path).unwrap();
        let class_node = result.nodes.iter().find(|n| n.label == "MyScalaClass");
        assert!(class_node.is_some(), "expected MyScalaClass class node");
    }

    #[test]
    fn test_ts_extract_scala_method() {
        let source = b"class MyScalaClass {\n  def myMethod(): Unit = {}\n}\n";
        let path = Path::new("MyScalaClass.scala");
        let result = TsScalaExtractor::extract(source, path).unwrap();
        let method_node = result.nodes.iter().find(|n| n.label == "myMethod()");
        assert!(method_node.is_some(), "expected myMethod() method node");
    }

    // --- Zig extractor test (51) ---

    #[test]
    fn test_ts_extract_zig_struct() {
        let source = b"const MyStruct = struct {};\nfn myFunc() void {}\n";
        let path = Path::new("main.zig");
        let result = TsZigExtractor::extract(source, path).unwrap();
        let struct_node = result.nodes.iter().find(|n| n.label == "MyStruct");
        assert!(struct_node.is_some(), "expected MyStruct struct node");
    }

    #[test]
    fn test_ts_extract_zig_function() {
        let source = b"const MyStruct = struct {};\nfn myFunc() void {}\n";
        let path = Path::new("main.zig");
        let result = TsZigExtractor::extract(source, path).unwrap();
        let func_node = result.nodes.iter().find(|n| n.label == "myFunc()");
        assert!(func_node.is_some(), "expected myFunc() function node");
    }

    // --- Julia extractor tests (52-53) ---

    #[test]
    fn test_ts_extract_julia_function() {
        let source = b"module MyMod\nfunction compute(x)\n  x * 2\nend\nend\n";
        let path = Path::new("MyMod.jl");
        let result = TsJuliaExtractor::extract(source, path).unwrap();
        let func_node = result.nodes.iter().find(|n| n.label == "compute()");
        assert!(func_node.is_some(), "expected compute() function node");
    }

    #[test]
    fn test_ts_extract_julia_struct() {
        let source = b"struct Point\n  x::Float64\n  y::Float64\nend\n";
        let path = Path::new("point.jl");
        let result = TsJuliaExtractor::extract(source, path).unwrap();
        let struct_node = result.nodes.iter().find(|n| n.label == "Point");
        assert!(struct_node.is_some(), "expected Point struct node");
    }

    // --- Elixir extractor tests (54-55) ---

    #[test]
    fn test_ts_extract_elixir_module() {
        let source = b"defmodule MyApp.Repo do\n  use Ecto.Repo\nend\n";
        let path = Path::new("repo.ex");
        let result = TsElixirExtractor::extract(source, path).unwrap();
        let mod_node = result.nodes.iter().find(|n| n.label == "MyApp.Repo");
        assert!(mod_node.is_some(), "expected MyApp.Repo module node");
    }

    #[test]
    fn test_ts_extract_elixir_function() {
        let source = b"defmodule Calc do\n  def add(a, b) do\n    a + b\n  end\nend\n";
        let path = Path::new("calc.ex");
        let result = TsElixirExtractor::extract(source, path).unwrap();
        let func_node = result.nodes.iter().find(|n| n.label == "add()");
        assert!(func_node.is_some(), "expected add() function node");
    }

    // --- Pascal extractor tests (56-57) ---

    #[test]
    fn test_ts_extract_pascal_procedure() {
        let source = b"unit MyUnit;\ninterface\nprocedure DoWork;\nimplementation\nprocedure DoWork;\nbegin\nend;\nend.\n";
        let path = Path::new("myunit.pas");
        let result = TsPascalExtractor::extract(source, path).unwrap();
        let proc_node = result.nodes.iter().find(|n| n.label == "DoWork");
        assert!(proc_node.is_some(), "expected DoWork procedure node");
    }

    #[test]
    fn test_ts_extract_pascal_uses() {
        let source = b"unit MyUnit;\ninterface\nuses SysUtils, Classes;\nimplementation\nend.\n";
        let path = Path::new("myunit.pas");
        let result = TsPascalExtractor::extract(source, path).unwrap();
        let sysutils_node = result.nodes.iter().find(|n| n.label == "SysUtils");
        assert!(sysutils_node.is_some(), "expected SysUtils import node");
        let import_edge = result.edges.iter().find(|e| e.relation == "imports");
        assert!(import_edge.is_some(), "expected imports edge");
    }

    // --- Markdown extractor tests (58-61) ---

    #[test]
    fn test_markdown_headings() {
        let source = b"# Title\n## Section One\n### Sub\n## Section Two\n";
        let path = Path::new("readme.md");
        let result = MarkdownExtractor::extract(source, path).unwrap();
        assert!(
            result.nodes.iter().any(|n| n.label == "Title"),
            "h1 missing"
        );
        assert!(
            result.nodes.iter().any(|n| n.label == "Section One"),
            "h2 missing"
        );
        assert!(result.nodes.iter().any(|n| n.label == "Sub"), "h3 missing");
        let contains = result
            .edges
            .iter()
            .filter(|e| e.relation == "contains")
            .count();
        assert!(
            contains >= 3,
            "expected >=3 contains edges, got {}",
            contains
        );
    }

    #[test]
    fn test_markdown_code_blocks() {
        let source = b"# Guide\n\n```rust\nfn main() {}\n```\n\n```python\nprint('hi')\n```\n";
        let path = Path::new("guide.md");
        let result = MarkdownExtractor::extract(source, path).unwrap();
        let code_nodes: Vec<_> = result
            .nodes
            .iter()
            .filter(|n| n.label.starts_with("code:"))
            .collect();
        assert_eq!(code_nodes.len(), 2, "expected 2 code block nodes");
    }

    #[test]
    fn test_markdown_heading_nesting() {
        let source = b"# Top\n## Child\n# Top\n";
        let path = Path::new("nested.md");
        let result = MarkdownExtractor::extract(source, path).unwrap();
        // Both "Top" headings kept (different IDs, same label)
        let top_nodes: Vec<_> = result.nodes.iter().filter(|n| n.label == "Top").collect();
        assert_eq!(
            top_nodes.len(),
            2,
            "expected 2 Top heading nodes (deduped IDs)"
        );
        assert!(
            result.nodes.iter().any(|n| n.label == "Child"),
            "Child missing"
        );
    }

    #[test]
    fn test_markdown_empty() {
        let source = b"just plain text, no headings or code blocks\n";
        let path = Path::new("plain.md");
        let result = MarkdownExtractor::extract(source, path).unwrap();
        // Only file node
        assert_eq!(result.nodes.len(), 1);
        assert!(result.edges.is_empty());
    }

    // --- Astro extractor tests (62-63) ---

    #[test]
    fn test_astro_frontmatter_imports() {
        let source = b"---\nimport Button from './Button.astro';\nimport { foo } from '../lib/utils';\n---\n<h1>Hello</h1>\n";
        let path = Path::new("page.astro");
        let result = AstroExtractor::extract(source, path).unwrap();
        let imp_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports_from")
            .collect();
        assert!(
            imp_edges.len() >= 2,
            "expected >=2 import edges, got {}",
            imp_edges.len()
        );
    }

    #[test]
    fn test_astro_script_block() {
        let source = b"<html>\n<script>\nimport { x } from './x.ts';\n</script>\n</html>\n";
        let path = Path::new("layout.astro");
        let result = AstroExtractor::extract(source, path).unwrap();
        let imp_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports_from")
            .collect();
        assert!(
            !imp_edges.is_empty(),
            "expected import edge from script block"
        );
    }

    // --- PowerShell extractor tests (64-65) ---

    #[test]
    fn test_powershell_function() {
        let source = b"function Get-Items {\n    param($path)\n    Get-ChildItem $path\n}\n";
        let path = Path::new("script.ps1");
        let result = PowerShellExtractor::extract(source, path).unwrap();
        let func_node = result.nodes.iter().find(|n| n.label.contains("Get-Items"));
        assert!(func_node.is_some(), "expected Get-Items function node");
        let contains = result.edges.iter().find(|e| e.relation == "contains");
        assert!(contains.is_some(), "expected contains edge");
    }

    #[test]
    fn test_powershell_class() {
        let source =
            b"class Animal {\n    [string]$Name\n    [void] Speak() { Write-Host $this.Name }\n}\n";
        let path = Path::new("classes.ps1");
        let result = PowerShellExtractor::extract(source, path).unwrap();
        assert!(!result.nodes.is_empty(), "expected at least file node");
        // class node or file node present
        let has_class = result
            .nodes
            .iter()
            .any(|n| n.label == "Animal" || n.file_type == "class");
        let has_file = result.nodes.iter().any(|n| n.file_type == "code");
        assert!(has_class || has_file, "expected class or file node");
    }

    // --- Verilog extractor tests (66-67) ---

    #[test]
    fn test_verilog_module() {
        let source =
            b"module adder(input a, input b, output sum);\n  assign sum = a + b;\nendmodule\n";
        let path = Path::new("adder.v");
        let result = VerilogExtractor::extract(source, path).unwrap();
        assert!(!result.nodes.is_empty(), "expected at least file node");
    }

    #[test]
    fn test_verilog_empty() {
        let source = b"// empty verilog\n";
        let path = Path::new("empty.v");
        let result = VerilogExtractor::extract(source, path).unwrap();
        assert_eq!(result.nodes.len(), 1, "only file node expected");
    }

    // --- Fortran extractor tests (68-69) ---

    #[test]
    fn test_fortran_subroutine() {
        let source =
            b"subroutine hello()\n  implicit none\n  print *, 'Hello'\nend subroutine hello\n";
        let path = Path::new("hello.f90");
        let result = FortranExtractor::extract(source, path).unwrap();
        assert!(!result.nodes.is_empty(), "expected at least file node");
    }

    #[test]
    fn test_fortran_module() {
        let source = b"module math_utils\n  implicit none\ncontains\n  function square(x) result(r)\n    real, intent(in) :: x\n    real :: r\n    r = x*x\n  end function square\nend module math_utils\n";
        let path = Path::new("math.f90");
        let result = FortranExtractor::extract(source, path).unwrap();
        assert!(!result.nodes.is_empty(), "expected at least file node");
    }

    // --- ObjC extractor tests (70-71) ---

    #[test]
    fn test_objc_interface() {
        let source =
            b"#import <Foundation/Foundation.h>\n@interface Dog : NSObject\n- (void)bark;\n@end\n";
        let path = Path::new("Dog.m");
        let result = ObjCExtractor::extract(source, path).unwrap();
        assert!(!result.nodes.is_empty(), "expected at least file node");
    }

    #[test]
    fn test_objc_empty() {
        let source = b"// no classes\n";
        let path = Path::new("empty.m");
        let result = ObjCExtractor::extract(source, path).unwrap();
        assert_eq!(result.nodes.len(), 1, "only file node expected");
    }

    // ── Phase 6: sample-fixture tests ───────────────────────────────────────────

    fn sample_path(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/sample")
            .join(name)
    }

    fn load_sample(name: &str) -> Vec<u8> {
        std::fs::read(sample_path(name)).unwrap_or_else(|_| panic!("fixture not found: {}", name))
    }

    // ── Java (72–74) ─────────────────────────────────────────────────────────────

    #[test]
    fn test_java_import_edges_have_import_context() {
        let src = load_sample("sample.java");
        let result = TsJavaExtractor::extract(&src, &sample_path("sample.java")).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    #[test]
    fn test_java_normalizes_inherits_and_implements() {
        let src = load_sample("sample.java");
        let path = sample_path("sample.java");
        let result = TsJavaExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let inherits: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "inherits")
            .map(|e| {
                (
                    nodes.get(&e.source).cloned().unwrap_or_default(),
                    nodes.get(&e.target).cloned().unwrap_or_default(),
                )
            })
            .collect();
        let implements: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "implements")
            .map(|e| {
                (
                    nodes.get(&e.source).cloned().unwrap_or_default(),
                    nodes.get(&e.target).cloned().unwrap_or_default(),
                )
            })
            .collect();
        assert!(
            inherits
                .iter()
                .any(|(s, t)| s.contains("DataProcessor") && t.contains("BaseProcessor")),
            "expected inherits(DataProcessor, BaseProcessor), got {:?}",
            inherits
        );
        assert!(
            implements
                .iter()
                .any(|(s, t)| s.contains("DataProcessor") && t.contains("Processor")),
            "expected implements(DataProcessor, Processor), got {:?}",
            implements
        );
    }

    #[test]
    fn test_java_parameter_return_generic_and_attribute_contexts() {
        let src = load_sample("sample.java");
        let path = sample_path("sample.java");
        let result = TsJavaExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let refs: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "references")
            .collect();
        let has_param = refs.iter().any(|e| {
            nodes
                .get(&e.source)
                .map(|s| s.contains("build"))
                .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t == "HttpClient")
                    .unwrap_or(false)
                && e.context.as_deref() == Some("parameter_type")
        });
        assert!(
            has_param,
            "expected build→HttpClient with parameter_type context"
        );
        let has_ret = refs.iter().any(|e| {
            nodes
                .get(&e.source)
                .map(|s| s.contains("build"))
                .unwrap_or(false)
                && nodes.get(&e.target).map(|t| t == "Result").unwrap_or(false)
                && e.context.as_deref() == Some("return_type")
        });
        assert!(has_ret, "expected build→Result with return_type context");
        let has_generic = refs.iter().any(|e| {
            nodes
                .get(&e.source)
                .map(|s| s.contains("build"))
                .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t == "DataProcessor")
                    .unwrap_or(false)
                && e.context.as_deref() == Some("generic_arg")
        });
        assert!(
            has_generic,
            "expected build→DataProcessor with generic_arg context"
        );
        let has_attrib = refs.iter().any(|e| {
            nodes
                .get(&e.source)
                .map(|s| s.contains("build"))
                .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t == "Override")
                    .unwrap_or(false)
                && e.context.as_deref() == Some("attribute")
        });
        assert!(has_attrib, "expected build→Override with attribute context");
    }

    // ── C (75–76) ────────────────────────────────────────────────────────────────

    #[test]
    fn test_c_import_edges_have_import_context() {
        let src = load_sample("sample.c");
        let path = sample_path("sample.c");
        let result = TsCExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    #[test]
    fn test_c_call_edges_have_call_context() {
        let src = load_sample("sample.c");
        let path = sample_path("sample.c");
        let result = TsCExtractor::extract(&src, &path).unwrap();
        let call_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "calls")
            .collect();
        assert!(!call_edges.is_empty(), "expected call edges");
        assert!(
            call_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("call")),
            "all call edges must have context='call'"
        );
    }

    // ── C++ (77–79) ──────────────────────────────────────────────────────────────

    #[test]
    fn test_cpp_import_edges_have_import_context() {
        let src = load_sample("sample.cpp");
        let path = sample_path("sample.cpp");
        let result = TsCppExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    #[test]
    fn test_cpp_class_inherits_edge() {
        let src = load_sample("sample.cpp");
        let path = sample_path("sample.cpp");
        let result = TsCppExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let found = result.edges.iter().any(|e| {
            e.relation == "inherits"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("AuthedHttpClient"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("HttpClient"))
                    .unwrap_or(false)
        });
        assert!(
            found,
            "AuthedHttpClient should have inherits edge to HttpClient"
        );
    }

    #[test]
    fn test_cpp_struct_inherits_edge() {
        let src = load_sample("sample.cpp");
        let path = sample_path("sample.cpp");
        let result = TsCppExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let found = result.edges.iter().any(|e| {
            e.relation == "inherits"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("RetryingHttpClient"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("HttpClient"))
                    .unwrap_or(false)
        });
        assert!(
            found,
            "RetryingHttpClient should have inherits edge to HttpClient"
        );
    }

    // ── C# (80–86) ───────────────────────────────────────────────────────────────

    #[test]
    fn test_csharp_import_edges_have_import_context() {
        let src = load_sample("sample.cs");
        let path = sample_path("sample.cs");
        let result = TsCSharpExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    #[test]
    fn test_csharp_inherits_edge() {
        let src = load_sample("sample.cs");
        let path = sample_path("sample.cs");
        let result = TsCSharpExtractor::extract(&src, &path).unwrap();
        let inherits: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "inherits")
            .collect();
        assert!(!inherits.is_empty(), "expected at least one inherits edge");
    }

    #[test]
    fn test_csharp_implements_iprocessor() {
        let src = load_sample("sample.cs");
        let path = sample_path("sample.cs");
        let result = TsCSharpExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let found = result.edges.iter().any(|e| {
            e.relation == "implements"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("IProcessor"))
                    .unwrap_or(false)
        });
        assert!(
            found,
            "DataProcessor should have implements edge to IProcessor"
        );
    }

    #[test]
    fn test_csharp_splits_inherits_and_implements_edges() {
        let src = load_sample("sample.cs");
        let path = sample_path("sample.cs");
        let result = TsCSharpExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let has_inherits = result.edges.iter().any(|e| {
            e.relation == "inherits"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("Processor"))
                    .unwrap_or(false)
        });
        let has_implements = result.edges.iter().any(|e| {
            e.relation == "implements"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("IProcessor"))
                    .unwrap_or(false)
        });
        assert!(has_inherits, "expected inherits(DataProcessor, Processor)");
        assert!(
            has_implements,
            "expected implements(DataProcessor, IProcessor)"
        );
    }

    #[test]
    fn test_csharp_call_edges_have_call_context() {
        let src = load_sample("sample.cs");
        let path = sample_path("sample.cs");
        let result = TsCSharpExtractor::extract(&src, &path).unwrap();
        let call_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "calls")
            .collect();
        assert!(!call_edges.is_empty(), "expected call edges");
        assert!(
            call_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("call")),
            "all call edges must have context='call'"
        );
    }

    #[test]
    fn test_csharp_parameter_return_and_generic_contexts() {
        let src = load_sample("sample.cs");
        let path = sample_path("sample.cs");
        let result = TsCSharpExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let refs: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "references")
            .collect();
        let has_param = refs.iter().any(|e| {
            nodes
                .get(&e.source)
                .map(|s| s.contains("Build"))
                .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t == "HttpClient")
                    .unwrap_or(false)
                && e.context.as_deref() == Some("parameter_type")
        });
        assert!(
            has_param,
            "expected Build→HttpClient with parameter_type context"
        );
        let has_ret = refs.iter().any(|e| {
            nodes
                .get(&e.source)
                .map(|s| s.contains("Build"))
                .unwrap_or(false)
                && nodes.get(&e.target).map(|t| t == "Result").unwrap_or(false)
                && e.context.as_deref() == Some("return_type")
        });
        assert!(has_ret, "expected Build→Result with return_type context");
        let has_generic = refs.iter().any(|e| {
            nodes
                .get(&e.source)
                .map(|s| s.contains("Build"))
                .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t == "DataProcessor")
                    .unwrap_or(false)
                && e.context.as_deref() == Some("generic_arg")
        });
        assert!(
            has_generic,
            "expected Build→DataProcessor with generic_arg context"
        );
    }

    // ── Kotlin (87–88) ───────────────────────────────────────────────────────────

    #[test]
    fn test_kotlin_splits_inherits_and_implements() {
        let src = load_sample("sample.kt");
        let path = sample_path("sample.kt");
        let result = TsKotlinExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let has_inherits = result.edges.iter().any(|e| {
            e.relation == "inherits"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("BaseProcessor"))
                    .unwrap_or(false)
        });
        let has_implements = result.edges.iter().any(|e| {
            e.relation == "implements"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("Loggable"))
                    .unwrap_or(false)
        });
        assert!(
            has_inherits,
            "expected inherits(DataProcessor, BaseProcessor)"
        );
        assert!(
            has_implements,
            "expected implements(DataProcessor, Loggable)"
        );
    }

    #[test]
    fn test_kotlin_parameter_return_generic_and_field_contexts() {
        let src = load_sample("sample.kt");
        let path = sample_path("sample.kt");
        let result = TsKotlinExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let refs: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "references")
            .collect();
        let has_param = refs.iter().any(|e| {
            nodes
                .get(&e.source)
                .map(|s| s.contains("run"))
                .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t == "DataProcessor")
                    .unwrap_or(false)
                && e.context.as_deref() == Some("parameter_type")
        });
        assert!(
            has_param,
            "expected run→DataProcessor with parameter_type context"
        );
        let has_field = refs.iter().any(|e| {
            nodes
                .get(&e.source)
                .map(|s| s.contains("DataProcessor"))
                .unwrap_or(false)
                && nodes.get(&e.target).map(|t| t == "Result").unwrap_or(false)
                && e.context.as_deref() == Some("field")
        });
        assert!(
            has_field,
            "expected DataProcessor→Result with field context, refs: {:?}",
            refs.iter()
                .map(|e| (nodes.get(&e.source), nodes.get(&e.target), &e.context))
                .collect::<Vec<_>>()
        );
    }

    // ── Scala (89–90) ────────────────────────────────────────────────────────────

    #[test]
    fn test_scala_import_edges_have_import_context() {
        let src = load_sample("sample.scala");
        let path = sample_path("sample.scala");
        let result = TsScalaExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    #[test]
    fn test_scala_call_edges_have_call_context() {
        let src = load_sample("sample.scala");
        let path = sample_path("sample.scala");
        let result = TsScalaExtractor::extract(&src, &path).unwrap();
        let call_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "calls")
            .collect();
        // Scala call extraction is best-effort; only check context if calls exist
        if !call_edges.is_empty() {
            assert!(
                call_edges
                    .iter()
                    .all(|e| e.context.as_deref() == Some("call")),
                "all call edges must have context='call'"
            );
        }
    }

    // ── PHP (91–94) ──────────────────────────────────────────────────────────────

    #[test]
    fn test_php_import_edges_have_import_context() {
        let src = load_sample("sample.php");
        let path = sample_path("sample.php");
        let result = TsPhpExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    #[test]
    fn test_php_call_edges_have_call_context() {
        let src = load_sample("sample.php");
        let path = sample_path("sample.php");
        let result = TsPhpExtractor::extract(&src, &path).unwrap();
        let call_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "calls")
            .collect();
        if !call_edges.is_empty() {
            assert!(
                call_edges
                    .iter()
                    .all(|e| e.context.as_deref() == Some("call")),
                "all call edges must have context='call'"
            );
        }
    }

    #[test]
    fn test_php_splits_inherits_implements_mixes_in() {
        let src = load_sample("sample.php");
        let path = sample_path("sample.php");
        let result = TsPhpExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let has_inherits = result.edges.iter().any(|e| {
            e.relation == "inherits"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("BaseProcessor"))
                    .unwrap_or(false)
        });
        let has_implements = result.edges.iter().any(|e| {
            e.relation == "implements"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("Loggable"))
                    .unwrap_or(false)
        });
        let has_mixes_in = result.edges.iter().any(|e| {
            e.relation == "mixes_in"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("HasName"))
                    .unwrap_or(false)
        });
        assert!(
            has_inherits,
            "expected inherits(DataProcessor, BaseProcessor)"
        );
        assert!(
            has_implements,
            "expected implements(DataProcessor, Loggable)"
        );
        assert!(has_mixes_in, "expected mixes_in(DataProcessor, HasName)");
    }

    #[test]
    fn test_php_property_parameter_and_return_contexts() {
        let src = load_sample("sample.php");
        let path = sample_path("sample.php");
        let result = TsPhpExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let refs: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "references")
            .collect();
        let has_field = refs.iter().any(|e| {
            nodes
                .get(&e.source)
                .map(|s| s.contains("DataProcessor"))
                .unwrap_or(false)
                && nodes.get(&e.target).map(|t| t == "Result").unwrap_or(false)
                && e.context.as_deref() == Some("field")
        });
        assert!(
            has_field,
            "expected DataProcessor→Result with field context"
        );
    }

    // ── Swift (95–100) ───────────────────────────────────────────────────────────

    #[test]
    fn test_swift_import_edges_have_import_context() {
        let src = load_sample("sample.swift");
        let path = sample_path("sample.swift");
        let result = TsSwiftExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    #[test]
    fn test_swift_splits_inherits_and_implements() {
        let src = load_sample("sample.swift");
        let path = sample_path("sample.swift");
        let result = TsSwiftExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let has_inherits = result.edges.iter().any(|e| {
            e.relation == "inherits"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("BaseProcessor"))
                    .unwrap_or(false)
        });
        let has_implements = result.edges.iter().any(|e| {
            e.relation == "implements"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("Processor"))
                    .unwrap_or(false)
        });
        assert!(
            has_inherits,
            "expected inherits(DataProcessor, BaseProcessor)"
        );
        assert!(
            has_implements,
            "expected implements(DataProcessor, Processor)"
        );
    }

    #[test]
    fn test_swift_protocol_conformance_emits_implements() {
        let src = load_sample("sample.swift");
        let path = sample_path("sample.swift");
        let result = TsSwiftExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let found = result.edges.iter().any(|e| {
            e.relation == "implements"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("Processor"))
                    .unwrap_or(false)
        });
        assert!(
            found,
            "DataProcessor should have implements edge to Processor (protocol)"
        );
    }

    #[test]
    fn test_swift_extension_conformance_emits_implements() {
        let src = load_sample("sample.swift");
        let path = sample_path("sample.swift");
        let result = TsSwiftExtractor::extract(&src, &path).unwrap();
        let nodes: std::collections::HashMap<String, String> = result
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();
        let found = result.edges.iter().any(|e| {
            e.relation == "implements"
                && nodes
                    .get(&e.source)
                    .map(|s| s.contains("DataProcessor"))
                    .unwrap_or(false)
                && nodes
                    .get(&e.target)
                    .map(|t| t.contains("Loggable"))
                    .unwrap_or(false)
        });
        assert!(
            found,
            "extension DataProcessor: Loggable should emit implements edge"
        );
    }

    #[test]
    fn test_swift_call_edges_have_call_context() {
        let src = load_sample("sample.swift");
        let path = sample_path("sample.swift");
        let result = TsSwiftExtractor::extract(&src, &path).unwrap();
        let call_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "calls")
            .collect();
        if !call_edges.is_empty() {
            assert!(
                call_edges
                    .iter()
                    .all(|e| e.context.as_deref() == Some("call")),
                "all call edges must have context='call'"
            );
        }
    }

    // ── Elixir (101–102) ─────────────────────────────────────────────────────────

    #[test]
    fn test_elixir_import_edges_have_import_context() {
        let src = load_sample("sample.ex");
        let path = sample_path("sample.ex");
        let result = TsElixirExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(
            !import_edges.is_empty(),
            "expected import edges (alias/import/require/use)"
        );
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    #[test]
    fn test_elixir_call_edges_have_call_context() {
        let src = load_sample("sample.ex");
        let path = sample_path("sample.ex");
        let result = TsElixirExtractor::extract(&src, &path).unwrap();
        let call_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "calls")
            .collect();
        if !call_edges.is_empty() {
            assert!(
                call_edges
                    .iter()
                    .all(|e| e.context.as_deref() == Some("call")),
                "all call edges must have context='call'"
            );
        }
    }

    // ── ObjC (103) ───────────────────────────────────────────────────────────────

    #[test]
    fn test_objc_import_edges_have_import_context() {
        let src = load_sample("sample.m");
        let path = sample_path("sample.m");
        let result = ObjCExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    // ── Julia (104–105) ──────────────────────────────────────────────────────────

    #[test]
    fn test_julia_import_edges_have_import_context() {
        let src = load_sample("sample.jl");
        let path = sample_path("sample.jl");
        let result = TsJuliaExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    #[test]
    fn test_julia_call_edges_have_call_context() {
        let src = load_sample("sample.jl");
        let path = sample_path("sample.jl");
        let result = TsJuliaExtractor::extract(&src, &path).unwrap();
        let call_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "calls")
            .collect();
        if !call_edges.is_empty() {
            assert!(
                call_edges
                    .iter()
                    .all(|e| e.context.as_deref() == Some("call")),
                "all call edges must have context='call'"
            );
        }
    }

    // ── Fortran (106) ────────────────────────────────────────────────────────────

    #[test]
    fn test_fortran_use_edges_have_use_context() {
        let src = load_sample("sample.f90");
        let path = sample_path("sample.f90");
        let result = FortranExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected use edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("use")),
            "all use edges must have context='use'"
        );
    }

    // ── Groovy (107) ─────────────────────────────────────────────────────────────

    #[test]
    fn test_groovy_import_edges_have_import_context() {
        let src = load_sample("sample.groovy");
        let path = sample_path("sample.groovy");
        let result = TsGroovyExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edges");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "all import edges must have context='import'"
        );
    }

    // ── SLN extractor (108–111) ──────────────────────────────────────────────────

    #[test]
    fn test_sln_no_error() {
        let src = load_sample("sample.sln");
        let path = sample_path("sample.sln");
        let result = SlnExtractor::extract(&src, &path).unwrap();
        assert!(!result.nodes.is_empty(), "expected nodes");
    }

    #[test]
    fn test_sln_finds_projects() {
        let src = load_sample("sample.sln");
        let path = sample_path("sample.sln");
        let result = SlnExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.contains("WebApi")),
            "expected WebApi project"
        );
        assert!(
            labels.iter().any(|l| l.contains("Domain")),
            "expected Domain project"
        );
    }

    #[test]
    fn test_sln_contains_edges() {
        let src = load_sample("sample.sln");
        let path = sample_path("sample.sln");
        let result = SlnExtractor::extract(&src, &path).unwrap();
        assert!(
            result.edges.iter().any(|e| e.relation == "contains"),
            "expected contains edges"
        );
    }

    #[test]
    fn test_sln_project_dependency_edges() {
        let src = load_sample("sample.sln");
        let path = sample_path("sample.sln");
        let result = SlnExtractor::extract(&src, &path).unwrap();
        assert!(
            result.edges.iter().any(|e| e.relation == "imports"),
            "expected dependency imports edges"
        );
    }

    // ── CSPROJ extractor (112–116) ───────────────────────────────────────────────

    #[test]
    fn test_csproj_no_error() {
        let src = load_sample("sample.csproj");
        let path = sample_path("sample.csproj");
        let result = CsprojExtractor::extract(&src, &path).unwrap();
        assert!(!result.nodes.is_empty(), "expected nodes");
    }

    #[test]
    fn test_csproj_finds_packages() {
        let src = load_sample("sample.csproj");
        let path = sample_path("sample.csproj");
        let result = CsprojExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.contains("MediatR")),
            "expected MediatR package"
        );
        assert!(
            labels.iter().any(|l| l.contains("FluentValidation")),
            "expected FluentValidation package"
        );
    }

    #[test]
    fn test_csproj_finds_project_references() {
        let src = load_sample("sample.csproj");
        let path = sample_path("sample.csproj");
        let result = CsprojExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.contains("Domain.csproj")),
            "expected Domain.csproj reference"
        );
    }

    #[test]
    fn test_csproj_finds_target_framework() {
        let src = load_sample("sample.csproj");
        let path = sample_path("sample.csproj");
        let result = CsprojExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.contains("net8.0")),
            "expected net8.0 target framework"
        );
    }

    #[test]
    fn test_csproj_finds_sdk() {
        let src = load_sample("sample.csproj");
        let path = sample_path("sample.csproj");
        let result = CsprojExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.contains("Microsoft.NET.Sdk.Web")),
            "expected SDK node"
        );
    }

    // ── Razor extractor (117–122) ────────────────────────────────────────────────

    #[test]
    fn test_razor_no_error() {
        let src = load_sample("sample.razor");
        let path = sample_path("sample.razor");
        let result = RazorExtractor::extract(&src, &path).unwrap();
        assert!(!result.nodes.is_empty(), "expected nodes");
    }

    #[test]
    fn test_razor_finds_using_directives() {
        let src = load_sample("sample.razor");
        let path = sample_path("sample.razor");
        let result = RazorExtractor::extract(&src, &path).unwrap();
        assert!(
            result.edges.iter().any(|e| e.relation == "imports"),
            "expected import edges"
        );
    }

    #[test]
    fn test_razor_finds_component_references() {
        let src = load_sample("sample.razor");
        let path = sample_path("sample.razor");
        let result = RazorExtractor::extract(&src, &path).unwrap();
        assert!(
            result.edges.iter().any(|e| e.relation == "calls"),
            "expected component call edges"
        );
    }

    #[test]
    fn test_razor_finds_inherits() {
        let src = load_sample("sample.razor");
        let path = sample_path("sample.razor");
        let result = RazorExtractor::extract(&src, &path).unwrap();
        assert!(
            result.edges.iter().any(|e| e.relation == "inherits"),
            "expected inherits edge"
        );
    }

    #[test]
    fn test_razor_finds_code_block_methods() {
        let src = load_sample("sample.razor");
        let path = sample_path("sample.razor");
        let result = RazorExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.contains("IncrementCount")),
            "expected IncrementCount method"
        );
        assert!(
            labels.iter().any(|l| l.contains("LoadData")),
            "expected LoadData method"
        );
    }

    #[test]
    fn test_razor_no_dangling_edges() {
        let src = load_sample("sample.razor");
        let path = sample_path("sample.razor");
        let result = RazorExtractor::extract(&src, &path).unwrap();
        let node_ids: std::collections::HashSet<_> =
            result.nodes.iter().map(|n| n.id.as_str()).collect();
        for e in &result.edges {
            assert!(
                node_ids.contains(e.source.as_str()),
                "dangling source: {}",
                e.source
            );
        }
    }

    // ── DM extractor (123–135) ───────────────────────────────────────────────────

    #[test]
    fn test_dm_no_error() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        assert!(!result.nodes.is_empty(), "expected nodes");
    }

    #[test]
    fn test_dm_finds_global_proc() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(labels.contains(&"log_event()"), "expected log_event() node");
        assert!(labels.contains(&"RunTest()"), "expected RunTest() node");
    }

    #[test]
    fn test_dm_finds_type_definition() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.contains(&"/datum/weapon"),
            "expected /datum/weapon node"
        );
        assert!(
            labels.contains(&"/datum/weapon/sword"),
            "expected /datum/weapon/sword node"
        );
    }

    #[test]
    fn test_dm_qualifies_proc_with_type_path() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.contains(&"/datum/weapon/attack()"),
            "expected /datum/weapon/attack()"
        );
        assert!(
            labels.contains(&"/datum/weapon/sword/attack()"),
            "expected /datum/weapon/sword/attack()"
        );
    }

    #[test]
    fn test_dm_finds_path_form_proc_definition() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.contains(&"/datum/weapon/sword/sharpen()"),
            "expected /datum/weapon/sword/sharpen()"
        );
    }

    #[test]
    fn test_dm_emits_include_edge() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let import_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports" || e.relation == "imports_from")
            .collect();
        assert!(!import_edges.is_empty(), "expected import edge");
        assert!(
            import_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("import")),
            "import edges must have context='import'"
        );
    }

    #[test]
    fn test_dm_unresolved_include_flagged_external() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let helpers_edge = result.edges.iter().find(|e| {
            e.target.contains("helpers")
                && (e.relation == "imports" || e.relation == "imports_from")
        });
        assert!(helpers_edge.is_some(), "expected edge targeting helpers");
        // Unresolved includes use "imports" relation (not "imports_from")
        assert_eq!(
            helpers_edge.unwrap().relation,
            "imports",
            "unresolved include must use 'imports' relation"
        );
    }

    #[test]
    fn test_dm_resolves_in_file_calls() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let node_by_id: std::collections::HashMap<_, _> = result
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect();
        let calls: Vec<(&str, &str)> = result
            .edges
            .iter()
            .filter(|e| e.relation == "calls")
            .map(|e| {
                (
                    node_by_id.get(e.source.as_str()).copied().unwrap_or(""),
                    node_by_id.get(e.target.as_str()).copied().unwrap_or(""),
                )
            })
            .collect();
        assert!(
            calls.iter().any(|(_, callee)| *callee == "log_event()"),
            "expected some proc to call log_event()"
        );
        assert!(
            calls.contains(&(
                "/datum/weapon/sword/attack()",
                "/datum/weapon/sword/sharpen()"
            )),
            "expected /datum/weapon/sword/attack() calls /datum/weapon/sword/sharpen()"
        );
    }

    #[test]
    fn test_dm_ambiguous_member_call_left_unresolved() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let node_by_id: std::collections::HashMap<_, _> = result
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect();
        let runtest_to_attack: Vec<_> = result
            .edges
            .iter()
            .filter(|e| {
                e.relation == "calls"
                    && node_by_id.get(e.source.as_str()).copied() == Some("RunTest()")
                    && node_by_id
                        .get(e.target.as_str())
                        .copied()
                        .map(|l| l.contains("attack"))
                        .unwrap_or(false)
            })
            .collect();
        assert!(
            runtest_to_attack.is_empty(),
            "ambiguous call to 'attack' must not be resolved"
        );
    }

    #[test]
    fn test_dm_emits_new_as_instantiates() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let node_by_id: std::collections::HashMap<_, _> = result
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect();
        let inst: Vec<(&str, &str)> = result
            .edges
            .iter()
            .filter(|e| e.relation == "instantiates")
            .map(|e| {
                (
                    node_by_id.get(e.source.as_str()).copied().unwrap_or(""),
                    node_by_id.get(e.target.as_str()).copied().unwrap_or(""),
                )
            })
            .collect();
        assert!(
            inst.contains(&("RunTest()", "/datum/weapon/sword")),
            "expected RunTest() instantiates /datum/weapon/sword, got: {:?}",
            inst
        );
    }

    #[test]
    fn test_dm_call_edges_have_call_context() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let call_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "calls" || e.relation == "instantiates")
            .collect();
        assert!(!call_edges.is_empty(), "expected call/instantiates edges");
        assert!(
            call_edges
                .iter()
                .all(|e| e.context.as_deref() == Some("call")),
            "all call/instantiates edges must have context='call'"
        );
    }

    #[test]
    fn test_dm_no_dangling_edges() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let node_ids: std::collections::HashSet<_> =
            result.nodes.iter().map(|n| n.id.as_str()).collect();
        for e in &result.edges {
            assert!(
                node_ids.contains(e.source.as_str()),
                "dangling source: {} (relation={})",
                e.source,
                e.relation
            );
        }
    }

    #[test]
    fn test_dm_super_call_not_emitted() {
        let src = load_sample("sample.dm");
        let path = sample_path("sample.dm");
        let result = DmExtractor::extract(&src, &path).unwrap();
        let node_by_label: std::collections::HashMap<_, _> = result
            .nodes
            .iter()
            .map(|n| (n.label.as_str(), n.id.as_str()))
            .collect();
        // No node or edge should involve ".."
        for n in &result.nodes {
            assert!(!n.label.contains(".."), "node with .. label: {}", n.label);
        }
        for e in &result.edges {
            let src_label = node_by_label.get(e.source.as_str()).copied().unwrap_or("");
            let tgt_label = node_by_label.get(e.target.as_str()).copied().unwrap_or("");
            assert!(
                !src_label.contains("..") && !tgt_label.contains(".."),
                "edge involving ..: {} -> {}",
                e.source,
                e.target
            );
        }
    }

    // ── DMI extractor (136–138) ──────────────────────────────────────────────────

    #[test]
    fn test_dmi_no_error() {
        let src = load_sample("sample.dmi");
        let path = sample_path("sample.dmi");
        let result = DmiExtractor::extract(&src, &path).unwrap();
        assert!(!result.nodes.is_empty(), "expected nodes");
    }

    #[test]
    fn test_dmi_emits_state_nodes() {
        let src = load_sample("sample.dmi");
        let path = sample_path("sample.dmi");
        let result = DmiExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(labels.contains(&"\"mob\""), "expected \"mob\" state node");
    }

    #[test]
    fn test_dmi_state_contained_by_file() {
        let src = load_sample("sample.dmi");
        let path = sample_path("sample.dmi");
        let result = DmiExtractor::extract(&src, &path).unwrap();
        let node_by_id: std::collections::HashMap<_, _> = result
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect();
        let contains: Vec<(&str, &str)> = result
            .edges
            .iter()
            .filter(|e| e.relation == "contains")
            .map(|e| {
                (
                    node_by_id.get(e.source.as_str()).copied().unwrap_or(""),
                    node_by_id.get(e.target.as_str()).copied().unwrap_or(""),
                )
            })
            .collect();
        assert!(
            contains.contains(&("sample.dmi", "\"mob\"")),
            "expected (sample.dmi, \"mob\") contains edge, got: {:?}",
            contains
        );
    }

    // ── DMM extractor (139–143) ──────────────────────────────────────────────────

    #[test]
    fn test_dmm_no_error() {
        let src = load_sample("sample.dmm");
        let path = sample_path("sample.dmm");
        let result = DmmExtractor::extract(&src, &path).unwrap();
        assert!(!result.nodes.is_empty(), "expected nodes");
    }

    #[test]
    fn test_dmm_extracts_type_paths_as_uses_edges() {
        let src = load_sample("sample.dmm");
        let path = sample_path("sample.dmm");
        let result = DmmExtractor::extract(&src, &path).unwrap();
        let targets: std::collections::HashSet<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "uses")
            .map(|e| e.target.as_str())
            .collect();
        // make_id(&["/turf/closed/wall"]) = "_turf_closed_wall" (leading _ not stripped in Rust)
        assert!(
            targets
                .iter()
                .any(|t| t.contains("turf") && t.contains("closed") && t.contains("wall")),
            "expected turf_closed_wall target"
        );
        assert!(
            targets
                .iter()
                .any(|t| t.contains("obj") && t.contains("structure") && t.contains("table")),
            "expected obj_structure_table target"
        );
        assert!(
            targets.iter().any(|t| t.contains("obj")
                && t.contains("item")
                && t.contains("weapon")
                && t.contains("sword")),
            "expected obj_item_weapon_sword target"
        );
    }

    #[test]
    fn test_dmm_strips_var_overrides() {
        let src = load_sample("sample.dmm");
        let path = sample_path("sample.dmm");
        let result = DmmExtractor::extract(&src, &path).unwrap();
        let targets: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "uses")
            .map(|e| e.target.as_str())
            .collect();
        assert!(
            !targets.iter().any(|t| t.contains('{')),
            "no targets should contain {{"
        );
        assert!(
            targets.iter().any(|t| t.contains("sword")),
            "sword target should exist after stripping overrides"
        );
    }

    #[test]
    fn test_dmm_handles_multiline_tile_definition() {
        let src = load_sample("sample.dmm");
        let path = sample_path("sample.dmm");
        let result = DmmExtractor::extract(&src, &path).unwrap();
        let targets: std::collections::HashSet<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "uses")
            .map(|e| e.target.as_str())
            .collect();
        assert!(
            targets
                .iter()
                .any(|t| t.contains("area") && t.contains("station") && t.contains("maintenance")),
            "expected area_station_maintenance from multiline tile"
        );
    }

    #[test]
    fn test_dmm_skips_grid_section() {
        let src = load_sample("sample.dmm");
        let path = sample_path("sample.dmm");
        let result = DmmExtractor::extract(&src, &path).unwrap();
        let targets: std::collections::HashSet<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "uses")
            .map(|e| e.target.as_str())
            .collect();
        assert_eq!(
            targets.len(),
            5,
            "expected exactly 5 unique type-path targets, got: {:?}",
            targets
        );
    }

    // ── DMF extractor (144–148) ──────────────────────────────────────────────────

    #[test]
    fn test_dmf_no_error() {
        let src = load_sample("sample.dmf");
        let path = sample_path("sample.dmf");
        let result = DmfExtractor::extract(&src, &path).unwrap();
        assert!(!result.nodes.is_empty(), "expected nodes");
    }

    #[test]
    fn test_dmf_extracts_windows() {
        let src = load_sample("sample.dmf");
        let path = sample_path("sample.dmf");
        let result = DmfExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.contains(&"window \"mapwindow\""),
            "expected window mapwindow"
        );
        assert!(
            labels.contains(&"window \"infowindow\""),
            "expected window infowindow"
        );
    }

    #[test]
    fn test_dmf_elem_labels_carry_control_type() {
        let src = load_sample("sample.dmf");
        let path = sample_path("sample.dmf");
        let result = DmfExtractor::extract(&src, &path).unwrap();
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.contains(&"elem \"map\" [MAP]"),
            "expected elem \"map\" [MAP], got: {:?}",
            labels
        );
    }

    #[test]
    fn test_dmf_elem_under_window() {
        let src = load_sample("sample.dmf");
        let path = sample_path("sample.dmf");
        let result = DmfExtractor::extract(&src, &path).unwrap();
        let node_by_id: std::collections::HashMap<_, _> = result
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect();
        let contains: Vec<(&str, &str)> = result
            .edges
            .iter()
            .filter(|e| e.relation == "contains")
            .map(|e| {
                (
                    node_by_id.get(e.source.as_str()).copied().unwrap_or(""),
                    node_by_id.get(e.target.as_str()).copied().unwrap_or(""),
                )
            })
            .collect();
        assert!(
            contains.contains(&("window \"mapwindow\"", "elem \"map\" [MAP]")),
            "expected mapwindow contains map elem, got: {:?}",
            contains
        );
    }

    #[test]
    fn test_dmf_no_dangling_edges() {
        let src = load_sample("sample.dmf");
        let path = sample_path("sample.dmf");
        let result = DmfExtractor::extract(&src, &path).unwrap();
        let node_ids: std::collections::HashSet<_> =
            result.nodes.iter().map(|n| n.id.as_str()).collect();
        for e in &result.edges {
            assert!(
                node_ids.contains(e.source.as_str()),
                "dangling source: {}",
                e.source
            );
            assert!(
                node_ids.contains(e.target.as_str()),
                "dangling target: {}",
                e.target
            );
        }
    }

    // ── strip_docstring helper ─────────────────────────────────────────────────

    #[test]
    fn test_strip_docstring_triple_quote() {
        let raw = r#""""Handles payment processing.""""#;
        assert_eq!(
            strip_docstring(raw),
            Some("Handles payment processing.".into())
        );
    }

    #[test]
    fn test_strip_docstring_triple_quote_multiline() {
        let raw = "\"\"\"\n    Line one.\n    Line two.\n    \"\"\"";
        let result = strip_docstring(raw).unwrap();
        assert!(
            result.contains("Line one."),
            "expected Line one. in {}",
            result
        );
        assert!(
            result.contains("Line two."),
            "expected Line two. in {}",
            result
        );
    }

    #[test]
    fn test_strip_docstring_javadoc() {
        let raw = "/** Processes payments. */";
        assert_eq!(strip_docstring(raw), Some("Processes payments.".into()));
    }

    #[test]
    fn test_strip_docstring_javadoc_multiline() {
        let raw = "/**\n * Manages connections.\n * Pooled.\n */";
        let result = strip_docstring(raw).unwrap();
        assert!(result.contains("Manages connections."), "got: {}", result);
        assert!(result.contains("Pooled."), "got: {}", result);
    }

    #[test]
    fn test_strip_docstring_empty_returns_none() {
        assert_eq!(strip_docstring(r#""""  """"#), None);
    }

    // ── run_query_matches ──────────────────────────────────────────────────────

    #[test]
    fn test_run_query_matches_per_match_groups() {
        let source = b"class Foo(Bar):\n    pass\nclass Baz:\n    pass\n";
        let lang = tree_sitter_python::LANGUAGE.into();
        let query = r#"
            (class_definition
                name: (identifier) @class.name
                superclasses: (argument_list
                    (identifier) @class.base
                )?
            )
        "#;
        let matches = run_query_matches(source, &lang, query).unwrap();
        assert_eq!(matches.len(), 2, "expected 2 matches");
        let foo_match = matches
            .iter()
            .find(|m| m.get("class.name").map(|s| s == "Foo").unwrap_or(false));
        assert!(foo_match.is_some());
        assert_eq!(
            foo_match.unwrap().get("class.base").map(|s| s.as_str()),
            Some("Bar")
        );
        let baz_match = matches
            .iter()
            .find(|m| m.get("class.name").map(|s| s == "Baz").unwrap_or(false));
        assert!(baz_match.is_some());
        assert!(
            baz_match.unwrap().get("class.base").is_none(),
            "Baz has no base, capture must be absent"
        );
    }

    // ── Python docstring extraction ────────────────────────────────────────────

    #[test]
    fn test_python_class_with_docstring() {
        let source = b"class Foo:\n    \"\"\"Handles payment processing.\"\"\"\n    pass\n";
        let result = TsPythonExtractor::extract(source, Path::new("test.py")).unwrap();
        let foo = result.nodes.iter().find(|n| n.label == "Foo").unwrap();
        assert_eq!(foo.docstring, Some("Handles payment processing.".into()));
    }

    #[test]
    fn test_python_class_without_docstring() {
        let source = b"class Bar:\n    pass\n";
        let result = TsPythonExtractor::extract(source, Path::new("test.py")).unwrap();
        let bar = result.nodes.iter().find(|n| n.label == "Bar").unwrap();
        assert_eq!(bar.docstring, None);
    }

    #[test]
    fn test_python_fn_with_docstring() {
        let source = b"def greet(name):\n    \"\"\"Say hello.\"\"\"\n    pass\n";
        let result = TsPythonExtractor::extract(source, Path::new("test.py")).unwrap();
        let greet = result.nodes.iter().find(|n| n.label == "greet()").unwrap();
        assert_eq!(greet.docstring, Some("Say hello.".into()));
    }

    #[test]
    fn test_python_fn_without_docstring() {
        let source = b"def no_doc():\n    pass\n";
        let result = TsPythonExtractor::extract(source, Path::new("test.py")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "no_doc()").unwrap();
        assert_eq!(node.docstring, None);
    }

    // ── Java docstring extraction ──────────────────────────────────────────────

    #[test]
    fn test_java_class_with_javadoc() {
        let source = b"/** Processes payments. */\npublic class PaymentService {}\n";
        let result = TsJavaExtractor::extract(source, Path::new("PaymentService.java")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "PaymentService")
            .unwrap();
        assert_eq!(node.docstring, Some("Processes payments.".into()));
    }

    #[test]
    fn test_java_class_without_javadoc() {
        let source = b"public class NoDoc {}\n";
        let result = TsJavaExtractor::extract(source, Path::new("NoDoc.java")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "NoDoc").unwrap();
        assert_eq!(node.docstring, None);
    }

    #[test]
    fn test_java_non_javadoc_comment_ignored() {
        let source = b"/* Not a javadoc */\npublic class AlsoNoDoc {}\n";
        let result = TsJavaExtractor::extract(source, Path::new("AlsoNoDoc.java")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "AlsoNoDoc")
            .unwrap();
        assert_eq!(node.docstring, None);
    }

    #[test]
    fn test_java_method_with_javadoc() {
        let source =
            b"public class Svc {\n    /** Charge the card. */\n    public void charge() {}\n}\n";
        let result = TsJavaExtractor::extract(source, Path::new("Svc.java")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "charge()").unwrap();
        assert_eq!(node.docstring, Some("Charge the card.".into()));
    }

    // ── JS docstring extraction ────────────────────────────────────────────────

    #[test]
    fn test_js_class_with_jsdoc() {
        let source = b"/** Handles auth flows. */\nclass AuthManager {}\n";
        let result = TsJavaScriptExtractor::extract(source, Path::new("auth.js")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "AuthManager")
            .unwrap();
        assert_eq!(node.docstring, Some("Handles auth flows.".into()));
    }

    #[test]
    fn test_js_class_without_jsdoc() {
        let source = b"class NoDocs {}\n";
        let result = TsJavaScriptExtractor::extract(source, Path::new("nodocs.js")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "NoDocs").unwrap();
        assert_eq!(node.docstring, None);
    }

    // --- JS/TS/Java source_location (Priority 2) ---

    #[test]
    fn test_js_fn_source_location_set() {
        let source = b"function greet(name) {\n  return 'hi';\n}\n";
        let result = TsJavaScriptExtractor::extract(source, Path::new("greet.js")).unwrap();
        let fn_node = result.nodes.iter().find(|n| n.label == "greet()").unwrap();
        let loc = fn_node
            .source_location
            .as_ref()
            .expect("source_location set");
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert_eq!(start, 0);
        assert!(end > start);
        assert!(source[start..end].starts_with(b"function"));
    }

    #[test]
    fn test_js_method_source_location_set() {
        let source = b"class Foo {\n  bar() {\n    return 1;\n  }\n}\n";
        let result = TsJavaScriptExtractor::extract(source, Path::new("foo.js")).unwrap();
        let method_node = result.nodes.iter().find(|n| n.label == "bar()").unwrap();
        let loc = method_node
            .source_location
            .as_ref()
            .expect("source_location set");
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert!(end > start);
    }

    #[test]
    fn test_js_arrow_fn_source_location_set() {
        let source = b"const add = (a, b) => a + b;\n";
        let result = TsJavaScriptExtractor::extract(source, Path::new("add.js")).unwrap();
        let fn_node = result.nodes.iter().find(|n| n.label == "add()").unwrap();
        let loc = fn_node
            .source_location
            .as_ref()
            .expect("source_location set");
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert!(end > start);
    }

    #[test]
    fn test_js_class_source_location_set() {
        let source = b"class Widget {\n  render() {}\n}\n";
        let result = TsJavaScriptExtractor::extract(source, Path::new("widget.js")).unwrap();
        let class_node = result.nodes.iter().find(|n| n.label == "Widget").unwrap();
        let loc = class_node
            .source_location
            .as_ref()
            .expect("source_location set");
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert_eq!(start, 0);
        assert!(end > start);
    }

    #[test]
    fn test_ts_fn_source_location_set() {
        let source = b"function parse(input: string): number {\n  return 0;\n}\n";
        let result = TsTypeScriptExtractor::extract(source, Path::new("parse.ts")).unwrap();
        let fn_node = result.nodes.iter().find(|n| n.label == "parse()").unwrap();
        let loc = fn_node
            .source_location
            .as_ref()
            .expect("source_location set");
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert_eq!(start, 0);
        assert!(end > start);
    }

    #[test]
    fn test_java_method_source_location_set() {
        let source = b"class Svc {\n  void process() {\n    return;\n  }\n}\n";
        let result = TsJavaExtractor::extract(source, Path::new("Svc.java")).unwrap();
        let method_node = result
            .nodes
            .iter()
            .find(|n| n.label == "process()")
            .unwrap();
        let loc = method_node
            .source_location
            .as_ref()
            .expect("source_location set");
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert!(end > start);
    }

    #[test]
    fn test_java_class_calls_edges_via_field_injection() {
        let source =
            b"class OrderService {\n  private PaymentGateway paymentGateway;\n  private NotificationSender notificationSender;\n  void pay() {}\n}\n";
        let result = TsJavaExtractor::extract(source, Path::new("OrderService.java")).unwrap();
        let calls_edges: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.relation == "calls")
            .collect();
        assert_eq!(calls_edges.len(), 2, "expected 2 calls edges");
        let targets: Vec<&str> = calls_edges.iter().map(|e| e.target.as_str()).collect();
        assert!(
            targets.iter().any(|t| t.contains("paymentgateway")),
            "expected calls edge to PaymentGateway"
        );
        assert!(
            targets.iter().any(|t| t.contains("notificationsender")),
            "expected calls edge to NotificationSender"
        );
    }

    #[test]
    fn test_java_class_source_location_set() {
        let source = b"class MyService {\n  void run() {}\n}\n";
        let result = TsJavaExtractor::extract(source, Path::new("MyService.java")).unwrap();
        let class_node = result
            .nodes
            .iter()
            .find(|n| n.label == "MyService")
            .unwrap();
        let loc = class_node
            .source_location
            .as_ref()
            .expect("source_location set");
        let parts: Vec<&str> = loc.split(':').collect();
        assert_eq!(parts.len(), 2);
        let start: usize = parts[0].parse().unwrap();
        let end: usize = parts[1].parse().unwrap();
        assert_eq!(start, 0);
        assert!(end > start);
    }

    // ---- Tier 3: Kotlin ----

    #[test]
    fn test_kotlin_class_file_type() {
        let source = b"class UserService {\n}\n";
        let result = TsKotlinExtractor::extract(source, Path::new("UserService.kt")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "UserService")
            .unwrap();
        assert_eq!(node.file_type, "class");
    }

    #[test]
    fn test_kotlin_interface_file_type() {
        let source = b"interface Repository {\n}\n";
        let result = TsKotlinExtractor::extract(source, Path::new("Repo.kt")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "Repository")
            .unwrap();
        assert_eq!(node.file_type, "interface");
    }

    #[test]
    fn test_kotlin_enum_class_file_type() {
        let source = b"enum class Status { ACTIVE, INACTIVE }\n";
        let result = TsKotlinExtractor::extract(source, Path::new("Status.kt")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "Status").unwrap();
        assert_eq!(node.file_type, "enum");
    }

    #[test]
    fn test_kotlin_toplevel_fun_file_type() {
        let source = b"fun buildClient(): Client {\n}\n";
        let result = TsKotlinExtractor::extract(source, Path::new("client.kt")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == ".buildClient()")
            .unwrap();
        assert_eq!(node.file_type, "function");
    }

    #[test]
    fn test_kotlin_class_method_file_type() {
        let source = b"class Service {\n    fun process() {}\n}\n";
        let result = TsKotlinExtractor::extract(source, Path::new("Service.kt")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == ".process()")
            .unwrap();
        assert_eq!(node.file_type, "method");
    }

    // ---- Tier 3: Swift ----

    #[test]
    fn test_swift_class_file_type() {
        let source = b"class AuthManager {\n}\n";
        let result = TsSwiftExtractor::extract(source, Path::new("Auth.swift")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "AuthManager")
            .unwrap();
        assert_eq!(node.file_type, "class");
    }

    #[test]
    fn test_swift_protocol_file_type() {
        let source = b"protocol Authenticatable {\n}\n";
        let result = TsSwiftExtractor::extract(source, Path::new("Auth.swift")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "Authenticatable")
            .unwrap();
        assert_eq!(node.file_type, "trait");
    }

    #[test]
    fn test_swift_toplevel_function_file_type() {
        let source = b"func buildURL(path: String) -> String { return path }\n";
        let result = TsSwiftExtractor::extract(source, Path::new("util.swift")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "buildURL()")
            .unwrap();
        assert_eq!(node.file_type, "function");
    }

    #[test]
    fn test_swift_method_file_type() {
        let source = b"class Session {\n  func login() {}\n}\n";
        let result = TsSwiftExtractor::extract(source, Path::new("Session.swift")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "login()").unwrap();
        assert_eq!(node.file_type, "method");
    }

    // ---- Tier 3: PHP ----

    #[test]
    fn test_php_class_file_type() {
        let source = b"<?php\nclass OrderController {\n}\n";
        let result = TsPhpExtractor::extract(source, Path::new("Order.php")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "OrderController")
            .unwrap();
        assert_eq!(node.file_type, "class");
    }

    #[test]
    fn test_php_method_file_type() {
        let source = b"<?php\nclass OrderController {\n    public function create() {}\n}\n";
        let result = TsPhpExtractor::extract(source, Path::new("Order.php")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "create()").unwrap();
        assert_eq!(node.file_type, "method");
    }

    #[test]
    fn test_php_function_file_type() {
        let source = b"<?php\nfunction formatDate($d) { return $d; }\n";
        let result = TsPhpExtractor::extract(source, Path::new("utils.php")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "formatDate()")
            .unwrap();
        assert_eq!(node.file_type, "function");
    }

    // ---- Tier 3: C++ ----

    #[test]
    fn test_cpp_class_file_type() {
        let source = b"class Connection {\n};\n";
        let result = TsCppExtractor::extract(source, Path::new("conn.cpp")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "Connection")
            .unwrap();
        assert_eq!(node.file_type, "class");
    }

    #[test]
    fn test_cpp_struct_file_type() {
        let source = b"struct Config {\n  int port;\n};\n";
        let result = TsCppExtractor::extract(source, Path::new("cfg.cpp")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "Config").unwrap();
        assert_eq!(node.file_type, "struct");
    }

    #[test]
    fn test_cpp_function_file_type() {
        let source = b"int add(int a, int b) { return a + b; }\n";
        let result = TsCppExtractor::extract(source, Path::new("math.cpp")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "add()").unwrap();
        assert_eq!(node.file_type, "function");
    }

    // ---- Tier 3: Dart ----

    #[test]
    fn test_dart_class_file_type() {
        let source = b"class Widget {\n}\n";
        let result = TsDartExtractor::extract(source, Path::new("widget.dart")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "Widget").unwrap();
        assert_eq!(node.file_type, "class");
    }

    // ---- Tier 3: Groovy ----

    #[test]
    fn test_groovy_class_file_type() {
        let source = b"class Pipeline {\n}\n";
        let result = TsGroovyExtractor::extract(source, Path::new("Pipeline.groovy")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "Pipeline").unwrap();
        assert_eq!(node.file_type, "class");
    }

    // ---- Tier 3: Julia ----

    #[test]
    fn test_julia_struct_file_type() {
        let source = b"struct Point\n  x::Float64\n  y::Float64\nend\n";
        let result = TsJuliaExtractor::extract(source, Path::new("geom.jl")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "Point").unwrap();
        assert_eq!(node.file_type, "struct");
    }

    #[test]
    fn test_julia_abstract_type_file_type() {
        let source = b"abstract type Shape end\n";
        let result = TsJuliaExtractor::extract(source, Path::new("geom.jl")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "Shape").unwrap();
        assert_eq!(node.file_type, "class");
    }

    // ---- Tier 3: Zig ----

    #[test]
    fn test_zig_struct_file_type() {
        let source = b"const Server = struct {\n    port: u16,\n};\n";
        let result = TsZigExtractor::extract(source, Path::new("server.zig")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "Server").unwrap();
        assert_eq!(node.file_type, "struct");
    }

    // ---- Tier 4: Bash ----

    #[test]
    fn test_bash_function_file_type() {
        let source = b"setup() {\n  echo hello\n}\n";
        let result = TsBashExtractor::extract(source, Path::new("deploy.sh")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "setup()").unwrap();
        assert_eq!(node.file_type, "function");
    }

    // ---- Tier 4: C ----

    #[test]
    fn test_c_function_file_type() {
        let source = b"int compute(int x) { return x * 2; }\n";
        let result = TsCExtractor::extract(source, Path::new("math.c")).unwrap();
        let node = result
            .nodes
            .iter()
            .find(|n| n.label == "compute()")
            .unwrap();
        assert_eq!(node.file_type, "function");
    }

    // ---- Tier 4: Vue ----

    #[test]
    fn test_vue_component_file_type() {
        let source = b"<script>\nexport default {\n  name: 'UserCard',\n}\n</script>\n";
        let result = TsVueExtractor::extract(source, Path::new("UserCard.vue")).unwrap();
        let node = result.nodes.iter().find(|n| n.label == "UserCard").unwrap();
        assert_eq!(node.file_type, "class");
    }
}
