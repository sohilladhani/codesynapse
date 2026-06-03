use super::{add_node_if_missing, make_file_node};
use crate::error::{CodeSynapseError, Result};
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Node as TsNode, Parser};

pub struct TsSwiftExtractor;

fn sw_text(source: &[u8], node: &TsNode<'_>) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .trim()
        .to_string()
}

fn sw_node(id: String, label: String, file_type: &str, path: &Path) -> Node {
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

fn prescan_protocols(root: TsNode<'_>, source: &[u8]) -> HashSet<String> {
    let mut protos = HashSet::new();
    fn walk(node: TsNode<'_>, source: &[u8], protos: &mut HashSet<String>) {
        if node.kind() == "protocol_declaration" {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "type_identifier" {
                        let name = sw_text(source, &child);
                        if !name.is_empty() {
                            protos.insert(name);
                        }
                        break;
                    }
                }
            }
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                walk(child, source, protos);
            }
        }
    }
    walk(root, source, &mut protos);
    protos
}

impl TsSwiftExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_swift::LANGUAGE.into();
        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| CodeSynapseError::Parse(format!("swift lang: {}", e)))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| CodeSynapseError::Parse("swift parse failed".to_string()))?;

        let stem = file_id.clone();

        let protocols = prescan_protocols(tree.root_node(), source);

        Self::walk(
            tree.root_node(),
            source,
            path,
            &file_id,
            &stem,
            &protocols,
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
        protocols: &HashSet<String>,
        fragment: &mut ExtractionFragment,
    ) {
        match node.kind() {
            "import_declaration" => {
                // import_declaration → identifier
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.kind() == "identifier" {
                            let name = sw_text(source, &child);
                            if !name.is_empty() {
                                let id = make_id(&[&name]);
                                add_node_if_missing(
                                    fragment,
                                    sw_node(id.clone(), name, "module", path),
                                );
                                fragment.edges.push(Edge {
                                    source: file_id.to_string(),
                                    target: id,
                                    relation: "imports".to_string(),
                                    confidence: "EXTRACTED".to_string(),
                                    source_file: Some(path.to_string_lossy().to_string()),
                                    weight: 1.0,
                                    context: Some("import".to_string()),
                                });
                                break;
                            }
                        }
                    }
                }
            }
            "class_declaration" => {
                // name field is the type identifier (class/struct/enum/actor/extension all use class_declaration)
                let type_name = node
                    .child_by_field_name("name")
                    .map(|n| sw_text(source, &n))
                    .filter(|t| !t.is_empty());
                if let Some(type_name) = type_name {
                    let type_id = make_id(&[stem, &type_name]);
                    add_node_if_missing(
                        fragment,
                        sw_node(type_id.clone(), type_name.clone(), "class", path),
                    );
                    fragment.edges.push(Edge {
                        source: file_id.to_string(),
                        target: type_id.clone(),
                        relation: "contains".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });

                    // inheritance_specifier is a direct child (no type_inheritance_clause wrapper in swift 0.7.2)
                    for i in 0..node.child_count() {
                        if let Some(child) = node.child(i) {
                            if child.kind() == "inheritance_specifier" {
                                Self::handle_inheritance_specifier(
                                    &child, source, path, &type_id, fragment, protocols, stem,
                                );
                            }
                        }
                    }

                    // Walk body for methods
                    if let Some(body) = node.child_by_field_name("body") {
                        Self::walk_body(
                            body, source, path, file_id, stem, &type_id, protocols, fragment,
                        );
                    }
                }
            }
            "protocol_declaration" => {
                let type_name = {
                    let mut found = None;
                    for i in 0..node.child_count() {
                        if let Some(child) = node.child(i) {
                            if child.kind() == "type_identifier" {
                                let t = sw_text(source, &child);
                                if !t.is_empty() {
                                    found = Some(t);
                                    break;
                                }
                            }
                        }
                    }
                    found
                };
                if let Some(type_name) = type_name {
                    let type_id = make_id(&[stem, &type_name]);
                    add_node_if_missing(
                        fragment,
                        sw_node(type_id.clone(), type_name, "trait", path),
                    );
                    fragment.edges.push(Edge {
                        source: file_id.to_string(),
                        target: type_id,
                        relation: "contains".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                }
            }
            "function_declaration" => {
                // Top-level function
                Self::extract_function(
                    node, source, path, file_id, stem, file_id, protocols, fragment,
                );
            }
            _ => {
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        Self::walk(child, source, path, file_id, stem, protocols, fragment);
                    }
                }
            }
        }
    }

    fn handle_inheritance_specifier(
        spec: &TsNode<'_>,
        source: &[u8],
        path: &Path,
        type_id: &str,
        fragment: &mut ExtractionFragment,
        protocols: &HashSet<String>,
        stem: &str,
    ) {
        // inherits_from field = user_type; user_type children include type_identifier
        let user_type = spec.child_by_field_name("inherits_from");
        if let Some(ut) = user_type {
            if ut.kind() == "user_type" {
                for i in 0..ut.child_count() {
                    if let Some(child) = ut.child(i) {
                        if child.kind() == "type_identifier" {
                            let base_name = sw_text(source, &child);
                            if !base_name.is_empty() {
                                let base_id = make_id(&[stem, &base_name]);
                                add_node_if_missing(
                                    fragment,
                                    sw_node(base_id.clone(), base_name.clone(), "code", path),
                                );
                                let relation = if protocols.contains(&base_name) {
                                    "implements"
                                } else {
                                    "inherits"
                                };
                                let already = fragment.edges.iter().any(|e| {
                                    e.relation == relation
                                        && e.source == type_id
                                        && e.target == base_id
                                });
                                if !already {
                                    fragment.edges.push(Edge {
                                        source: type_id.to_string(),
                                        target: base_id,
                                        relation: relation.to_string(),
                                        confidence: "EXTRACTED".to_string(),
                                        source_file: Some(path.to_string_lossy().to_string()),
                                        weight: 1.0,
                                        context: None,
                                    });
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_body(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        file_id: &str,
        stem: &str,
        owner_id: &str,
        protocols: &HashSet<String>,
        fragment: &mut ExtractionFragment,
    ) {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                match child.kind() {
                    "function_declaration"
                    | "init_declaration"
                    | "deinit_declaration"
                    | "subscript_declaration" => {
                        Self::extract_function(
                            child, source, path, file_id, stem, owner_id, protocols, fragment,
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn extract_function(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        file_id: &str,
        stem: &str,
        owner_id: &str,
        _protocols: &HashSet<String>,
        fragment: &mut ExtractionFragment,
    ) {
        let func_name = match node.kind() {
            "deinit_declaration" => "deinit".to_string(),
            "subscript_declaration" => "subscript".to_string(),
            _ => {
                let mut found = None;
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.kind() == "simple_identifier" {
                            let t = sw_text(source, &child);
                            if !t.is_empty() && t != "func" && t != "init" {
                                found = Some(t);
                                break;
                            }
                        }
                    }
                }
                match found {
                    Some(n) => n,
                    None => return,
                }
            }
        };
        let func_id = make_id(&[stem, &func_name, "()"]);
        let fn_file_type = if owner_id == file_id {
            "function"
        } else {
            "method"
        };
        add_node_if_missing(
            fragment,
            sw_node(
                func_id.clone(),
                format!("{}()", func_name),
                fn_file_type,
                path,
            ),
        );
        fragment.edges.push(Edge {
            source: owner_id.to_string(),
            target: func_id.clone(),
            relation: "method".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some(path.to_string_lossy().to_string()),
            weight: 1.0,
            context: None,
        });
        fragment.edges.push(Edge {
            source: file_id.to_string(),
            target: func_id.clone(),
            relation: "contains".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some(path.to_string_lossy().to_string()),
            weight: 1.0,
            context: None,
        });

        // Return type annotation
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if child.kind() == "type_annotation" {
                    // type_annotation → user_type → type_identifier
                    Self::emit_return_type_ref(&child, source, path, &func_id, fragment, stem);
                }
            }
        }

        // Parameters
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if child.kind() == "parameter" {
                    Self::extract_swift_param(&child, source, path, &func_id, fragment, stem);
                } else if child.kind() == "function_value_parameters"
                    || child.kind() == "parameter_clause"
                {
                    for j in 0..child.child_count() {
                        if let Some(param) = child.child(j) {
                            if param.kind() == "parameter" {
                                Self::extract_swift_param(
                                    &param, source, path, &func_id, fragment, stem,
                                );
                            }
                        }
                    }
                }
            }
        }

        // Walk body for calls
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if child.kind() == "function_body" || child.kind() == "code_block" {
                    Self::walk_calls_sw(child, source, path, &func_id, fragment, stem);
                }
            }
        }
    }

    fn emit_return_type_ref(
        annot: &TsNode<'_>,
        source: &[u8],
        path: &Path,
        func_id: &str,
        fragment: &mut ExtractionFragment,
        stem: &str,
    ) {
        for i in 0..annot.child_count() {
            if let Some(child) = annot.child(i) {
                if child.kind() == "user_type" {
                    if let Some(name_n) = child.child(0) {
                        if name_n.kind() == "type_identifier" {
                            let name = sw_text(source, &name_n);
                            if !name.is_empty() {
                                let id = make_id(&[stem, &name]);
                                add_node_if_missing(
                                    fragment,
                                    sw_node(id.clone(), name.clone(), "code", path),
                                );
                                fragment.edges.push(Edge {
                                    source: func_id.to_string(),
                                    target: id,
                                    relation: "references".to_string(),
                                    confidence: "EXTRACTED".to_string(),
                                    source_file: Some(path.to_string_lossy().to_string()),
                                    weight: 1.0,
                                    context: Some("return_type".to_string()),
                                });
                                // Generic args
                                if name_n.child_count() == 0 {
                                    // Look for type_arguments in user_type
                                    for j in 0..child.child_count() {
                                        if let Some(ta) = child.child(j) {
                                            if ta.kind() == "type_arguments" {
                                                Self::emit_generic_args_sw(
                                                    &ta, source, path, func_id, fragment, stem,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn emit_generic_args_sw(
        args: &TsNode<'_>,
        source: &[u8],
        path: &Path,
        func_id: &str,
        fragment: &mut ExtractionFragment,
        stem: &str,
    ) {
        for i in 0..args.child_count() {
            if let Some(arg) = args.child(i) {
                if arg.kind() == "type_identifier" {
                    let name = sw_text(source, &arg);
                    if !name.is_empty() {
                        let id = make_id(&[stem, &name]);
                        add_node_if_missing(fragment, sw_node(id.clone(), name, "code", path));
                        fragment.edges.push(Edge {
                            source: func_id.to_string(),
                            target: id,
                            relation: "references".to_string(),
                            confidence: "EXTRACTED".to_string(),
                            source_file: Some(path.to_string_lossy().to_string()),
                            weight: 1.0,
                            context: Some("generic_arg".to_string()),
                        });
                    }
                }
            }
        }
    }

    fn extract_swift_param(
        param: &TsNode<'_>,
        source: &[u8],
        path: &Path,
        func_id: &str,
        fragment: &mut ExtractionFragment,
        stem: &str,
    ) {
        for i in 0..param.child_count() {
            if let Some(child) = param.child(i) {
                if child.kind() == "type_annotation" {
                    for j in 0..child.child_count() {
                        if let Some(t) = child.child(j) {
                            if t.kind() == "user_type" {
                                if let Some(name_n) = t.child(0) {
                                    if name_n.kind() == "type_identifier" {
                                        let name = sw_text(source, &name_n);
                                        if !name.is_empty() {
                                            let id = make_id(&[stem, &name]);
                                            add_node_if_missing(
                                                fragment,
                                                sw_node(id.clone(), name.clone(), "code", path),
                                            );
                                            fragment.edges.push(Edge {
                                                source: func_id.to_string(),
                                                target: id.clone(),
                                                relation: "references".to_string(),
                                                confidence: "EXTRACTED".to_string(),
                                                source_file: Some(
                                                    path.to_string_lossy().to_string(),
                                                ),
                                                weight: 1.0,
                                                context: Some("parameter_type".to_string()),
                                            });
                                            // generic args in this param type
                                            for k in 0..t.child_count() {
                                                if let Some(ta) = t.child(k) {
                                                    if ta.kind() == "type_arguments" {
                                                        Self::emit_generic_args_sw(
                                                            &ta, source, path, func_id, fragment,
                                                            stem,
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn walk_calls_sw(
        node: TsNode<'_>,
        source: &[u8],
        path: &Path,
        caller_id: &str,
        fragment: &mut ExtractionFragment,
        stem: &str,
    ) {
        if node.kind() == "call_expression" {
            if let Some(func) = node.child(0) {
                let callee_name = match func.kind() {
                    "navigation_expression" | "member_access_expression" => {
                        // last identifier
                        let mut name = String::new();
                        for i in 0..func.child_count() {
                            if let Some(child) = func.child(i) {
                                if child.kind() == "simple_identifier" {
                                    name = sw_text(source, &child);
                                }
                            }
                        }
                        name
                    }
                    "simple_identifier" => sw_text(source, &func),
                    _ => String::new(),
                };
                if !callee_name.is_empty() {
                    let callee_id = make_id(&[stem, &callee_name, "()"]);
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
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                Self::walk_calls_sw(child, source, path, caller_id, fragment, stem);
            }
        }
    }
}

impl LanguageExtractor for TsSwiftExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["swift"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
