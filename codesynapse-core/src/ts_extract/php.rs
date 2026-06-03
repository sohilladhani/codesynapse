use super::{add_node_if_missing, make_file_node};
use crate::error::{CodeSynapseError, Result};
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node as TsNode, Parser};

pub struct TsPhpExtractor;

fn ph_text(source: &[u8], node: &TsNode<'_>) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .trim()
        .to_string()
}

fn ph_node(id: String, label: String, file_type: &str, path: &Path) -> Node {
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

fn simple_name(s: &str) -> &str {
    s.split('\\').next_back().unwrap_or(s)
}

impl TsPhpExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_php::LANGUAGE_PHP_ONLY.into();
        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| CodeSynapseError::Parse(format!("php lang: {}", e)))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| CodeSynapseError::Parse("php parse failed".to_string()))?;

        let stem = file_id.clone();

        Self::walk(
            tree.root_node(),
            source,
            path,
            &file_id,
            &stem,
            None,
            &mut fragment,
        );

        Ok(fragment)
    }

    fn walk(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        file_id: &str,
        stem: &str,
        class_id: Option<&str>,
        fragment: &mut ExtractionFragment,
    ) {
        match node.kind() {
            "namespace_use_clause" | "namespace_use_declaration" => {
                // use App\Http\Something
                let text = ph_text(source, &node);
                let leaf = text
                    .split('\\')
                    .next_back()
                    .unwrap_or(&text)
                    .trim()
                    .to_string();
                if !leaf.is_empty() && leaf != "use" {
                    let id = make_id(&[&leaf]);
                    add_node_if_missing(fragment, ph_node(id.clone(), leaf, "module", path));
                    fragment.edges.push(Edge {
                        source: file_id.to_string(),
                        target: id,
                        relation: "imports".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: Some("import".to_string()),
                    });
                }
            }
            "class_declaration" => {
                let name_node = node.child_by_field_name("name");
                let class_name = match name_node {
                    Some(n) => {
                        let t = ph_text(source, &n);
                        simple_name(&t).to_string()
                    }
                    None => return,
                };
                let new_class_id = make_id(&[stem, &class_name]);
                add_node_if_missing(
                    fragment,
                    ph_node(new_class_id.clone(), class_name.clone(), "class", path),
                );
                fragment.edges.push(Edge {
                    source: file_id.to_string(),
                    target: new_class_id.clone(),
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });

                // base_clause and class_interface_clause are unnamed children (not fields)
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        match child.kind() {
                            "base_clause" => {
                                Self::collect_names_as(
                                    &child,
                                    source,
                                    path,
                                    &new_class_id,
                                    fragment,
                                    "inherits",
                                    stem,
                                );
                            }
                            "class_interface_clause" => {
                                Self::collect_names_as(
                                    &child,
                                    source,
                                    path,
                                    &new_class_id,
                                    fragment,
                                    "implements",
                                    stem,
                                );
                            }
                            _ => {}
                        }
                    }
                }

                // Walk body
                if let Some(body) = node.child_by_field_name("body") {
                    for i in 0..body.child_count() {
                        if let Some(child) = body.child(i) {
                            Self::walk(
                                child,
                                source,
                                path,
                                file_id,
                                stem,
                                Some(&new_class_id),
                                fragment,
                            );
                        }
                    }
                }
            }
            "use_declaration" => {
                // trait use inside class: use TraitName;
                if let Some(owner) = class_id {
                    for i in 0..node.child_count() {
                        if let Some(child) = node.child(i) {
                            if child.kind() == "name" || child.kind() == "qualified_name" {
                                let trait_name = simple_name(&ph_text(source, &child)).to_string();
                                let trait_id = make_id(&[stem, &trait_name]);
                                add_node_if_missing(
                                    fragment,
                                    ph_node(trait_id.clone(), trait_name, "code", path),
                                );
                                fragment.edges.push(Edge {
                                    source: owner.to_string(),
                                    target: trait_id,
                                    relation: "mixes_in".to_string(),
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
            "method_declaration" => {
                let owner = class_id.unwrap_or(file_id);
                Self::extract_method(node, source, path, file_id, stem, owner, fragment);
            }
            "function_definition" => {
                if class_id.is_none() {
                    // Top-level function
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = ph_text(source, &name_node);
                        let id = make_id(&[stem, &name, "()"]);
                        add_node_if_missing(
                            fragment,
                            ph_node(id.clone(), format!("{}()", name), "function", path),
                        );
                        fragment.edges.push(Edge {
                            source: file_id.to_string(),
                            target: id,
                            relation: "contains".to_string(),
                            confidence: "EXTRACTED".to_string(),
                            source_file: Some(path.to_string_lossy().to_string()),
                            weight: 1.0,
                            context: None,
                        });
                    }
                }
            }
            "property_declaration" => {
                if let Some(owner) = class_id {
                    Self::extract_property(node, source, path, owner, fragment, stem);
                }
            }
            _ => {
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        Self::walk(child, source, path, file_id, stem, class_id, fragment);
                    }
                }
            }
        }
    }

    fn collect_names_as(
        node: &TsNode<'_>,
        source: &[u8],
        path: &Path,
        source_id: &str,
        fragment: &mut ExtractionFragment,
        relation: &str,
        stem: &str,
    ) {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if child.kind() == "name" || child.kind() == "qualified_name" {
                    let name = simple_name(&ph_text(source, &child)).to_string();
                    let id = make_id(&[stem, &name]);
                    add_node_if_missing(fragment, ph_node(id.clone(), name, "code", path));
                    fragment.edges.push(Edge {
                        source: source_id.to_string(),
                        target: id,
                        relation: relation.to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                } else {
                    Self::collect_names_as(
                        &child, source, path, source_id, fragment, relation, stem,
                    );
                }
            }
        }
    }

    fn extract_method(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        file_id: &str,
        stem: &str,
        owner_id: &str,
        fragment: &mut ExtractionFragment,
    ) {
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return,
        };
        let method_name = ph_text(source, &name_node);
        let method_id = make_id(&[stem, &method_name, "()"]);
        add_node_if_missing(
            fragment,
            ph_node(
                method_id.clone(),
                format!("{}()", method_name),
                "method",
                path,
            ),
        );
        fragment.edges.push(Edge {
            source: owner_id.to_string(),
            target: method_id.clone(),
            relation: "method".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some(path.to_string_lossy().to_string()),
            weight: 1.0,
            context: None,
        });
        fragment.edges.push(Edge {
            source: file_id.to_string(),
            target: method_id.clone(),
            relation: "contains".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some(path.to_string_lossy().to_string()),
            weight: 1.0,
            context: None,
        });

        // Return type
        if let Some(ret) = node.child_by_field_name("return_type") {
            Self::emit_type_ref_ph(
                &ret,
                source,
                path,
                &method_id,
                fragment,
                "return_type",
                stem,
            );
        }

        // Parameters
        if let Some(params) = node.child_by_field_name("parameters") {
            for i in 0..params.child_count() {
                if let Some(param) = params.child(i) {
                    if param.kind() == "simple_parameter"
                        || param.kind() == "property_promotion_parameter"
                    {
                        if let Some(ptype) = param.child_by_field_name("type") {
                            Self::emit_type_ref_ph(
                                &ptype,
                                source,
                                path,
                                &method_id,
                                fragment,
                                "parameter_type",
                                stem,
                            );
                        }
                    }
                }
            }
        }

        // Body for calls
        if let Some(body) = node.child_by_field_name("body") {
            Self::walk_calls_ph(body, source, path, &method_id, fragment, stem);
        }
    }

    fn extract_property(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        class_id: &str,
        fragment: &mut ExtractionFragment,
        stem: &str,
    ) {
        if let Some(type_node) = node.child_by_field_name("type") {
            Self::emit_type_ref_ph(&type_node, source, path, class_id, fragment, "field", stem);
        }
    }

    fn emit_type_ref_ph(
        type_node: &TsNode<'_>,
        source: &[u8],
        path: &Path,
        owner_id: &str,
        fragment: &mut ExtractionFragment,
        context: &str,
        stem: &str,
    ) {
        match type_node.kind() {
            "named_type" | "name" | "qualified_name" => {
                let name = simple_name(&ph_text(source, type_node)).to_string();
                if !name.is_empty() {
                    let id = make_id(&[stem, &name]);
                    add_node_if_missing(fragment, ph_node(id.clone(), name, "code", path));
                    fragment.edges.push(Edge {
                        source: owner_id.to_string(),
                        target: id,
                        relation: "references".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: Some(context.to_string()),
                    });
                }
            }
            _ => {
                for i in 0..type_node.child_count() {
                    if let Some(child) = type_node.child(i) {
                        Self::emit_type_ref_ph(
                            &child, source, path, owner_id, fragment, context, stem,
                        );
                    }
                }
            }
        }
    }

    fn walk_calls_ph(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        caller_id: &str,
        fragment: &mut ExtractionFragment,
        stem: &str,
    ) {
        if node.kind() == "function_call_expression" || node.kind() == "member_call_expression" {
            let func_name = node
                .child_by_field_name("function")
                .or_else(|| node.child_by_field_name("name"))
                .map(|n| ph_text(source, &n));
            if let Some(name) = func_name {
                let callee_id = make_id(&[stem, &name, "()"]);
                if fragment.nodes.iter().any(|n| n.id == callee_id) {
                    let already = fragment.edges.iter().any(|e| {
                        e.relation == "calls" && e.source == caller_id && e.target == callee_id
                    });
                    if !already && caller_id != callee_id.as_str() {
                        fragment.edges.push(Edge {
                            source: caller_id.to_string(),
                            target: callee_id,
                            relation: "calls".to_string(),
                            confidence: "EXTRACTED".to_string(),
                            source_file: Some(path.to_string_lossy().to_string()),
                            weight: 1.0,
                            context: Some("call".to_string()),
                        });
                    }
                }
            }
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                Self::walk_calls_ph(child, source, path, caller_id, fragment, stem);
            }
        }
    }
}

impl LanguageExtractor for TsPhpExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["php"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
