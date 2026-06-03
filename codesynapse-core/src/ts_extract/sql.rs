use super::{add_contains_edge, add_node_if_missing, make_file_node};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsSqlExtractor;

impl TsSqlExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");
        for line in content.lines() {
            let upper = line.to_uppercase();
            let trimmed = line.trim();

            if upper.contains("CREATE TABLE") {
                let after = trimmed
                    .strip_prefix("CREATE TABLE")
                    .or_else(|| trimmed.strip_prefix("create table"))
                    .or_else(|| trimmed.strip_prefix("create table"))
                    .unwrap_or("")
                    .trim();
                let table_name = after
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches('(');
                let table_id = make_id(&[&file_id, table_name]);
                fragment.nodes.push(Node {
                    id: table_id.clone(),
                    label: table_name.to_string(),
                    file_type: "data".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("kind".to_string(), "table".to_string());
                        m
                    },
                });
                add_contains_edge(&mut fragment, &file_id, table_id, path);
            }

            // FOREIGN KEY references
            if upper.contains("FOREIGN KEY") && upper.contains("REFERENCES") {
                if let Some(ref_part) = trimmed.to_lowercase().split("references").nth(1) {
                    let target = ref_part
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim_matches('(');
                    if !target.is_empty() {
                        let ref_id = make_id(&[&file_id, target]);
                        let ref_node = Node {
                            id: ref_id.clone(),
                            label: target.to_string(),
                            file_type: "data".to_string(),
                            source_file: path.to_string_lossy().to_string(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        };
                        add_node_if_missing(&mut fragment, ref_node);
                        if !fragment
                            .edges
                            .iter()
                            .any(|e| e.target == ref_id && e.relation == "references")
                        {
                            fragment.edges.push(Edge {
                                source: file_id.clone(),
                                target: ref_id.clone(),
                                relation: "references".to_string(),
                                confidence: "EXTRACTED".to_string(),
                                source_file: Some(path.to_string_lossy().to_string()),
                                weight: 1.0,
                                context: None,
                            });
                        }
                    }
                }
            }
        }

        Ok(fragment)
    }
}

/// Tree-sitter based Bash extractor
impl LanguageExtractor for TsSqlExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["sql"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
