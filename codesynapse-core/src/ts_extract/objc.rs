use super::make_file_node;
use crate::error::{CodeSynapseError, Result};
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node as TsNode, Parser};

pub struct ObjCExtractor;

fn oc_text(source: &[u8], node: &TsNode<'_>) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .trim()
        .to_string()
}

fn oc_node(id: String, label: String, file_type: &str, path: &Path) -> Node {
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

fn oc_edge(fragment: &mut ExtractionFragment, src: &str, tgt: &str, rel: &str, path: &Path) {
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

fn first_identifier(source: &[u8], node: &TsNode<'_>) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "identifier" {
                let t = oc_text(source, &child);
                if !t.is_empty() {
                    return Some(t);
                }
            }
        }
    }
    None
}

/// Build ObjC method selector: join all identifier children (selector parts).
fn method_selector(source: &[u8], node: &TsNode<'_>) -> Option<String> {
    let parts: Vec<String> = (0..node.child_count())
        .filter_map(|i| node.child(i))
        .filter(|c| c.kind() == "identifier")
        .map(|c| oc_text(source, &c))
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(""))
    }
}

fn walk_oc<'t>(
    node: TsNode<'t>,
    source: &[u8],
    file_id: &str,
    stem: &str,
    path: &Path,
    fragment: &mut ExtractionFragment,
    parent_nid: Option<String>,
) {
    match node.kind() {
        "preproc_include" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    match child.kind() {
                        "system_lib_string" => {
                            let raw = oc_text(source, &child);
                            let module = raw.trim_matches(|c| c == '<' || c == '>');
                            let module = module.rsplit('/').next().unwrap_or(module);
                            let module = module.trim_end_matches(".h");
                            if !module.is_empty() {
                                let tgt = make_id(&[module]);
                                fragment.edges.push(Edge {
                                    source: file_id.to_string(),
                                    target: tgt,
                                    relation: "imports".to_string(),
                                    confidence: "EXTRACTED".to_string(),
                                    source_file: Some(path.to_string_lossy().to_string()),
                                    weight: 1.0,
                                    context: Some("import".to_string()),
                                });
                            }
                        }
                        "string_literal" => {
                            // string_content child
                            for j in 0..child.child_count() {
                                if let Some(sub) = child.child(j) {
                                    if sub.kind() == "string_content" {
                                        let raw = oc_text(source, &sub);
                                        let module = raw.rsplit('/').next().unwrap_or(&raw);
                                        let module = module.trim_end_matches(".h");
                                        if !module.is_empty() {
                                            let tgt = make_id(&[module]);
                                            fragment.edges.push(Edge {
                                                source: file_id.to_string(),
                                                target: tgt,
                                                relation: "imports".to_string(),
                                                confidence: "EXTRACTED".to_string(),
                                                source_file: Some(
                                                    path.to_string_lossy().to_string(),
                                                ),
                                                weight: 1.0,
                                                context: Some("import".to_string()),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        "class_interface" => {
            if let Some(name) = first_identifier(source, &node) {
                let cls_nid = make_id(&[stem, &name]);
                fragment
                    .nodes
                    .push(oc_node(cls_nid.clone(), name.clone(), "class", path));
                oc_edge(fragment, file_id, &cls_nid, "contains", path);

                // Second identifier after ':' is superclass
                let mut colon_seen = false;
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        match child.kind() {
                            ":" => colon_seen = true,
                            "identifier" if colon_seen => {
                                let super_name = oc_text(source, &child);
                                if !super_name.is_empty() && super_name != name {
                                    let super_nid = make_id(&[&super_name]);
                                    oc_edge(fragment, &cls_nid, &super_nid, "inherits", path);
                                }
                                colon_seen = false;
                            }
                            "method_declaration" => {
                                walk_oc(
                                    child,
                                    source,
                                    file_id,
                                    stem,
                                    path,
                                    fragment,
                                    Some(cls_nid.clone()),
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        "class_implementation" => {
            if let Some(name) = first_identifier(source, &node) {
                let impl_nid = make_id(&[stem, &name]);
                if !fragment.nodes.iter().any(|n| n.id == impl_nid) {
                    fragment
                        .nodes
                        .push(oc_node(impl_nid.clone(), name, "class", path));
                    oc_edge(fragment, file_id, &impl_nid, "contains", path);
                }
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.kind() == "implementation_definition" {
                            for j in 0..child.child_count() {
                                if let Some(sub) = child.child(j) {
                                    walk_oc(
                                        sub,
                                        source,
                                        file_id,
                                        stem,
                                        path,
                                        fragment,
                                        Some(impl_nid.clone()),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        "protocol_declaration" => {
            if let Some(name) = first_identifier(source, &node) {
                let proto_nid = make_id(&[stem, &name]);
                fragment.nodes.push(oc_node(
                    proto_nid.clone(),
                    format!("<{}>", name),
                    "code",
                    path,
                ));
                oc_edge(fragment, file_id, &proto_nid, "contains", path);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        walk_oc(
                            child,
                            source,
                            file_id,
                            stem,
                            path,
                            fragment,
                            Some(proto_nid.clone()),
                        );
                    }
                }
            }
        }
        "method_declaration" | "method_definition" => {
            let container = parent_nid.as_deref().unwrap_or(file_id);
            if let Some(sel) = method_selector(source, &node) {
                let method_nid = make_id(&[container, &sel]);
                let label = format!("-{}", sel);
                add_node_if_missing(
                    fragment,
                    oc_node(method_nid.clone(), label, "function", path),
                );
                oc_edge(fragment, container, &method_nid, "method", path);
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    walk_oc(
                        child,
                        source,
                        file_id,
                        stem,
                        path,
                        fragment,
                        parent_nid.clone(),
                    );
                }
            }
        }
    }
}

impl ObjCExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let stem = file_id.clone();
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang: tree_sitter::Language = tree_sitter_objc::LANGUAGE.into();
        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| CodeSynapseError::Parse(format!("objc set_language: {e}")))?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| CodeSynapseError::Parse("objc parse failed".to_string()))?;

        walk_oc(
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

impl LanguageExtractor for ObjCExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["m", "mm"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
