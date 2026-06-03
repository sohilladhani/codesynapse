use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_named};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node as TsNode, Parser};

pub struct TsJuliaExtractor;

fn jl_text(source: &[u8], node: &TsNode<'_>) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .trim()
        .to_string()
}

impl TsJuliaExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_julia::LANGUAGE.into();

        // Module declarations
        let module_query = r#"
            (module_definition
                (identifier) @module.name)
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

        // Function definitions
        let func_query = r#"
            (function_definition
                (signature
                    (call_expression
                        . (identifier) @func.name)))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, func_query) {
            for name in captures.remove("func.name").unwrap_or_default() {
                let label = format!("{}()", name);
                let id = make_id(&[&file_id, &name]);
                fragment.nodes.push(Node {
                    id: id.clone(),
                    label,
                    file_type: "function".to_string(),
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

        // Short function: perimeter(c::Circle) = expr
        let short_fn_query = r#"
            (assignment
                (call_expression
                    . (identifier) @fn.name))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, short_fn_query) {
            for name in captures.remove("fn.name").unwrap_or_default() {
                let id = make_id(&[&file_id, &name]);
                if !fragment.nodes.iter().any(|n| n.id == id) {
                    let label = format!("{}()", name);
                    fragment.nodes.push(Node {
                        id: id.clone(),
                        label,
                        file_type: "function".to_string(),
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
        }

        // Struct definitions (with optional <: supertype)
        // Walk AST manually to handle binary_expression in type_head
        Self::extract_structs_and_inherits(source, path, &file_id, &lang, &mut fragment);

        // Abstract type definitions
        let abstract_query = r#"
            (abstract_definition
                (type_head
                    (identifier) @abs.name))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, abstract_query) {
            for name in captures.remove("abs.name").unwrap_or_default() {
                let id = make_id(&[&file_id, &name]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: id.clone(),
                        label: name,
                        file_type: "class".to_string(),
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

        // using / import statements with context="import"
        let import_query = r#"
            [
                (import_statement (identifier) @import.name)
                (using_statement (identifier) @import.name)
            ]
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, import_query) {
            for name in captures.remove("import.name").unwrap_or_default() {
                let mod_id = make_id(&[&file_id, &name]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: mod_id.clone(),
                        label: name,
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

        // Call extraction: find call_expression nodes inside function bodies
        Self::extract_calls(source, path, &file_id, &lang, &mut fragment);

        Ok(fragment)
    }

    fn extract_structs_and_inherits(
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
        Self::walk_structs(root, source, path, file_id, file_id, fragment);
    }

    fn walk_structs(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        file_id: &str,
        stem: &str,
        fragment: &mut ExtractionFragment,
    ) {
        if node.kind() == "struct_definition" {
            // type_head may contain: identifier (simple) or binary_expression (Foo <: Bar)
            for i in 0..node.child_count() {
                if let Some(type_head) = node.child(i) {
                    if type_head.kind() == "type_head" {
                        // Check for binary_expression (Foo <: Bar)
                        let mut bin_expr = None;
                        for j in 0..type_head.child_count() {
                            if let Some(c) = type_head.child(j) {
                                if c.kind() == "binary_expression" {
                                    bin_expr = Some(c);
                                }
                            }
                        }
                        if let Some(bin) = bin_expr {
                            // First identifier = struct name, last = supertype
                            let identifiers: Vec<TsNode<'_>> = (0..bin.child_count())
                                .filter_map(|k| bin.child(k))
                                .filter(|c| c.kind() == "identifier")
                                .collect();
                            if !identifiers.is_empty() {
                                let struct_name = jl_text(source, &identifiers[0]);
                                let struct_id = make_id(&[stem, &struct_name]);
                                add_node_if_missing(
                                    fragment,
                                    Node {
                                        id: struct_id.clone(),
                                        label: struct_name.clone(),
                                        file_type: "struct".to_string(),
                                        source_file: path.to_string_lossy().to_string(),
                                        source_location: None,
                                        community: None,
                                        rationale: None,
                                        docstring: None,
                                        metadata: HashMap::new(),
                                    },
                                );
                                fragment.edges.push(Edge {
                                    source: file_id.to_string(),
                                    target: struct_id.clone(),
                                    relation: "contains".to_string(),
                                    confidence: "EXTRACTED".to_string(),
                                    source_file: Some(path.to_string_lossy().to_string()),
                                    weight: 1.0,
                                    context: None,
                                });
                                if identifiers.len() >= 2 {
                                    let super_name =
                                        jl_text(source, &identifiers[identifiers.len() - 1]);
                                    let super_id = make_id(&[stem, &super_name]);
                                    add_node_if_missing(
                                        fragment,
                                        Node {
                                            id: super_id.clone(),
                                            label: super_name,
                                            file_type: "code".to_string(),
                                            source_file: path.to_string_lossy().to_string(),
                                            source_location: None,
                                            community: None,
                                            rationale: None,
                                            docstring: None,
                                            metadata: HashMap::new(),
                                        },
                                    );
                                    fragment.edges.push(Edge {
                                        source: struct_id,
                                        target: super_id,
                                        relation: "inherits".to_string(),
                                        confidence: "EXTRACTED".to_string(),
                                        source_file: Some(path.to_string_lossy().to_string()),
                                        weight: 1.0,
                                        context: None,
                                    });
                                }
                            }
                        } else {
                            // Simple struct, no supertype
                            for j in 0..type_head.child_count() {
                                if let Some(id_node) = type_head.child(j) {
                                    if id_node.kind() == "identifier" {
                                        let name = jl_text(source, &id_node);
                                        let nid = make_id(&[stem, &name]);
                                        add_node_if_missing(
                                            fragment,
                                            Node {
                                                id: nid.clone(),
                                                label: name,
                                                file_type: "struct".to_string(),
                                                source_file: path.to_string_lossy().to_string(),
                                                source_location: None,
                                                community: None,
                                                rationale: None,
                                                docstring: None,
                                                metadata: HashMap::new(),
                                            },
                                        );
                                        fragment.edges.push(Edge {
                                            source: file_id.to_string(),
                                            target: nid,
                                            relation: "contains".to_string(),
                                            confidence: "EXTRACTED".to_string(),
                                            source_file: Some(path.to_string_lossy().to_string()),
                                            weight: 1.0,
                                            context: None,
                                        });
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            return;
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                Self::walk_structs(child, source, path, file_id, stem, fragment);
            }
        }
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

        // Build label→id map for known functions
        let label_to_id: HashMap<String, String> = fragment
            .nodes
            .iter()
            .filter(|n| n.file_type == "function")
            .map(|n| {
                let label = n.label.trim_end_matches("()").to_string();
                (label, n.id.clone())
            })
            .collect();

        let mut seen_pairs: std::collections::HashSet<(String, String)> = Default::default();
        Self::walk_calls_julia(
            root,
            source,
            path,
            file_id,
            file_id,
            fragment,
            None,
            &label_to_id,
            &mut seen_pairs,
        );
    }

    #[allow(clippy::only_used_in_recursion, clippy::too_many_arguments)]
    fn walk_calls_julia(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        file_id: &str,
        stem: &str,
        fragment: &mut ExtractionFragment,
        caller_id: Option<&str>,
        label_to_id: &HashMap<String, String>,
        seen: &mut std::collections::HashSet<(String, String)>,
    ) {
        match node.kind() {
            "function_definition" | "assignment" => {
                // Find the function name
                let func_id = Self::get_julia_func_id(node, source, stem);
                let effective_caller = func_id.as_deref().or(caller_id);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        Self::walk_calls_julia(
                            child,
                            source,
                            path,
                            file_id,
                            stem,
                            fragment,
                            effective_caller,
                            label_to_id,
                            seen,
                        );
                    }
                }
            }
            "call_expression" => {
                if let Some(caller) = caller_id {
                    if let Some(first_child) = node.child(0) {
                        if first_child.kind() == "identifier" {
                            let callee_name = jl_text(source, &first_child);
                            if let Some(callee_id) = label_to_id.get(&callee_name) {
                                let pair = (caller.to_string(), callee_id.clone());
                                if !seen.contains(&pair) && caller != callee_id.as_str() {
                                    seen.insert(pair.clone());
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
                    }
                }
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        Self::walk_calls_julia(
                            child,
                            source,
                            path,
                            file_id,
                            stem,
                            fragment,
                            caller_id,
                            label_to_id,
                            seen,
                        );
                    }
                }
            }
            _ => {
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        Self::walk_calls_julia(
                            child,
                            source,
                            path,
                            file_id,
                            stem,
                            fragment,
                            caller_id,
                            label_to_id,
                            seen,
                        );
                    }
                }
            }
        }
    }

    fn get_julia_func_id(node: TsNode<'_>, source: &[u8], stem: &str) -> Option<String> {
        if node.kind() == "function_definition" {
            for i in 0..node.child_count() {
                if let Some(sig) = node.child(i) {
                    if sig.kind() == "signature" {
                        for j in 0..sig.child_count() {
                            if let Some(call_expr) = sig.child(j) {
                                if call_expr.kind() == "call_expression" {
                                    if let Some(name_node) = call_expr.child(0) {
                                        if name_node.kind() == "identifier" {
                                            let name = jl_text(source, &name_node);
                                            return Some(make_id(&[stem, &name]));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else if node.kind() == "assignment" {
            if let Some(lhs) = node.child(0) {
                if lhs.kind() == "call_expression" {
                    if let Some(name_node) = lhs.child(0) {
                        if name_node.kind() == "identifier" {
                            let name = jl_text(source, &name_node);
                            return Some(make_id(&[stem, &name]));
                        }
                    }
                }
            }
        }
        None
    }
}

impl LanguageExtractor for TsJuliaExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["jl"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
