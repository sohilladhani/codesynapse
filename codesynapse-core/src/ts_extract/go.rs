use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_matches_ranged};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsGoExtractor;

impl TsGoExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_go::LANGUAGE.into();

        // Struct definitions with source_location
        let struct_query = r#"
            (type_declaration
                (type_spec
                    name: (type_identifier) @struct.name
                    type: (struct_type)
                )
            ) @struct.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, struct_query) {
            for (texts, ranges) in &matches {
                let name = match texts.get("struct.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let source_location = ranges
                    .get("struct.node")
                    .map(|(s, e)| format!("{}:{}", s, e));
                let struct_id = make_id(&[&file_id, &name]);
                fragment.nodes.push(Node {
                    id: struct_id.clone(),
                    label: name.clone(),
                    file_type: "struct".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("kind".to_string(), "struct".to_string());
                        m
                    },
                });
                add_contains_edge(&mut fragment, &file_id, struct_id, path);
            }
        }

        // Top-level functions
        let fn_query = r#"
            (function_declaration
                name: (identifier) @fn.name
            ) @fn.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, fn_query) {
            for (texts, ranges) in &matches {
                let name = match texts.get("fn.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let source_location = ranges.get("fn.node").map(|(s, e)| format!("{}:{}", s, e));
                let fn_id = make_id(&[&file_id, &name]);
                let label = format!("{}()", name);
                fragment.nodes.push(Node {
                    id: fn_id.clone(),
                    label,
                    file_type: "function".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("kind".to_string(), "function".to_string());
                        m
                    },
                });
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        // Pointer receiver methods: func (e *Engine) ServeHTTP(...)
        let ptr_recv_query = r#"
            (method_declaration
                receiver: (parameter_list
                    (parameter_declaration
                        type: (pointer_type (type_identifier) @recv.type)
                    )
                )
                name: (field_identifier) @fn.name
            ) @fn.node
        "#;
        Self::extract_methods(source, &lang, path, &file_id, &mut fragment, ptr_recv_query);

        // Value receiver methods: func (e Engine) ServeHTTP(...)
        let val_recv_query = r#"
            (method_declaration
                receiver: (parameter_list
                    (parameter_declaration
                        type: (type_identifier) @recv.type
                    )
                )
                name: (field_identifier) @fn.name
            ) @fn.node
        "#;
        Self::extract_methods(source, &lang, path, &file_id, &mut fragment, val_recv_query);

        Ok(fragment)
    }

    fn extract_methods(
        source: &[u8],
        lang: &tree_sitter::Language,
        path: &Path,
        file_id: &String,
        fragment: &mut ExtractionFragment,
        query_str: &str,
    ) {
        let Ok(matches) = run_query_matches_ranged(source, lang, query_str) else {
            return;
        };

        // Build set of struct IDs already in fragment for contains edge guard
        let existing_struct_ids: std::collections::HashSet<String> =
            fragment.nodes.iter().map(|n| n.id.clone()).collect();

        for (texts, ranges) in &matches {
            let fn_name = match texts.get("fn.name") {
                Some(n) => n.clone(),
                None => continue,
            };
            let recv_type = match texts.get("recv.type") {
                Some(t) => t.clone(),
                None => continue,
            };
            let source_location = ranges.get("fn.node").map(|(s, e)| format!("{}:{}", s, e));
            let method_id = make_id(&[file_id, &recv_type, &fn_name]);
            let label = format!("{}()", fn_name);

            let method_node = Node {
                id: method_id.clone(),
                label,
                file_type: "method".to_string(),
                source_file: path.to_string_lossy().to_string(),
                source_location,
                community: None,
                rationale: None,
                docstring: None,
                metadata: {
                    let mut m = HashMap::new();
                    m.insert("kind".to_string(), "method".to_string());
                    m
                },
            };
            add_node_if_missing(fragment, method_node);

            // file → method contains edge
            add_contains_edge(fragment, file_id, method_id.clone(), path);

            // struct → method contains edge if struct exists in fragment
            let struct_id = make_id(&[file_id, &recv_type]);
            if existing_struct_ids.contains(&struct_id) {
                fragment.edges.push(Edge {
                    source: struct_id,
                    target: method_id,
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }
    }
}

/// Tree-sitter based Go extractor
impl LanguageExtractor for TsGoExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["go"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
