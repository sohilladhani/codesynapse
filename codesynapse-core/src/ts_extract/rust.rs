use super::{
    add_contains_edge, add_node_if_missing, make_file_node, run_query_matches_ranged,
    run_query_named,
};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsRustExtractor;

impl TsRustExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_rust::LANGUAGE.into();

        // Struct definitions with generics
        let struct_query = r#"
            (struct_item
                name: (type_identifier) @struct.name
                (type_parameters
                    (type_parameter
                        (type_identifier) @struct.generic
                    )
                )?
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, struct_query) {
            let names = captures.remove("struct.name").unwrap_or_default();
            let _generics = captures.remove("struct.generic").unwrap_or_default();

            for name in &names {
                let struct_id = make_id(&[&file_id, name]);
                fragment.nodes.push(Node {
                    id: struct_id.clone(),
                    label: name.to_string(),
                    file_type: "struct".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
                add_contains_edge(&mut fragment, &file_id, struct_id.clone(), path);

                // Find generic params for this struct
                let content = std::str::from_utf8(source);
                if let Ok(content) = content {
                    if let Some(line) = content
                        .lines()
                        .find(|l| l.contains(&format!("struct {}", name)))
                    {
                        if let Some(gen_start) = line.find('<') {
                            if let Some(gen_end) = line.find('>') {
                                let gen_str = &line[gen_start + 1..gen_end].trim();
                                for gen_name in gen_str.split(',').map(|s| s.trim()) {
                                    if !gen_name.is_empty() {
                                        let gen_id = make_id(&[&file_id, gen_name, "ty"]);
                                        if !fragment.nodes.iter().any(|n| n.id == gen_id) {
                                            fragment.nodes.push(Node {
                                                id: gen_id.clone(),
                                                label: gen_name.to_string(),
                                                file_type: "code".to_string(),
                                                source_file: path.to_string_lossy().to_string(),
                                                source_location: None,
                                                community: None,
                                                rationale: None,
                                                docstring: None,
                                                metadata: {
                                                    let mut m = HashMap::new();
                                                    m.insert(
                                                        "kind".to_string(),
                                                        "generic_param".to_string(),
                                                    );
                                                    m
                                                },
                                            });
                                        }
                                        fragment.edges.push(Edge {
                                            source: struct_id.clone(),
                                            target: gen_id,
                                            relation: "generic".to_string(),
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
                }
            }
        }

        // Use declarations
        let use_query = r#"
            (use_declaration
                argument: (scoped_identifier) @use.path
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, use_query) {
            let paths = captures.remove("use.path").unwrap_or_default();
            for p in &paths {
                let parts: Vec<&str> = p.split("::").collect();
                for (i, part) in parts.iter().enumerate() {
                    let seg_id = if i == 0 {
                        make_id(&[part])
                    } else {
                        let parent = make_id(parts[..i].as_ref());
                        make_id(&[&parent, part])
                    };
                    let seg_node = Node {
                        id: seg_id.clone(),
                        label: part.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    };
                    add_node_if_missing(&mut fragment, seg_node);
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: seg_id,
                        relation: "imports".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                }
            }
        }

        // Function items (top-level and impl methods) with source_location for BM25 body indexing
        let fn_query = r#"
            (function_item
                name: (identifier) @fn.name
            ) @fn.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, fn_query) {
            for (text_map, range_map) in &matches {
                let Some(name) = text_map.get("fn.name") else {
                    continue;
                };
                if name.is_empty() {
                    continue;
                }
                let label = format!("{}()", name);
                let fn_id = make_id(&[&file_id, &label]);
                let (start, end) = range_map.get("fn.node").copied().unwrap_or((0, 0));
                let source_location = if start < end {
                    Some(format!("{}:{}", start, end))
                } else {
                    None
                };
                let fn_node = Node {
                    id: fn_id.clone(),
                    label,
                    file_type: "function".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("kind".to_string(), "function".to_string());
                        m
                    },
                };
                add_node_if_missing(&mut fragment, fn_node);
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        // Impl method → struct contains edges for simple impl types: impl Foo { fn m() {} }
        let impl_simple_query = r#"
            (impl_item
                type: (type_identifier) @impl.type
                body: (declaration_list
                    (function_item
                        name: (identifier) @method.name
                    )
                )
            )
        "#;
        Self::add_impl_edges(
            &mut fragment,
            source,
            &lang,
            impl_simple_query,
            &file_id,
            path,
        );

        // Impl method → struct contains edges for generic impl types: impl<T> Foo<T> { fn m() {} }
        let impl_generic_query = r#"
            (impl_item
                type: (generic_type
                    type: (type_identifier) @impl.type
                )
                body: (declaration_list
                    (function_item
                        name: (identifier) @method.name
                    )
                )
            )
        "#;
        Self::add_impl_edges(
            &mut fragment,
            source,
            &lang,
            impl_generic_query,
            &file_id,
            path,
        );

        Ok(fragment)
    }

    fn add_impl_edges(
        fragment: &mut ExtractionFragment,
        source: &[u8],
        lang: &tree_sitter::Language,
        query_str: &str,
        file_id: &str,
        path: &Path,
    ) {
        let Ok(matches) = run_query_matches_ranged(source, lang, query_str) else {
            return;
        };
        for (text_map, _) in &matches {
            let Some(impl_type) = text_map.get("impl.type") else {
                continue;
            };
            let Some(method_name) = text_map.get("method.name") else {
                continue;
            };
            let impl_type_id = make_id(&[file_id, impl_type]);
            let method_label = format!("{}()", method_name);
            let method_id = make_id(&[file_id, &method_label]);
            if fragment.nodes.iter().any(|n| n.id == impl_type_id) {
                fragment.edges.push(Edge {
                    source: impl_type_id,
                    target: method_id,
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }
    }
}

/// Tree-sitter based Rust extractor
impl LanguageExtractor for TsRustExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["rs"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
