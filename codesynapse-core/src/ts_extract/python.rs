use super::{
    add_contains_edge, add_node_if_missing, make_file_node, run_query_matches_ranged,
    run_query_named, strip_docstring,
};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

/// Tree-sitter based Python extractor
pub struct TsPythonExtractor;

impl TsPythonExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_python::LANGUAGE.into();

        // Class definitions with bases and docstrings
        let class_query = r#"
            (class_definition
                name: (identifier) @class.name
                superclasses: (argument_list
                    (identifier) @class.base
                )?
                body: (block .
                    (expression_statement (string) @class.docstring)
                )?
            ) @class.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, class_query) {
            for (m, ranges) in &matches {
                let name = match m.get("class.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let class_id = make_id(&[&file_id, &name]);
                let docstring = m.get("class.docstring").and_then(|s| strip_docstring(s));
                let source_location = ranges
                    .get("class.node")
                    .map(|(start, end)| format!("{}:{}", start, end));
                let class_node = Node {
                    id: class_id.clone(),
                    label: name.clone(),
                    file_type: "class".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, class_node);
                add_contains_edge(&mut fragment, &file_id, class_id.clone(), path);

                if let Some(base_name) = m.get("class.base") {
                    let base_id = make_id(&[base_name]);
                    let base_node_ = Node {
                        id: base_id.clone(),
                        label: base_name.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    };
                    add_node_if_missing(&mut fragment, base_node_);
                    fragment.edges.push(Edge {
                        source: class_id.clone(),
                        target: base_id,
                        relation: "inherits".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                }
            }
        }

        // Methods inside class bodies — must run before the general fn_query so that
        // add_node_if_missing skips them when the general query runs.
        let method_query = r#"
            (class_definition
                body: (block
                    (function_definition
                        name: (identifier) @method.name
                        body: (block .
                            (expression_statement (string) @method.docstring)
                        )?
                    ) @method.node
                )
            )
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, method_query) {
            for (m, ranges) in &matches {
                let name = match m.get("method.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let fn_id = make_id(&[&file_id, &name, "()"]);
                let docstring = m.get("method.docstring").and_then(|s| strip_docstring(s));
                let source_location = ranges
                    .get("method.node")
                    .map(|(start, end)| format!("{}:{}", start, end));
                let method_node = Node {
                    id: fn_id.clone(),
                    label: format!("{}()", name),
                    file_type: "method".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, method_node);
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        // Module-level function definitions with docstrings.
        // Methods already added above are skipped by add_node_if_missing.
        let fn_query = r#"
            (function_definition
                name: (identifier) @func.name
                body: (block .
                    (expression_statement (string) @func.docstring)
                )?
            ) @func.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, fn_query) {
            for (m, ranges) in &matches {
                let name = match m.get("func.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let fn_id = make_id(&[&file_id, &name, "()"]);
                let docstring = m.get("func.docstring").and_then(|s| strip_docstring(s));
                let source_location = ranges
                    .get("func.node")
                    .map(|(start, end)| format!("{}:{}", start, end));
                let fn_node = Node {
                    id: fn_id.clone(),
                    label: format!("{}()", name),
                    file_type: "function".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, fn_node);
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        // Import: import x, import x as y
        let import_query = r#"
            (import_statement
                name: (dotted_name) @import.name
                alias: (identifier)? @import.alias
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, import_query) {
            let names = captures.remove("import.name").unwrap_or_default();
            let aliases = captures.remove("import.alias").unwrap_or_default();
            for name in &names {
                let parts: Vec<&str> = name.split('.').collect();
                let simple = parts.last().copied().unwrap_or(name.as_str());
                let mod_id = make_id(&[simple]);
                let mod_node = Node {
                    id: mod_id.clone(),
                    label: simple.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, mod_node);
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: mod_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
            for alias in &aliases {
                let alias_id = make_id(&[&file_id, alias, "alias"]);
                let alias_node = Node {
                    id: alias_id.clone(),
                    label: alias.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, alias_node);
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: alias_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        // From-import: from x import y, from x import y as z
        let from_query = r#"
            (import_from_statement
                module_name: [
                    (dotted_name)
                    (relative_import)
                ] @from.module
                name: (dotted_name) @from.name
                alias: (identifier)? @from.alias
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, from_query) {
            let modules = captures.remove("from.module").unwrap_or_default();
            let names = captures.remove("from.name").unwrap_or_default();
            let from_aliases = captures.remove("from.alias").unwrap_or_default();

            for mod_name in &modules {
                let mod_id = make_id(&[&file_id, mod_name]);
                let mod_node = Node {
                    id: mod_id.clone(),
                    label: mod_name.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, mod_node);
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: mod_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
            for name in &names {
                let import_id = make_id(&[&file_id, name]);
                let import_node = Node {
                    id: import_id.clone(),
                    label: name.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, import_node);
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: import_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
            for alias in &from_aliases {
                let alias_id = make_id(&[&file_id, alias, "alias"]);
                let alias_node = Node {
                    id: alias_id.clone(),
                    label: alias.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, alias_node);
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: alias_id,
                    relation: "imports".to_string(),
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

/// Tree-sitter based JavaScript/TypeScript extractor
impl LanguageExtractor for TsPythonExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["py"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
