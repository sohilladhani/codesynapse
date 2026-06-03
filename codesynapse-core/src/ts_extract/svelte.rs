use super::{add_contains_edge, make_file_node};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsSvelteExtractor;

impl TsSvelteExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");

        // Extract script section
        let mut script_content = String::new();
        let mut in_script = false;

        for line in content.lines() {
            if line.trim().starts_with("<script") {
                in_script = true;
                continue;
            }
            if line.trim() == "</script>" {
                in_script = false;
                continue;
            }
            if in_script {
                script_content.push_str(line);
                script_content.push('\n');
            }
        }

        // Look for $state, $derived, $effect runes
        for line in script_content.lines() {
            if line.contains("$state") || line.contains("$derived") || line.contains("$effect") {
                if let Some(var_part) = line.split('=').next() {
                    let var_name = var_part.split_whitespace().last().unwrap_or("").trim();
                    if !var_name.is_empty() {
                        let state_id = make_id(&[&file_id, var_name]);
                        fragment.nodes.push(Node {
                            id: state_id.clone(),
                            label: var_name.to_string(),
                            file_type: "constant".to_string(),
                            source_file: path.to_string_lossy().to_string(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        });
                        add_contains_edge(&mut fragment, &file_id, state_id, path);
                    }
                }
            }
        }

        Ok(fragment)
    }
}

/// Package.json extractor (JSON-based)
impl LanguageExtractor for TsSvelteExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["svelte"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
