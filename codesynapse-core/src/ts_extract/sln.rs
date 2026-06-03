use super::make_file_node;
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct SlnExtractor;

impl SlnExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");

        // Parse Project("...") = "Name", "Path", "{GUID}"
        let proj_re = regex::Regex::new(
            r#"Project\("[^"]*"\)\s*=\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*"\{([^}]+)\}""#,
        );
        let proj_re = match proj_re {
            Ok(r) => r,
            Err(_) => return Ok(fragment),
        };

        let mut guid_to_id: HashMap<String, String> = HashMap::new();

        for cap in proj_re.captures_iter(content) {
            let name = cap.get(1).map(|m| m.as_str()).unwrap_or("").trim();
            let proj_path = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim();
            let guid = cap
                .get(3)
                .map(|m| m.as_str())
                .unwrap_or("")
                .trim()
                .to_uppercase();

            if name.is_empty() || guid.is_empty() {
                continue;
            }

            let proj_id = make_id(&[&file_id, name]);
            guid_to_id.insert(guid.clone(), proj_id.clone());

            fragment.nodes.push(Node {
                id: proj_id.clone(),
                label: name.to_string(),
                file_type: "code".to_string(),
                source_file: path.to_string_lossy().to_string(),
                source_location: None,
                community: None,
                rationale: None,
                docstring: None,
                metadata: {
                    let mut m = HashMap::new();
                    m.insert("path".to_string(), proj_path.to_string());
                    m.insert("guid".to_string(), guid.clone());
                    m
                },
            });
            fragment.edges.push(Edge {
                source: file_id.clone(),
                target: proj_id,
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });
        }

        // Parse ProjectDependencies = postProject sections for dependency edges
        let dep_re = regex::Regex::new(r#"\{([A-F0-9\-]+)\}\s*=\s*\{([A-F0-9\-]+)\}"#);
        if let Ok(dep_re) = dep_re {
            let mut in_deps = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.contains("ProjectDependencies") {
                    in_deps = true;
                    continue;
                }
                if trimmed == "EndProjectSection" {
                    in_deps = false;
                    continue;
                }
                if in_deps {
                    if let Some(cap) = dep_re.captures(trimmed) {
                        let dep_guid = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_uppercase();
                        let src_guid = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_uppercase();
                        if let (Some(dep_id), Some(src_id)) =
                            (guid_to_id.get(&dep_guid), guid_to_id.get(&src_guid))
                        {
                            fragment.edges.push(Edge {
                                source: src_id.clone(),
                                target: dep_id.clone(),
                                relation: "imports".to_string(),
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

impl LanguageExtractor for SlnExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["sln"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
