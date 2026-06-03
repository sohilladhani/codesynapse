use super::{add_contains_edge, add_node_if_missing, make_file_node};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct McpConfigExtractor;

impl McpConfigExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");

        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
            if let Some(servers) = parsed.get("servers").and_then(|s| s.as_array()) {
                for server in servers {
                    if let Some(name) = server.get("name").and_then(|n| n.as_str()) {
                        let server_id = make_id(&[&file_id, name]);
                        fragment.nodes.push(Node {
                            id: server_id.clone(),
                            label: name.to_string(),
                            file_type: "config".to_string(),
                            source_file: path.to_string_lossy().to_string(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        });
                        add_contains_edge(&mut fragment, &file_id, server_id.clone(), path);

                        if let Some(env) = server.get("env").and_then(|e| e.as_object()) {
                            for (env_key, _env_val) in env {
                                let env_id = make_id(&[&server_id, env_key]);
                                let env_node = Node {
                                    id: env_id.clone(),
                                    label: format!("env:{}", env_key),
                                    file_type: "config".to_string(),
                                    source_file: path.to_string_lossy().to_string(),
                                    source_location: None,
                                    community: None,
                                    rationale: None,
                                    docstring: None,
                                    metadata: {
                                        let mut m = HashMap::new();
                                        m.insert("kind".to_string(), "env_var".to_string());
                                        m
                                    },
                                };
                                add_node_if_missing(&mut fragment, env_node);
                                fragment.edges.push(Edge {
                                    source: server_id.clone(),
                                    target: env_id,
                                    relation: "requires".to_string(),
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
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for McpConfigExtractor {
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
