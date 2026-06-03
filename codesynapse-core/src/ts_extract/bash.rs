use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_named};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsBashExtractor;

impl TsBashExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_bash::LANGUAGE.into();
        let content = std::str::from_utf8(source).unwrap_or("");

        // Function definitions
        let fn_query = r#"
            (function_definition
                name: (word) @func.name
                body: (_)
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, fn_query) {
            let names = captures.remove("func.name").unwrap_or_default();
            for name in &names {
                let fn_id = make_id(&[&file_id, name, "()"]);
                fragment.nodes.push(Node {
                    id: fn_id.clone(),
                    label: format!("{}()", name),
                    file_type: "function".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        // Source/import commands
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("source ") || trimmed.starts_with(". ") {
                let target = trimmed
                    .strip_prefix("source ")
                    .or_else(|| trimmed.strip_prefix(". "))
                    .unwrap_or("")
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');
                if !target.is_empty() {
                    let source_id = make_id(&[&file_id, target]);
                    let source_node = Node {
                        id: source_id.clone(),
                        label: target.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    };
                    add_node_if_missing(&mut fragment, source_node);
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: source_id,
                        relation: "imports_from".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                }
            }
        }

        Ok(fragment)
    }
}

/// Vue SFC extractor (script extraction + template analysis)
impl LanguageExtractor for TsBashExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["sh", "bash"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
