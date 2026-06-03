use super::javascript::TsJavaScriptExtractor;
use super::{
    add_contains_edge, add_node_if_missing, make_file_node, run_query_matches_ranged,
    run_query_named,
};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsTypeScriptExtractor;

impl TsTypeScriptExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let mut fragment = TsJavaScriptExtractor::extract(source, path)?;
        let (file_id, _, _) = make_file_node(path);
        let lang = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();

        // Interface declarations
        let iface_query = r#"
            (interface_declaration
                name: (type_identifier) @iface.name
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, iface_query) {
            let iface_names = captures.remove("iface.name").unwrap_or_default();
            for name in &iface_names {
                let iface_id = make_id(&[&file_id, name]);
                if !fragment.nodes.iter().any(|n| n.id == iface_id) {
                    fragment.nodes.push(Node {
                        id: iface_id.clone(),
                        label: name.to_string(),
                        file_type: "interface".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: {
                            let mut m = HashMap::new();
                            m.insert("kind".to_string(), "interface".to_string());
                            m
                        },
                    });
                    add_contains_edge(&mut fragment, &file_id, iface_id, path);
                }
            }
        }

        // Type aliases
        let alias_query = r#"
            (type_alias_declaration
                name: (type_identifier) @alias.name
                value: [
                    (type_identifier) @alias.ref
                    (generic_type
                        (type_identifier) @alias.ref
                    )
                ]
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, alias_query) {
            let alias_names = captures.remove("alias.name").unwrap_or_default();
            let alias_refs = captures.remove("alias.ref").unwrap_or_default();

            for name in &alias_names {
                let alias_id = make_id(&[&file_id, name]);
                if !fragment.nodes.iter().any(|n| n.id == alias_id) {
                    fragment.nodes.push(Node {
                        id: alias_id.clone(),
                        label: name.to_string(),
                        file_type: "class".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: {
                            let mut m = HashMap::new();
                            m.insert("kind".to_string(), "type_alias".to_string());
                            m
                        },
                    });
                    add_contains_edge(&mut fragment, &file_id, alias_id.clone(), path);
                }

                // Find matching type reference for this alias
                for ref_name in &alias_refs {
                    let ref_id = make_id(&[ref_name]);
                    if !fragment.nodes.iter().any(|n| n.id == ref_id) {
                        fragment.nodes.push(Node {
                            id: ref_id.clone(),
                            label: ref_name.to_string(),
                            file_type: "code".to_string(),
                            source_file: path.to_string_lossy().to_string(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        });
                    }
                    fragment.edges.push(Edge {
                        source: alias_id.clone(),
                        target: ref_id,
                        relation: "type_ref".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                }
            }
        }

        // Named function declarations (TS parser handles TS-specific syntax)
        let fn_query = r#"
            (function_declaration
                name: (identifier) @func.name
            ) @func.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, fn_query) {
            for (m, ranges) in &matches {
                let name = match m.get("func.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let fn_id = make_id(&[&file_id, &name, "()"]);
                let source_location = ranges
                    .get("func.node")
                    .map(|(start, end)| format!("{}:{}", start, end));
                let fn_node = Node {
                    id: fn_id.clone(),
                    label: format!("{}()", name),
                    file_type: "function".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, fn_node);
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        // Class method definitions
        let method_query = r#"
            (method_definition
                name: (property_identifier) @method.name
            ) @method.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, method_query) {
            for (m, ranges) in &matches {
                let name = match m.get("method.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let method_id = make_id(&[&file_id, &name, "()"]);
                let source_location = ranges
                    .get("method.node")
                    .map(|(start, end)| format!("{}:{}", start, end));
                let method_node = Node {
                    id: method_id.clone(),
                    label: format!("{}()", name),
                    file_type: "method".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, method_node);
                add_contains_edge(&mut fragment, &file_id, method_id, path);
            }
        }

        // Arrow functions and function expressions assigned to variables
        let arrow_query = r#"
            (variable_declarator
                name: (identifier) @func.name
                value: [
                    (arrow_function)
                    (function_expression)
                ] @func.node
            )
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, arrow_query) {
            for (m, ranges) in &matches {
                let name = match m.get("func.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let fn_id = make_id(&[&file_id, &name, "()"]);
                let source_location = ranges
                    .get("func.node")
                    .map(|(start, end)| format!("{}:{}", start, end));
                let fn_node = Node {
                    id: fn_id.clone(),
                    label: format!("{}()", name),
                    file_type: "function".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, fn_node);
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        Ok(fragment)
    }
}

/// Tree-sitter based Go extractor
impl LanguageExtractor for TsTypeScriptExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["ts", "tsx", "mts", "cts"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
