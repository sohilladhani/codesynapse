use super::{add_node_if_missing, make_file_node};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct JsonPackageExtractor;

impl JsonPackageExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");

        // Parse JSON
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
            if let Some(deps) = parsed.get("dependencies").and_then(|d| d.as_object()) {
                for (dep_name, _version) in deps {
                    let dep_id = make_id(&[&file_id, dep_name]);
                    let dep_node = Node {
                        id: dep_id.clone(),
                        label: dep_name.to_string(),
                        file_type: "config".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    };
                    add_node_if_missing(&mut fragment, dep_node);
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: dep_id,
                        relation: "depends_on".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                }
            }
            if let Some(dev_deps) = parsed.get("devDependencies").and_then(|d| d.as_object()) {
                for (dep_name, _version) in dev_deps {
                    let dep_id = make_id(&[&file_id, dep_name]);
                    let dep_node = Node {
                        id: dep_id.clone(),
                        label: dep_name.to_string(),
                        file_type: "config".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    };
                    add_node_if_missing(&mut fragment, dep_node);
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: dep_id,
                        relation: "dev_depends_on".to_string(),
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

/// MCP config extractor (JSON-based)
impl LanguageExtractor for JsonPackageExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["json"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
