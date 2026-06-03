use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_named};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsScalaExtractor;

impl TsScalaExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_scala::LANGUAGE.into();

        let class_query = r#"
            (class_definition
                name: (identifier) @class.name
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, class_query) {
            for name in captures.remove("class.name").unwrap_or_default() {
                let id = make_id(&[&file_id, &name]);
                fragment.nodes.push(Node {
                    id: id.clone(),
                    label: name,
                    file_type: "code".to_string(),
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

        let object_query = r#"
            (object_definition
                name: (identifier) @object.name
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, object_query) {
            for name in captures.remove("object.name").unwrap_or_default() {
                let id = make_id(&[&file_id, &name]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: id.clone(),
                        label: name,
                        file_type: "code".to_string(),
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

        let method_query = r#"
            (function_definition
                name: (identifier) @method.name
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, method_query) {
            for name in captures.remove("method.name").unwrap_or_default() {
                let label = format!("{}()", name);
                let id = make_id(&[&file_id, &name]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: id.clone(),
                        label,
                        file_type: "method".to_string(),
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

        // tree-sitter-scala 0.26 import_declaration has `path` field with multiple identifiers;
        // text scan is simpler and reliable for extracting the last path segment
        let source_str = std::str::from_utf8(source).unwrap_or("");
        for line in source_str.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("import ") {
                continue;
            }
            let path_part = trimmed[7..].trim();
            let name = path_part.split('.').next_back().unwrap_or(path_part).trim();
            if name.is_empty() || name == "_" || name == "*" || name == "{" {
                continue;
            }
            let name = name.to_string();
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

        // Call extraction
        let call_query = r#"
            (call_expression
                (identifier) @call.name
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, call_query) {
            for name in captures.remove("call.name").unwrap_or_default() {
                let callee_id = make_id(&[&file_id, &name]);
                if fragment.nodes.iter().any(|n| n.id == callee_id) {
                    add_node_if_missing(
                        &mut fragment,
                        Node {
                            id: callee_id.clone(),
                            label: format!("{}()", name),
                            file_type: "function".to_string(),
                            source_file: path.to_string_lossy().to_string(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        },
                    );
                    let already = fragment.edges.iter().any(|e| {
                        e.relation == "calls" && e.source == file_id && e.target == callee_id
                    });
                    if !already {
                        fragment.edges.push(Edge {
                            source: file_id.clone(),
                            target: callee_id,
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

        Ok(fragment)
    }
}

impl LanguageExtractor for TsScalaExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["scala"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
