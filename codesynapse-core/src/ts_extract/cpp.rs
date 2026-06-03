use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_named};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsCppExtractor;
impl TsCppExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };
        let lang = tree_sitter_cpp::LANGUAGE.into();

        // Class declarations
        let class_query = r#"
            (class_specifier
                name: (type_identifier) @class.name
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, class_query) {
            let names = captures.remove("class.name").unwrap_or_default();
            for name in &names {
                let class_id = make_id(&[&file_id, name]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: class_id.clone(),
                        label: name.to_string(),
                        file_type: "class".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    },
                );
                add_contains_edge(&mut fragment, &file_id, class_id, path);
            }
        }

        // Struct declarations
        let struct_query = r#"
            (struct_specifier
                name: (type_identifier) @struct.name
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, struct_query) {
            let names = captures.remove("struct.name").unwrap_or_default();
            for name in &names {
                let id = make_id(&[&file_id, name]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: id.clone(),
                        label: name.to_string(),
                        file_type: "struct".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    },
                );
                add_contains_edge(&mut fragment, &file_id, id, path);
            }
        }

        // Class inheritance: base_clause inside class_specifier
        let _inherits_query = r#"
            (class_specifier
                name: (type_identifier) @child.name
                (base_class_clause
                    (type_identifier) @base.name
                )
            )
        "#;
        // Since run_query_named doesn't associate captures within a match,
        // we use a simple heuristic: class/struct with a base clause inherits from that base.
        // We need a different approach - use the walker pattern.
        Self::extract_inheritance(source, path, &file_id, &lang, &mut fragment);

        // Top-level free functions (identifier declarator = not a member)
        let fn_top_query = r#"
            (function_definition
                declarator: (function_declarator
                    declarator: (identifier) @fn.name
                )
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, fn_top_query) {
            for name in captures.remove("fn.name").unwrap_or_default() {
                let fn_id = make_id(&[&file_id, &name, "()"]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: fn_id.clone(),
                        label: format!("{}()", name),
                        file_type: "function".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    },
                );
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        // Member function definitions / declarations (field_identifier = inside a class body)
        let fn_member_query = r#"
            [
                (function_definition
                    declarator: (function_declarator
                        declarator: (field_identifier) @method.name
                    )
                )
                (field_declaration
                    declarator: (function_declarator
                        declarator: (field_identifier) @method.name
                    )
                )
            ]
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, fn_member_query) {
            for name in captures.remove("method.name").unwrap_or_default() {
                let fn_id = make_id(&[&file_id, &name, "()"]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: fn_id.clone(),
                        label: format!("{}()", name),
                        file_type: "method".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    },
                );
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        // Preprocessor includes with context="import"
        let include_query = r#"
            [
                (preproc_include path: (string_literal) @include.path)
                (preproc_include path: (system_lib_string) @include.path)
            ]
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, include_query) {
            for p in captures.remove("include.path").unwrap_or_default() {
                let inc = p
                    .trim()
                    .trim_matches('"')
                    .trim_matches('<')
                    .trim_matches('>');
                let inc_id = make_id(&[&file_id, inc]);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: inc_id.clone(),
                        label: inc.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    },
                );
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: inc_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: Some("import".to_string()),
                });
            }
        }

        Ok(fragment)
    }

    fn extract_inheritance(
        source: &[u8],
        path: &Path,
        file_id: &str,
        lang: &tree_sitter::Language,
        fragment: &mut ExtractionFragment,
    ) {
        use tree_sitter::{Node as TsNode, Parser};

        let mut parser = Parser::new();
        if parser.set_language(lang).is_err() {
            return;
        }
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return,
        };

        fn text(source: &[u8], node: &TsNode<'_>) -> String {
            std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
                .unwrap_or("")
                .trim()
                .to_string()
        }

        fn walk(
            node: TsNode<'_>,
            source: &[u8],
            path: &Path,
            file_id: &str,
            fragment: &mut ExtractionFragment,
        ) {
            let kind = node.kind();
            if kind == "class_specifier" || kind == "struct_specifier" {
                // Find name
                let name_node = node.child_by_field_name("name");
                if let Some(name_node) = name_node {
                    let class_name = text(source, &name_node);
                    let class_id = make_id(&[file_id, &class_name]);

                    // Find base_class_clause
                    for i in 0..node.child_count() {
                        if let Some(child) = node.child(i) {
                            if child.kind() == "base_class_clause" {
                                // Find type_identifier children (the base classes)
                                for j in 0..child.child_count() {
                                    if let Some(base) = child.child(j) {
                                        if base.kind() == "type_identifier" {
                                            let base_name = text(source, &base);
                                            let base_id = make_id(&[&base_name]);
                                            add_node_if_missing(
                                                fragment,
                                                Node {
                                                    id: base_id.clone(),
                                                    label: base_name,
                                                    file_type: "code".to_string(),
                                                    source_file: path.to_string_lossy().to_string(),
                                                    source_location: None,
                                                    community: None,
                                                    rationale: None,
                                                    docstring: None,
                                                    metadata: HashMap::new(),
                                                },
                                            );
                                            let already = fragment.edges.iter().any(|e| {
                                                e.relation == "inherits"
                                                    && e.source == class_id
                                                    && e.target == base_id
                                            });
                                            if !already {
                                                fragment.edges.push(Edge {
                                                    source: class_id.clone(),
                                                    target: base_id,
                                                    relation: "inherits".to_string(),
                                                    confidence: "EXTRACTED".to_string(),
                                                    source_file: Some(
                                                        path.to_string_lossy().to_string(),
                                                    ),
                                                    weight: 1.0,
                                                    context: None,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                            // Walk body
                            walk(child, source, path, file_id, fragment);
                        }
                    }
                }
                return;
            }

            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    walk(child, source, path, file_id, fragment);
                }
            }
        }

        walk(tree.root_node(), source, path, file_id, fragment);
    }
}

impl LanguageExtractor for TsCppExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["cpp", "cxx", "hpp", "hxx", "cc"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
