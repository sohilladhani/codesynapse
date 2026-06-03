use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_named};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsPascalExtractor;

impl TsPascalExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_pascal::LANGUAGE.into();

        // procedure Foo; / function Bar: T;  →  declProc > identifier (direct child)
        // Covers both interface declarations and implementation definitions (via defProc > declProc)
        let proc_query = r#"
            (declProc (identifier) @proc.name)
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, proc_query) {
            for name in captures.remove("proc.name").unwrap_or_default() {
                let id = make_id(&[&file_id, &name]);
                fragment.nodes.push(Node {
                    id: id.clone(),
                    label: name,
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

        // uses SysUtils;  →  declUses > moduleName > identifier
        let uses_query = r#"
            (declUses (moduleName (identifier) @import.name))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, uses_query) {
            for name in captures.remove("import.name").unwrap_or_default() {
                let mod_id = make_id(&[&file_id, &name]);
                let mod_node = Node {
                    id: mod_id.clone(),
                    label: name,
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

impl LanguageExtractor for TsPascalExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["pas", "pp", "inc"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
