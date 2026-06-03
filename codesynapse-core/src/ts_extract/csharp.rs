use super::{add_node_if_missing, make_file_node};
use crate::error::{CodeSynapseError, Result};
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Node as TsNode, Parser};

pub struct TsCSharpExtractor;

fn cs_text(source: &[u8], node: &TsNode<'_>) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .trim()
        .to_string()
}

fn cs_node(id: String, label: String, file_type: &str, path: &Path) -> Node {
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

/// Pre-scan to collect interface names (start with 'I' followed by uppercase).
fn prescan_interfaces(root: TsNode<'_>, source: &[u8]) -> HashSet<String> {
    let mut ifaces = HashSet::new();
    prescan_walk(root, source, &mut ifaces);
    ifaces
}

fn prescan_walk(node: TsNode<'_>, source: &[u8], ifaces: &mut HashSet<String>) {
    if node.kind() == "interface_declaration" {
        if let Some(name_node) = node.child_by_field_name("name") {
            let name = cs_text(source, &name_node);
            ifaces.insert(name);
        }
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            prescan_walk(child, source, ifaces);
        }
    }
}

impl TsCSharpExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_c_sharp::LANGUAGE.into();
        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| CodeSynapseError::Parse(format!("csharp lang: {}", e)))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| CodeSynapseError::Parse("csharp parse failed".to_string()))?;

        let stem = file_id.clone();

        let interface_names = prescan_interfaces(tree.root_node(), source);

        Self::walk(
            tree.root_node(),
            source,
            path,
            &file_id,
            &stem,
            &interface_names,
            None,
            &mut fragment,
        );

        Ok(fragment)
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        file_id: &str,
        stem: &str,
        ifaces: &HashSet<String>,
        class_id: Option<&str>,
        fragment: &mut ExtractionFragment,
    ) {
        match node.kind() {
            "using_directive" => {
                // using System.Collections.Generic;
                // Find qualified_name or identifier
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        match child.kind() {
                            "identifier" | "qualified_name" => {
                                let full = cs_text(source, &child);
                                let leaf = full.split('.').next_back().unwrap_or(&full).to_string();
                                let leaf_id = make_id(&[&leaf]);
                                add_node_if_missing(
                                    fragment,
                                    cs_node(leaf_id.clone(), leaf, "code", path),
                                );
                                fragment.edges.push(Edge {
                                    source: file_id.to_string(),
                                    target: leaf_id,
                                    relation: "imports".to_string(),
                                    confidence: "EXTRACTED".to_string(),
                                    source_file: Some(path.to_string_lossy().to_string()),
                                    weight: 1.0,
                                    context: Some("import".to_string()),
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
            "namespace_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = cs_text(source, &name_node);
                    let ns_id = make_id(&[file_id, &name, "ns"]);
                    add_node_if_missing(
                        fragment,
                        Node {
                            id: ns_id.clone(),
                            label: format!("{} namespace", name),
                            file_type: "namespace".to_string(),
                            source_file: path.to_string_lossy().to_string(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: {
                                let mut m = HashMap::new();
                                m.insert("kind".to_string(), "namespace".to_string());
                                m
                            },
                        },
                    );
                    fragment.edges.push(Edge {
                        source: file_id.to_string(),
                        target: ns_id.clone(),
                        relation: "contains".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                }
                // Walk children
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        Self::walk(
                            child, source, path, file_id, stem, ifaces, class_id, fragment,
                        );
                    }
                }
            }
            "class_declaration" => {
                let name_node = match node.child_by_field_name("name") {
                    Some(n) => n,
                    None => return,
                };
                let class_name = cs_text(source, &name_node);
                let new_class_id = make_id(&[stem, &class_name]);
                add_node_if_missing(
                    fragment,
                    cs_node(new_class_id.clone(), class_name.clone(), "class", path),
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

                // base_list is not a named field — scan children for node kind "base_list"
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.kind() == "base_list" {
                            Self::handle_base_list(
                                &child,
                                source,
                                path,
                                &new_class_id,
                                fragment,
                                ifaces,
                                stem,
                            );
                            break;
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
                                ifaces,
                                Some(&new_class_id),
                                fragment,
                            );
                        }
                    }
                }
            }
            "interface_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = cs_text(source, &name_node);
                    let id = make_id(&[stem, &name]);
                    add_node_if_missing(fragment, cs_node(id.clone(), name, "interface", path));
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
            "method_declaration" => {
                let owner = class_id.unwrap_or(file_id);
                Self::extract_method(node, source, path, file_id, stem, owner, fragment);
            }
            "field_declaration" => {
                if let Some(owner) = class_id {
                    Self::extract_field(node, source, path, owner, fragment, stem);
                }
            }
            "invocation_expression" => {
                if let Some(caller) = class_id {
                    Self::emit_call(node, source, path, caller, fragment, stem);
                }
            }
            _ => {
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        Self::walk(
                            child, source, path, file_id, stem, ifaces, class_id, fragment,
                        );
                    }
                }
            }
        }
    }

    fn handle_base_list(
        bases: &TsNode<'_>,
        source: &[u8],
        path: &Path,
        class_id: &str,
        fragment: &mut ExtractionFragment,
        ifaces: &HashSet<String>,
        stem: &str,
    ) {
        for i in 0..bases.child_count() {
            if let Some(child) = bases.child(i) {
                let type_name = match child.kind() {
                    "identifier" => cs_text(source, &child),
                    "generic_name" => {
                        if let Some(name_n) = child.child_by_field_name("name") {
                            cs_text(source, &name_n)
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };
                let base_id = make_id(&[stem, &type_name]);
                add_node_if_missing(
                    fragment,
                    cs_node(base_id.clone(), type_name.clone(), "code", path),
                );
                let relation = if ifaces.contains(&type_name) {
                    "implements"
                } else {
                    "inherits"
                };
                fragment.edges.push(Edge {
                    source: class_id.to_string(),
                    target: base_id,
                    relation: relation.to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }
    }

    fn extract_method(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        file_id: &str,
        stem: &str,
        _class_id: &str,
        fragment: &mut ExtractionFragment,
    ) {
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return,
        };
        let method_name = cs_text(source, &name_node);
        let method_id = make_id(&[stem, &method_name, "()"]);
        add_node_if_missing(
            fragment,
            cs_node(
                method_id.clone(),
                format!("{}()", method_name),
                "method",
                path,
            ),
        );
        fragment.edges.push(Edge {
            source: file_id.to_string(),
            target: method_id.clone(),
            relation: "contains".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some(path.to_string_lossy().to_string()),
            weight: 1.0,
            context: None,
        });

        // Return type → return_type context
        if let Some(ret_type) = node.child_by_field_name("returns") {
            Self::emit_type_ref_cs(
                &ret_type,
                source,
                path,
                &method_id,
                fragment,
                "return_type",
                true,
            );
        }

        // Parameters → parameter_type context
        if let Some(params) = node.child_by_field_name("parameters") {
            for i in 0..params.child_count() {
                if let Some(param) = params.child(i) {
                    if param.kind() == "parameter" {
                        if let Some(ptype) = param.child_by_field_name("type") {
                            Self::emit_type_ref_cs(
                                &ptype,
                                source,
                                path,
                                &method_id,
                                fragment,
                                "parameter_type",
                                false,
                            );
                        }
                    }
                }
            }
        }

        // Walk body for calls
        if let Some(body) = node.child_by_field_name("body") {
            Self::walk_for_calls(body, source, path, &method_id, fragment, stem);
        }
    }

    fn extract_field(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        class_id: &str,
        fragment: &mut ExtractionFragment,
        _stem: &str,
    ) {
        // field_declaration has type: and declaration: (variable_declaration)
        if let Some(type_node) = node.child_by_field_name("type") {
            Self::emit_type_ref_cs(&type_node, source, path, class_id, fragment, "field", false);
        }
    }

    fn walk_for_calls(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        caller_id: &str,
        fragment: &mut ExtractionFragment,
        stem: &str,
    ) {
        if node.kind() == "invocation_expression" {
            Self::emit_call(node, source, path, caller_id, fragment, stem);
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                Self::walk_for_calls(child, source, path, caller_id, fragment, stem);
            }
        }
    }

    fn emit_call(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        caller_id: &str,
        fragment: &mut ExtractionFragment,
        stem: &str,
    ) {
        // invocation_expression: function member_access_expression or identifier
        if let Some(func) = node.child_by_field_name("function") {
            let callee_name = match func.kind() {
                "member_access_expression" => {
                    // name field is the method name
                    func.child_by_field_name("name")
                        .map(|n| cs_text(source, &n))
                        .unwrap_or_default()
                }
                "identifier" => cs_text(source, &func),
                _ => return,
            };
            if callee_name.is_empty() {
                return;
            }
            let callee_id = make_id(&[stem, &callee_name, "()"]);
            // emit call without requiring callee to already be in fragment (handles forward refs)
            add_node_if_missing(
                fragment,
                cs_node(
                    callee_id.clone(),
                    format!("{}()", callee_name),
                    "code",
                    path,
                ),
            );
            let already = fragment
                .edges
                .iter()
                .any(|e| e.relation == "calls" && e.source == caller_id && e.target == callee_id);
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

    fn emit_type_ref_cs(
        type_node: &TsNode<'_>,
        source: &[u8],
        path: &Path,
        owner_id: &str,
        fragment: &mut ExtractionFragment,
        context: &str,
        emit_generics: bool,
    ) {
        match type_node.kind() {
            "identifier" => {
                let name = cs_text(source, type_node);
                if !name.is_empty() && name != "void" && name != "var" {
                    let id = make_id(&[&name]);
                    add_node_if_missing(fragment, cs_node(id.clone(), name, "code", path));
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
            "generic_name" => {
                // generic_name has no named fields; scan children for identifier + type_argument_list
                let mut type_name = String::new();
                let mut args_node: Option<TsNode> = None;
                for i in 0..type_node.child_count() {
                    if let Some(child) = type_node.child(i) {
                        match child.kind() {
                            "identifier" if type_name.is_empty() => {
                                type_name = cs_text(source, &child);
                            }
                            "type_argument_list" => {
                                args_node = Some(child);
                            }
                            _ => {}
                        }
                    }
                }
                if !type_name.is_empty() {
                    let id = make_id(&[&type_name]);
                    add_node_if_missing(fragment, cs_node(id.clone(), type_name, "code", path));
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
                if emit_generics {
                    if let Some(args) = args_node {
                        for i in 0..args.child_count() {
                            if let Some(arg) = args.child(i) {
                                Self::emit_type_ref_cs(
                                    &arg,
                                    source,
                                    path,
                                    owner_id,
                                    fragment,
                                    "generic_arg",
                                    false,
                                );
                            }
                        }
                    }
                }
            }
            "nullable_type" | "array_type" | "pointer_type" => {
                for i in 0..type_node.child_count() {
                    if let Some(child) = type_node.child(i) {
                        Self::emit_type_ref_cs(
                            &child,
                            source,
                            path,
                            owner_id,
                            fragment,
                            context,
                            emit_generics,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

impl LanguageExtractor for TsCSharpExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["cs"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
