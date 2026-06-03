use super::make_file_node;
use crate::error::{CodeSynapseError, Result};
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node as TsNode, Parser};

pub struct PowerShellExtractor;

fn ps_text(source: &[u8], node: &TsNode<'_>) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .to_string()
}

fn ps_node(id: String, label: String, file_type: &str, path: &Path) -> Node {
    Node {
        id,
        label,
        file_type: file_type.to_string(),
        source_file: path.to_string_lossy().to_string(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    }
}

fn ps_edge(fragment: &mut ExtractionFragment, src: &str, tgt: &str, rel: &str, path: &Path) {
    fragment.edges.push(Edge {
        source: src.to_string(),
        target: tgt.to_string(),
        relation: rel.to_string(),
        confidence: "EXTRACTED".to_string(),
        source_file: Some(path.to_string_lossy().to_string()),
        weight: 1.0,
        context: None,
    });
}

fn walk_ps<'t>(
    node: TsNode<'t>,
    source: &[u8],
    file_id: &str,
    stem: &str,
    path: &Path,
    fragment: &mut ExtractionFragment,
    parent_class: Option<String>,
) {
    match node.kind() {
        "function_statement" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "function_name" {
                        let name = ps_text(source, &child);
                        if !name.is_empty() {
                            let id = make_id(&[stem, &name]);
                            fragment.nodes.push(ps_node(
                                id.clone(),
                                format!("{}()", name),
                                "function",
                                path,
                            ));
                            ps_edge(fragment, file_id, &id, "contains", path);
                        }
                        break;
                    }
                }
            }
        }
        "class_statement" => {
            let mut class_nid: Option<String> = None;
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "simple_name" {
                        let name = ps_text(source, &child);
                        if !name.is_empty() {
                            let id = make_id(&[stem, &name]);
                            fragment
                                .nodes
                                .push(ps_node(id.clone(), name, "class", path));
                            ps_edge(fragment, file_id, &id, "contains", path);
                            class_nid = Some(id);
                            break;
                        }
                    }
                }
            }
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    walk_ps(
                        child,
                        source,
                        file_id,
                        stem,
                        path,
                        fragment,
                        class_nid.clone(),
                    );
                }
            }
        }
        "class_method_definition" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "simple_name" {
                        let name = ps_text(source, &child);
                        if !name.is_empty() {
                            let parent = parent_class.as_deref().unwrap_or(file_id);
                            let id = make_id(&[parent, &name]);
                            fragment.nodes.push(ps_node(
                                id.clone(),
                                format!(".{}()", name),
                                "function",
                                path,
                            ));
                            ps_edge(fragment, parent, &id, "method", path);
                        }
                        break;
                    }
                }
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    walk_ps(
                        child,
                        source,
                        file_id,
                        stem,
                        path,
                        fragment,
                        parent_class.clone(),
                    );
                }
            }
        }
    }
}

impl PowerShellExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let stem = file_id.clone();
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang: tree_sitter::Language = tree_sitter_powershell::LANGUAGE.into();
        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| CodeSynapseError::Parse(format!("ps1 set_language: {e}")))?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| CodeSynapseError::Parse("ps1 parse failed".to_string()))?;

        walk_ps(
            tree.root_node(),
            source,
            &file_id,
            &stem,
            path,
            &mut fragment,
            None,
        );

        Ok(fragment)
    }
}

impl LanguageExtractor for PowerShellExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["ps1", "psm1", "psd1"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
