use super::make_file_node;
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct MarkdownExtractor;

impl MarkdownExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");
        let str_path = path.to_string_lossy().to_string();
        let stem = file_id.clone();

        let mut heading_stack: Vec<(usize, String)> = Vec::new();
        let mut in_code_block = false;
        let mut code_block_lang: Option<String> = None;
        let mut code_block_start = 0usize;
        let mut code_block_first_line: Option<String> = None;
        let mut code_block_count = 0usize;

        for (line_num_0, line_text) in content.lines().enumerate() {
            let line_num = line_num_0 + 1;
            let stripped = line_text.trim();

            if let Some(fence_rest) = stripped.strip_prefix("```") {
                if !in_code_block {
                    in_code_block = true;
                    let after = fence_rest.trim();
                    code_block_lang = if !after.is_empty() {
                        after.split_whitespace().next().map(str::to_string)
                    } else {
                        None
                    };
                    code_block_start = line_num;
                    code_block_first_line = None;
                } else {
                    in_code_block = false;
                    code_block_count += 1;
                    let label = match (&code_block_lang, &code_block_first_line) {
                        (Some(lang), Some(first)) => {
                            let preview: String = first.chars().take(60).collect();
                            format!("code:{} ({})", lang, preview)
                        }
                        (Some(lang), None) => format!("code:{}", lang),
                        _ => format!("code:block{}", code_block_count),
                    };
                    let cb_nid = make_id(&[&stem, &format!("codeblock_{}", code_block_count)]);
                    fragment.nodes.push(Node {
                        id: cb_nid.clone(),
                        label,
                        file_type: "document".to_string(),
                        source_file: str_path.clone(),
                        source_location: Some(format!("L{}", code_block_start)),
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                    let parent = heading_stack
                        .last()
                        .map(|(_, nid)| nid.clone())
                        .unwrap_or_else(|| file_id.clone());
                    fragment.edges.push(Edge {
                        source: parent,
                        target: cb_nid,
                        relation: "contains".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(str_path.clone()),
                        weight: 1.0,
                        context: None,
                    });
                }
                continue;
            }

            if in_code_block {
                if code_block_first_line.is_none() && !stripped.is_empty() {
                    code_block_first_line = Some(stripped.to_string());
                }
                continue;
            }

            // ATX headings: ^(#{1,6})\s+(.+)
            let hash_count = line_text.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&hash_count) {
                let after = &line_text[hash_count..];
                if after.starts_with(' ') || after.starts_with('\t') {
                    let title = after.trim();
                    if !title.is_empty() {
                        let h_nid_base = make_id(&[&stem, title]);
                        let h_nid = if fragment.nodes.iter().any(|n| n.id == h_nid_base) {
                            make_id(&[&stem, title, &line_num.to_string()])
                        } else {
                            h_nid_base
                        };
                        fragment.nodes.push(Node {
                            id: h_nid.clone(),
                            label: title.to_string(),
                            file_type: "document".to_string(),
                            source_file: str_path.clone(),
                            source_location: Some(format!("L{}", line_num)),
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        });
                        while heading_stack
                            .last()
                            .map(|(l, _)| *l >= hash_count)
                            .unwrap_or(false)
                        {
                            heading_stack.pop();
                        }
                        let parent = heading_stack
                            .last()
                            .map(|(_, nid)| nid.clone())
                            .unwrap_or_else(|| file_id.clone());
                        fragment.edges.push(Edge {
                            source: parent,
                            target: h_nid.clone(),
                            relation: "contains".to_string(),
                            confidence: "EXTRACTED".to_string(),
                            source_file: Some(str_path.clone()),
                            weight: 1.0,
                            context: None,
                        });
                        heading_stack.push((hash_count, h_nid));
                    }
                }
            }
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for MarkdownExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["md", "mdx"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
