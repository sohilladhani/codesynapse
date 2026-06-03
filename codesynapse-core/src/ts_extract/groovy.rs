use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_named};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsGroovyExtractor;

impl TsGroovyExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_groovy::LANGUAGE.into();

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
                name: (identifier) @method.name
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

        // tree-sitter-groovy 0.1.2 import_declaration has no named fields; text scan is reliable
        let source_str = std::str::from_utf8(source).unwrap_or("");
        for line in source_str.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("import ") {
                continue;
            }
            let path_part = trimmed[7..].trim().trim_end_matches(';');
            let name = path_part.split('.').next_back().unwrap_or(path_part).trim();
            if name.is_empty() || name == "*" {
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

        Ok(fragment)
    }
}

impl LanguageExtractor for TsGroovyExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["groovy", "gvy", "gy", "gsh"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
