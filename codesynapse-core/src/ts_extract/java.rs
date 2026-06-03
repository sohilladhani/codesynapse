use super::{add_node_if_missing, make_file_node, strip_docstring};
use crate::error::{CodeSynapseError, Result};
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Node as TsNode, Parser};

pub struct TsJavaExtractor;

fn collect_field_call_targets(body: &TsNode<'_>, source: &[u8]) -> HashSet<String> {
    let mut types: HashSet<String> = HashSet::new();
    for i in 0..body.child_count() {
        if let Some(child) = body.child(i) {
            if child.kind() == "field_declaration" {
                if let Some(type_node) = child.child_by_field_name("type") {
                    let raw = jv_text(source, &type_node);
                    let base: String = raw
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if base
                        .chars()
                        .next()
                        .map(|c| c.is_uppercase())
                        .unwrap_or(false)
                    {
                        types.insert(base);
                    }
                }
            }
        }
    }
    types
}

fn jv_text(source: &[u8], node: &TsNode<'_>) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .trim()
        .to_string()
}

fn jv_javadoc(node: &TsNode<'_>, source: &[u8]) -> Option<String> {
    node.prev_named_sibling()
        .filter(|s| s.kind() == "block_comment")
        .and_then(|s| {
            let text = std::str::from_utf8(&source[s.start_byte()..s.end_byte()]).ok()?;
            if text.starts_with("/**") {
                strip_docstring(text)
            } else {
                None
            }
        })
}

fn jv_node(id: String, label: String, file_type: &str, path: &Path) -> Node {
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

impl TsJavaExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_java::LANGUAGE.into();
        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| CodeSynapseError::Parse(format!("java lang: {}", e)))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| CodeSynapseError::Parse("java parse failed".to_string()))?;

        let stem = file_id.clone();

        Self::walk(
            tree.root_node(),
            source,
            path,
            &file_id,
            &stem,
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
        fragment: &mut ExtractionFragment,
    ) {
        match node.kind() {
            "method_declaration" => {
                // Top-level method (outside any class) — use file_id as parent
                Self::extract_method(node, source, path, file_id, stem, file_id, fragment);
            }
            "import_declaration" => {
                // scoped_identifier or identifier child
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.kind() == "scoped_identifier" || child.kind() == "identifier" {
                            let full = jv_text(source, &child);
                            let parts: Vec<&str> = full.split('.').collect();
                            // Emit import edge to leaf name
                            let leaf = parts.last().copied().unwrap_or(&full);
                            let leaf_id = make_id(&[leaf]);
                            add_node_if_missing(
                                fragment,
                                jv_node(leaf_id.clone(), leaf.to_string(), "code", path),
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
                    }
                }
            }
            "class_declaration" => {
                let name_node = node.child_by_field_name("name");
                let class_name = match name_node {
                    Some(n) => jv_text(source, &n),
                    None => return,
                };
                let class_id = make_id(&[stem, &class_name]);
                let mut class_node = jv_node(class_id.clone(), class_name.clone(), "class", path);
                class_node.docstring = jv_javadoc(&node, source);
                class_node.source_location =
                    Some(format!("{}:{}", node.start_byte(), node.end_byte()));
                add_node_if_missing(fragment, class_node);
                fragment.edges.push(Edge {
                    source: file_id.to_string(),
                    target: class_id.clone(),
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });

                // superclass → inherits
                if let Some(sc) = node.child_by_field_name("superclass") {
                    for i in 0..sc.child_count() {
                        if let Some(t) = sc.child(i) {
                            if t.kind() == "type_identifier" {
                                let super_name = jv_text(source, &t);
                                let super_id = make_id(&[stem, &super_name]);
                                add_node_if_missing(
                                    fragment,
                                    jv_node(super_id.clone(), super_name, "code", path),
                                );
                                fragment.edges.push(Edge {
                                    source: class_id.clone(),
                                    target: super_id,
                                    relation: "inherits".to_string(),
                                    confidence: "EXTRACTED".to_string(),
                                    source_file: Some(path.to_string_lossy().to_string()),
                                    weight: 1.0,
                                    context: None,
                                });
                            }
                        }
                    }
                }

                // super_interfaces → implements
                if let Some(si) = node.child_by_field_name("interfaces") {
                    Self::collect_type_identifiers_as(
                        &si,
                        source,
                        &class_id,
                        path,
                        fragment,
                        "implements",
                        stem,
                    );
                }

                // Walk body
                if let Some(body) = node.child_by_field_name("body") {
                    for type_name in collect_field_call_targets(&body, source) {
                        let target_id = make_id(&[&type_name, &type_name]);
                        fragment.edges.push(Edge {
                            source: class_id.clone(),
                            target: target_id,
                            relation: "calls".to_string(),
                            confidence: "EXTRACTED".to_string(),
                            source_file: Some(path.to_string_lossy().to_string()),
                            weight: 1.0,
                            context: Some("field_injection".to_string()),
                        });
                    }
                    for i in 0..body.child_count() {
                        if let Some(child) = body.child(i) {
                            if child.kind() == "method_declaration" {
                                Self::extract_method(
                                    child, source, path, file_id, stem, &class_id, fragment,
                                );
                            } else {
                                Self::walk(child, source, path, file_id, stem, fragment);
                            }
                        }
                    }
                }
            }
            "interface_declaration" => {
                let name_node = node.child_by_field_name("name");
                if let Some(n) = name_node {
                    let name = jv_text(source, &n);
                    let id = make_id(&[stem, &name]);
                    add_node_if_missing(fragment, jv_node(id.clone(), name, "interface", path));
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
            _ => {
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        Self::walk(child, source, path, file_id, stem, fragment);
                    }
                }
            }
        }
    }

    fn collect_type_identifiers_as(
        node: &TsNode<'_>,
        source: &[u8],
        source_id: &str,
        path: &Path,
        fragment: &mut ExtractionFragment,
        relation: &str,
        stem: &str,
    ) {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if child.kind() == "type_identifier" {
                    let name = jv_text(source, &child);
                    let id = make_id(&[stem, &name]);
                    add_node_if_missing(fragment, jv_node(id.clone(), name, "code", path));
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
                    Self::collect_type_identifiers_as(
                        &child, source, source_id, path, fragment, relation, stem,
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
        class_id: &str,
        fragment: &mut ExtractionFragment,
    ) {
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return,
        };
        let method_name = jv_text(source, &name_node);
        let method_id = make_id(&[stem, &method_name, "()"]);
        let method_label = format!("{}()", method_name);
        let mut method_node = jv_node(method_id.clone(), method_label, "method", path);
        method_node.docstring = jv_javadoc(&node, source);
        method_node.source_location = Some(format!("{}:{}", node.start_byte(), node.end_byte()));
        add_node_if_missing(fragment, method_node);
        fragment.edges.push(Edge {
            source: class_id.to_string(),
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

        // Annotations → attribute context
        // `modifiers` is not a named field in tree-sitter-java; scan children by kind
        let mods_opt =
            (0..node.child_count()).find_map(|i| node.child(i).filter(|c| c.kind() == "modifiers"));
        if let Some(mods) = mods_opt {
            for i in 0..mods.child_count() {
                if let Some(annot) = mods.child(i) {
                    if annot.kind() == "annotation" || annot.kind() == "marker_annotation" {
                        if let Some(name_n) = annot.child_by_field_name("name") {
                            let annot_name = jv_text(source, &name_n);
                            let annot_id = make_id(&[&annot_name]);
                            add_node_if_missing(
                                fragment,
                                jv_node(annot_id.clone(), annot_name, "code", path),
                            );
                            // Also emit full-text "@Name" node for compatibility
                            let full_text = jv_text(source, &annot);
                            let full_id = make_id(&[&full_text]);
                            add_node_if_missing(
                                fragment,
                                jv_node(full_id, full_text, "code", path),
                            );
                            fragment.edges.push(Edge {
                                source: method_id.clone(),
                                target: annot_id,
                                relation: "references".to_string(),
                                confidence: "EXTRACTED".to_string(),
                                source_file: Some(path.to_string_lossy().to_string()),
                                weight: 1.0,
                                context: Some("attribute".to_string()),
                            });
                        }
                    }
                }
            }
        }

        // Return type
        if let Some(ret_type) = node.child_by_field_name("type") {
            Self::emit_type_ref(
                &ret_type,
                source,
                path,
                &method_id,
                fragment,
                "return_type",
                stem,
                true,
            );
        }

        // Parameters → parameter_type context
        if let Some(params) = node.child_by_field_name("parameters") {
            for i in 0..params.child_count() {
                if let Some(param) = params.child(i) {
                    if param.kind() == "formal_parameter" || param.kind() == "spread_parameter" {
                        if let Some(param_type) = param.child_by_field_name("type") {
                            Self::emit_type_ref(
                                &param_type,
                                source,
                                path,
                                &method_id,
                                fragment,
                                "parameter_type",
                                stem,
                                false,
                            );
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_type_ref(
        type_node: &TsNode<'_>,
        source: &[u8],
        path: &Path,
        method_id: &str,
        fragment: &mut ExtractionFragment,
        context: &str,
        _stem: &str,
        emit_generics: bool,
    ) {
        match type_node.kind() {
            "type_identifier" => {
                let name = jv_text(source, type_node);
                let id = make_id(&[&name]);
                add_node_if_missing(fragment, jv_node(id.clone(), name, "code", path));
                fragment.edges.push(Edge {
                    source: method_id.to_string(),
                    target: id,
                    relation: "references".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: Some(context.to_string()),
                });
            }
            "generic_type" => {
                // tree-sitter-java generic_type has no named fields; scan children by kind
                let name_n = (0..type_node.child_count())
                    .find_map(|i| type_node.child(i).filter(|c| c.kind() == "type_identifier"));
                if let Some(name_n) = name_n {
                    let name = jv_text(source, &name_n);
                    let id = make_id(&[&name]);
                    add_node_if_missing(fragment, jv_node(id.clone(), name, "code", path));
                    fragment.edges.push(Edge {
                        source: method_id.to_string(),
                        target: id,
                        relation: "references".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: Some(context.to_string()),
                    });
                }
                // Emit generic args from type_arguments child
                if emit_generics {
                    let args = (0..type_node.child_count())
                        .find_map(|i| type_node.child(i).filter(|c| c.kind() == "type_arguments"));
                    if let Some(args) = args {
                        for i in 0..args.child_count() {
                            if let Some(arg) = args.child(i) {
                                if arg.kind() == "type_identifier" {
                                    let name = jv_text(source, &arg);
                                    let id = make_id(&[&name]);
                                    add_node_if_missing(
                                        fragment,
                                        jv_node(id.clone(), name, "code", path),
                                    );
                                    fragment.edges.push(Edge {
                                        source: method_id.to_string(),
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
            }
            _ => {}
        }
    }
}

impl LanguageExtractor for TsJavaExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["java"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
