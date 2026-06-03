use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_named};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsDartExtractor;

impl TsDartExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_dart::LANGUAGE.into();

        let class_query = r#"
            (class_declaration
                name: (identifier) @class.name
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, class_query) {
            for name in captures.remove("class.name").unwrap_or_default() {
                let id = make_id(&[&file_id, &name]);
                fragment.nodes.push(Node {
                    id: id.clone(),
                    label: name,
                    file_type: "class".to_string(),
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

        let method_query = r#"
            (method_declaration
                signature: (method_signature
                    (function_signature
                        name: (identifier) @method.name
                    )
                )
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, method_query) {
            for name in captures.remove("method.name").unwrap_or_default() {
                let label = format!("{}()", name);
                let id = make_id(&[&file_id, &name]);
                fragment.nodes.push(Node {
                    id: id.clone(),
                    label,
                    file_type: "method".to_string(),
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

        let import_query = r#"
            (import_specification
                uri: (uri
                    (string_literal) @import.uri))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, import_query) {
            for uri in captures.remove("import.uri").unwrap_or_default() {
                let module = uri.trim_matches('"').trim().trim_matches('\'');
                let mod_id = make_id(&[&file_id, module]);
                let mod_node = Node {
                    id: mod_id.clone(),
                    label: module.to_string(),
                    file_type: "module".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, mod_node);
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: mod_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for TsDartExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["dart"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
