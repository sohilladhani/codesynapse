use super::{add_contains_edge, add_node_if_missing, make_file_node};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsRacketExtractor;

impl TsRacketExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");
        for line in content.lines() {
            let trimmed = line.trim();

            if let Some(def) = trimmed.strip_prefix("(define ") {
                let name = def
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(')');
                if !name.is_empty() && !name.starts_with('(') {
                    let label = format!("{}()", name);
                    let id = make_id(&[&file_id, name]);
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

            if let Some(require) = trimmed.strip_prefix("(require ") {
                let rest = require.trim_end_matches(')');
                for part in rest.split_whitespace() {
                    let cleaned = part.trim().trim_matches('"');
                    if !cleaned.is_empty() && !cleaned.starts_with('(') {
                        let mod_id = make_id(&[&file_id, cleaned]);
                        let mod_node = Node {
                            id: mod_id.clone(),
                            label: cleaned.to_string(),
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
            }

            if let Some(provide) = trimmed.strip_prefix("(provide ") {
                let rest = provide.trim_end_matches(')');
                for part in rest.split_whitespace() {
                    let cleaned = part.trim();
                    if !cleaned.is_empty() && !cleaned.starts_with('(') && !cleaned.starts_with('"')
                    {
                        let id = make_id(&[&file_id, cleaned]);
                        fragment.nodes.push(Node {
                            id: id.clone(),
                            label: cleaned.to_string(),
                            file_type: "export".to_string(),
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
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for TsRacketExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["rkt", "scrbl", "ss"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
