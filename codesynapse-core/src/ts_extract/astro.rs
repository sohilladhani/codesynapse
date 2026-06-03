use super::make_file_node;
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct AstroExtractor;

impl AstroExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");
        let str_path = path.to_string_lossy().to_string();

        let mut regions: Vec<String> = Vec::new();

        // Extract frontmatter block: ---\n...\n---
        let trimmed = content.trim_start_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("---") {
            if let Some(end) = rest.find("\n---") {
                regions.push(rest[..end].to_string());
            }
        }

        // Extract <script> blocks
        let content_lower = content.to_ascii_lowercase();
        let mut search_start = 0usize;
        while let Some(rel_open) = content_lower[search_start..].find("<script") {
            let abs_open = search_start + rel_open;
            if let Some(rel_tag_end) = content_lower[abs_open..].find('>') {
                let content_start = abs_open + rel_tag_end + 1;
                if let Some(rel_close) = content_lower[content_start..].find("</script") {
                    let abs_close = content_start + rel_close;
                    regions.push(content[content_start..abs_close].to_string());
                    search_start = abs_close + 8;
                    continue;
                }
            }
            break;
        }

        // Regex patterns (compiled once per call — acceptable for extraction workloads)
        let static_re = regex::Regex::new(r#"import\s+(?:[^'"`;\n]+?\s+from\s+)?['"]([^'"]+)['"]"#)
            .expect("static import regex");
        let dynamic_re = regex::Regex::new(r#"import\(\s*['"]([^'"]+)['"]\s*\)?"#)
            .expect("dynamic import regex");

        let mut seen: HashSet<String> = HashSet::new();

        let add_import = |fragment: &mut ExtractionFragment,
                          seen: &mut HashSet<String>,
                          raw: &str,
                          relation: &str| {
            if raw.is_empty() {
                return;
            }
            let module = raw.rsplit('/').next().unwrap_or(raw);
            if module.is_empty() {
                return;
            }
            let node_id = make_id(&[module]);
            let is_new = seen.insert(node_id.clone());
            if is_new {
                fragment.nodes.push(Node {
                    id: node_id.clone(),
                    label: raw.to_string(),
                    file_type: "module".to_string(),
                    source_file: str_path.clone(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
            }
            fragment.edges.push(Edge {
                source: file_id.clone(),
                target: node_id,
                relation: relation.to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(str_path.clone()),
                weight: 1.0,
                context: None,
            });
        };

        for region in &regions {
            for cap in static_re.captures_iter(region) {
                if let Some(m) = cap.get(1) {
                    add_import(&mut fragment, &mut seen, m.as_str(), "imports_from");
                }
            }
            for cap in dynamic_re.captures_iter(region) {
                if let Some(m) = cap.get(1) {
                    add_import(&mut fragment, &mut seen, m.as_str(), "dynamic_import");
                }
            }
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for AstroExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["astro"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
