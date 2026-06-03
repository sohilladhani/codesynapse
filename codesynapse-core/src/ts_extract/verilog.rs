use super::make_file_node;
use crate::error::{CodeSynapseError, Result};
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node as TsNode, Parser};

pub struct VerilogExtractor;

fn vl_text(source: &[u8], node: &TsNode<'_>) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .trim()
        .to_string()
}

fn vl_node(id: String, label: String, file_type: &str, path: &Path) -> Node {
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

fn vl_edge(fragment: &mut ExtractionFragment, src: &str, tgt: &str, rel: &str, path: &Path) {
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

fn add_node_if_missing(fragment: &mut ExtractionFragment, node: Node) {
    if !fragment.nodes.iter().any(|n| n.id == node.id) {
        fragment.nodes.push(node);
    }
}

fn walk_vl<'t>(
    node: TsNode<'t>,
    source: &[u8],
    file_id: &str,
    stem: &str,
    path: &Path,
    fragment: &mut ExtractionFragment,
    module_nid: Option<String>,
) {
    match node.kind() {
        "module_declaration" => {
            let name_node = node.child_by_field_name("name");
            if let Some(n) = name_node {
                let name = vl_text(source, &n);
                if !name.is_empty() {
                    let nid = make_id(&[stem, &name]);
                    fragment
                        .nodes
                        .push(vl_node(nid.clone(), name, "module", path));
                    vl_edge(fragment, file_id, &nid, "defines", path);
                    for i in 0..node.child_count() {
                        if let Some(child) = node.child(i) {
                            walk_vl(
                                child,
                                source,
                                file_id,
                                stem,
                                path,
                                fragment,
                                Some(nid.clone()),
                            );
                        }
                    }
                    return;
                }
            }
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    walk_vl(
                        child,
                        source,
                        file_id,
                        stem,
                        path,
                        fragment,
                        module_nid.clone(),
                    );
                }
            }
        }
        "function_declaration" | "function_prototype" => {
            let name_node = node.child_by_field_name("name");
            if let Some(n) = name_node {
                let name = vl_text(source, &n);
                if !name.is_empty() {
                    let parent = module_nid.as_deref().unwrap_or(file_id);
                    let nid = make_id(&[parent, &name]);
                    fragment.nodes.push(vl_node(
                        nid.clone(),
                        format!("{}()", name),
                        "function",
                        path,
                    ));
                    vl_edge(fragment, parent, &nid, "contains", path);
                }
            }
        }
        "task_declaration" => {
            let name_node = node.child_by_field_name("name");
            if let Some(n) = name_node {
                let name = vl_text(source, &n);
                if !name.is_empty() {
                    let parent = module_nid.as_deref().unwrap_or(file_id);
                    let nid = make_id(&[parent, &name]);
                    fragment
                        .nodes
                        .push(vl_node(nid.clone(), name, "code", path));
                    vl_edge(fragment, parent, &nid, "contains", path);
                }
            }
        }
        "package_import_declaration" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "package_import_item" {
                        let text = vl_text(source, &child);
                        let pkg_name = text.split("::").next().unwrap_or("").trim().to_string();
                        if !pkg_name.is_empty() {
                            let tgt_nid = make_id(&[&pkg_name]);
                            let tgt_node = vl_node(tgt_nid.clone(), pkg_name, "module", path);
                            add_node_if_missing(fragment, tgt_node);
                            let src = module_nid.as_deref().unwrap_or(file_id);
                            vl_edge(fragment, src, &tgt_nid, "imports_from", path);
                        }
                    }
                }
            }
        }
        "module_instantiation" => {
            let type_node = node.child_by_field_name("module_type");
            if let Some(n) = type_node {
                if let Some(mod_nid) = &module_nid {
                    let inst_type = vl_text(source, &n);
                    if !inst_type.is_empty() {
                        let tgt_nid = make_id(&[&inst_type]);
                        let tgt_node = vl_node(tgt_nid.clone(), inst_type, "module", path);
                        add_node_if_missing(fragment, tgt_node);
                        vl_edge(fragment, mod_nid, &tgt_nid, "instantiates", path);
                    }
                }
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    walk_vl(
                        child,
                        source,
                        file_id,
                        stem,
                        path,
                        fragment,
                        module_nid.clone(),
                    );
                }
            }
        }
    }
}

impl VerilogExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let stem = file_id.clone();
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang: tree_sitter::Language = tree_sitter_verilog::LANGUAGE.into();
        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| CodeSynapseError::Parse(format!("verilog set_language: {e}")))?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| CodeSynapseError::Parse("verilog parse failed".to_string()))?;

        walk_vl(
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

impl LanguageExtractor for VerilogExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["v", "sv", "svh"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
