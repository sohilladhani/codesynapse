use super::{add_contains_edge, add_node_if_missing, make_file_node};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsVueExtractor;

impl TsVueExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");

        // Extract script section
        let mut script_content = String::new();
        let mut template_content = String::new();
        let mut in_script = false;
        let mut in_template = false;

        for line in content.lines() {
            if line.trim().starts_with("<script") {
                in_script = true;
                continue;
            }
            if line.trim() == "</script>" {
                in_script = false;
                continue;
            }
            if line.trim().starts_with("<template") {
                in_template = true;
                continue;
            }
            if line.trim() == "</template>" {
                in_template = false;
                continue;
            }
            if in_script {
                script_content.push_str(line);
                script_content.push('\n');
            }
            if in_template {
                template_content.push_str(line);
                template_content.push('\n');
            }
        }

        // Extract component name from script
        if script_content.contains("name:") {
            for line in script_content.lines() {
                if line.contains("name:") {
                    let name_part = line.split("name:").nth(1).unwrap_or("").trim();
                    let comp_name = name_part
                        .trim_matches(',')
                        .trim()
                        .trim_matches('\'')
                        .trim_matches('"')
                        .trim();
                    if !comp_name.is_empty() {
                        let comp_id = make_id(&[&file_id, comp_name]);
                        fragment.nodes.push(Node {
                            id: comp_id.clone(),
                            label: comp_name.to_string(),
                            file_type: "class".to_string(),
                            source_file: path.to_string_lossy().to_string(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: {
                                let mut m = HashMap::new();
                                m.insert("kind".to_string(), "component".to_string());
                                m
                            },
                        });
                        add_contains_edge(&mut fragment, &file_id, comp_id, path);
                    }
                }
            }
        }

        // Track template refs
        for line in template_content.lines() {
            if line.contains("ref=\"") || line.contains("ref='") {
                let ref_name = line
                    .split("ref=\"")
                    .nth(1)
                    .or_else(|| line.split("ref='").nth(1))
                    .and_then(|s| s.split('"').next().or_else(|| s.split('\'').next()))
                    .unwrap_or("")
                    .trim();
                if !ref_name.is_empty() {
                    let ref_id = make_id(&[&file_id, ref_name, "ref"]);
                    let ref_node = Node {
                        id: ref_id.clone(),
                        label: format!("ref:{}", ref_name),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    };
                    add_node_if_missing(&mut fragment, ref_node);
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: ref_id,
                        relation: "contains".to_string(),
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

/// Svelte SFC extractor
impl LanguageExtractor for TsVueExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["vue"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
