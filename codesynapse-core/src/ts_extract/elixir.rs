use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_named};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node as TsNode, Parser};

pub struct TsElixirExtractor;

fn ex_text(source: &[u8], node: &TsNode<'_>) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .trim()
        .to_string()
}

const ELIXIR_KEYWORDS: &[&str] = &[
    "def",
    "defp",
    "defmodule",
    "defmacro",
    "defmacrop",
    "defstruct",
    "defprotocol",
    "defimpl",
    "defguard",
    "alias",
    "import",
    "require",
    "use",
    "if",
    "unless",
    "case",
    "cond",
    "with",
    "for",
];

impl TsElixirExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_elixir::LANGUAGE.into();

        // defmodule
        let module_query = r#"
            (call
                target: (identifier) @kw
                (arguments (alias) @module.name)
                (#eq? @kw "defmodule"))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, module_query) {
            for name in captures.remove("module.name").unwrap_or_default() {
                let id = make_id(&[&file_id, &name]);
                fragment.nodes.push(Node {
                    id: id.clone(),
                    label: name,
                    file_type: "module".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
                add_contains_edge(&mut fragment, &file_id, id, path);
            }
        }

        // def/defp functions
        let func_query = r#"
            (call
                target: (identifier) @kw
                (arguments (call target: (identifier) @func.name))
                (#match? @kw "^defp?$"))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, func_query) {
            for name in captures.remove("func.name").unwrap_or_default() {
                let label = format!("{}()", name);
                let id = make_id(&[&file_id, &name]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: id.clone(),
                        label,
                        file_type: "function".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    },
                );
                add_contains_edge(&mut fragment, &file_id, id, path);
            }
        }

        // alias / import / require / use → import edges with context="import"
        let import_query = r#"
            (call
                target: (identifier) @kw
                (arguments (alias) @import.name)
                (#match? @kw "^(alias|import|require|use)$"))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, import_query) {
            for name in captures.remove("import.name").unwrap_or_default() {
                let mod_id = make_id(&[&name]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: mod_id.clone(),
                        label: name.clone(),
                        file_type: "module".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    },
                );
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: mod_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: Some("import".to_string()),
                });
            }
        }

        // Call extraction: find non-keyword calls inside function bodies
        Self::extract_calls(source, path, &file_id, &lang, &mut fragment);

        Ok(fragment)
    }

    fn extract_calls(
        source: &[u8],
        path: &Path,
        file_id: &str,
        lang: &tree_sitter::Language,
        fragment: &mut ExtractionFragment,
    ) {
        let mut parser = Parser::new();
        if parser.set_language(lang).is_err() {
            return;
        }
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return,
        };
        let root = tree.root_node();

        let label_to_id: HashMap<String, String> = fragment
            .nodes
            .iter()
            .filter(|n| n.file_type == "function")
            .map(|n| {
                let label = n.label.trim_end_matches("()").to_string();
                (label, n.id.clone())
            })
            .collect();

        let mut seen: std::collections::HashSet<(String, String)> = Default::default();
        Self::walk_elixir_calls(
            root,
            source,
            path,
            file_id,
            fragment,
            None,
            &label_to_id,
            &mut seen,
        );
    }

    #[allow(clippy::only_used_in_recursion, clippy::too_many_arguments)]
    fn walk_elixir_calls(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        file_id: &str,
        fragment: &mut ExtractionFragment,
        caller_id: Option<&str>,
        label_to_id: &HashMap<String, String>,
        seen: &mut std::collections::HashSet<(String, String)>,
    ) {
        if node.kind() != "call" {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    Self::walk_elixir_calls(
                        child,
                        source,
                        path,
                        file_id,
                        fragment,
                        caller_id,
                        label_to_id,
                        seen,
                    );
                }
            }
            return;
        }

        // Find identifier (keyword)
        let mut kw = None;
        let mut args_node = None;
        let mut do_block = None;
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                match child.kind() {
                    "identifier" if kw.is_none() => {
                        kw = Some(ex_text(source, &child));
                    }
                    "arguments" => args_node = Some(child),
                    "do_block" => do_block = Some(child),
                    _ => {}
                }
            }
        }

        let kw_str = kw.as_deref().unwrap_or("");

        if kw_str == "def" || kw_str == "defp" {
            // Recurse into do_block with this function as the caller
            let func_name = args_node.and_then(|args| {
                // Find call > identifier inside arguments
                for i in 0..args.child_count() {
                    if let Some(child) = args.child(i) {
                        if child.kind() == "call" {
                            for j in 0..child.child_count() {
                                if let Some(id_node) = child.child(j) {
                                    if id_node.kind() == "identifier" {
                                        return Some(ex_text(source, &id_node));
                                    }
                                }
                            }
                        } else if child.kind() == "identifier" {
                            return Some(ex_text(source, &child));
                        }
                    }
                }
                None
            });
            let new_caller = func_name
                .as_ref()
                .and_then(|n| label_to_id.get(n))
                .map(|s| s.as_str());
            let effective_caller = new_caller.or(caller_id);
            if let Some(do_b) = do_block {
                for i in 0..do_b.child_count() {
                    if let Some(child) = do_b.child(i) {
                        Self::walk_elixir_calls(
                            child,
                            source,
                            path,
                            file_id,
                            fragment,
                            effective_caller,
                            label_to_id,
                            seen,
                        );
                    }
                }
            }
            return;
        }

        if kw_str == "defmodule" {
            if let Some(do_b) = do_block {
                for i in 0..do_b.child_count() {
                    if let Some(child) = do_b.child(i) {
                        Self::walk_elixir_calls(
                            child,
                            source,
                            path,
                            file_id,
                            fragment,
                            caller_id,
                            label_to_id,
                            seen,
                        );
                    }
                }
            }
            return;
        }

        // Skip known keywords
        if ELIXIR_KEYWORDS.contains(&kw_str) {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    Self::walk_elixir_calls(
                        child,
                        source,
                        path,
                        file_id,
                        fragment,
                        caller_id,
                        label_to_id,
                        seen,
                    );
                }
            }
            return;
        }

        // This is a non-keyword call
        if let Some(caller) = caller_id {
            if let Some(callee_id) = label_to_id.get(kw_str) {
                let pair = (caller.to_string(), callee_id.clone());
                if !seen.contains(&pair) && caller != callee_id.as_str() {
                    seen.insert(pair);
                    fragment.edges.push(Edge {
                        source: caller.to_string(),
                        target: callee_id.clone(),
                        relation: "calls".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: Some("call".to_string()),
                    });
                }
            }
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                Self::walk_elixir_calls(
                    child,
                    source,
                    path,
                    file_id,
                    fragment,
                    caller_id,
                    label_to_id,
                    seen,
                );
            }
        }
    }
}

impl LanguageExtractor for TsElixirExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["ex", "exs"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
