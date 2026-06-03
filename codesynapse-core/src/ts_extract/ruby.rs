use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_matches_ranged};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsRubyExtractor;

impl TsRubyExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_ruby::LANGUAGE.into();

        // Classes with source_location and superclass inherits edges
        let class_query = r#"
            (class
                name: (constant) @class.name
                superclass: (superclass (constant) @class.super)?
            ) @class.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, class_query) {
            for (texts, ranges) in &matches {
                let name = match texts.get("class.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let source_location = ranges
                    .get("class.node")
                    .map(|(s, e)| format!("{}:{}", s, e));
                let class_id = make_id(&[&file_id, &name]);
                fragment.nodes.push(Node {
                    id: class_id.clone(),
                    label: name.clone(),
                    file_type: "class".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("kind".to_string(), "class".to_string());
                        m
                    },
                });
                add_contains_edge(&mut fragment, &file_id, class_id.clone(), path);

                if let Some(super_name) = texts.get("class.super") {
                    let super_id = make_id(&[super_name]);
                    add_node_if_missing(
                        &mut fragment,
                        Node {
                            id: super_id.clone(),
                            label: super_name.clone(),
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
                        source: class_id,
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

        // Instance methods
        let method_query = r#"
            (method name: (identifier) @fn.name) @fn.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, method_query) {
            for (texts, ranges) in &matches {
                let name = match texts.get("fn.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let source_location = ranges.get("fn.node").map(|(s, e)| format!("{}:{}", s, e));
                let method_id = make_id(&[&file_id, &name]);
                let label = format!("{}()", name);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: method_id.clone(),
                        label,
                        file_type: "method".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: {
                            let mut m = HashMap::new();
                            m.insert("kind".to_string(), "method".to_string());
                            m
                        },
                    },
                );
                add_contains_edge(&mut fragment, &file_id, method_id, path);
            }
        }

        // Class (singleton) methods
        let singleton_query = r#"
            (singleton_method name: (identifier) @fn.name) @fn.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, singleton_query) {
            for (texts, ranges) in &matches {
                let name = match texts.get("fn.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let source_location = ranges.get("fn.node").map(|(s, e)| format!("{}:{}", s, e));
                let method_id = make_id(&[&file_id, "self", &name]);
                let label = format!("self.{}()", name);
                add_node_if_missing(
                    &mut fragment,
                    Node {
                        id: method_id.clone(),
                        label,
                        file_type: "method".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: {
                            let mut m = HashMap::new();
                            m.insert("kind".to_string(), "method".to_string());
                            m
                        },
                    },
                );
                add_contains_edge(&mut fragment, &file_id, method_id, path);
            }
        }

        // Class → instance method contains edges
        let class_method_query = r#"
            (class
                name: (constant) @class.name
                body: (body_statement
                    (method name: (identifier) @method.name)
                )
            )
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, class_method_query) {
            for (texts, _) in &matches {
                let class_name = match texts.get("class.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let method_name = match texts.get("method.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let class_id = make_id(&[&file_id, &class_name]);
                let method_id = make_id(&[&file_id, &method_name]);
                fragment.edges.push(Edge {
                    source: class_id,
                    target: method_id,
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        // Class → singleton method contains edges
        let class_singleton_query = r#"
            (class
                name: (constant) @class.name
                body: (body_statement
                    (singleton_method name: (identifier) @method.name)
                )
            )
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, class_singleton_query) {
            for (texts, _) in &matches {
                let class_name = match texts.get("class.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let method_name = match texts.get("method.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let class_id = make_id(&[&file_id, &class_name]);
                let method_id = make_id(&[&file_id, "self", &method_name]);
                fragment.edges.push(Edge {
                    source: class_id,
                    target: method_id,
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for TsRubyExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["rb"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
