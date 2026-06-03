use super::{
    add_contains_edge, add_node_if_missing, make_file_node, run_query_matches_ranged,
    run_query_named, run_tree_walk_docstrings,
};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsJavaScriptExtractor;

impl TsJavaScriptExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_javascript::LANGUAGE.into();
        let content = std::str::from_utf8(source).unwrap_or("");

        let docstrings = run_tree_walk_docstrings(source, &lang, &["class_declaration"]);

        // Class declarations with extends
        let class_query = r#"
            (class_declaration
                name: (identifier) @class.name
                (class_heritage
                    (identifier) @class.base
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
                let source_location = ranges
                    .get("class.node")
                    .map(|(start, end)| format!("{}:{}", start, end));
                let class_node = Node {
                    id: class_id.clone(),
                    label: name.to_string(),
                    file_type: "class".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring: docstrings.get(name.as_str()).cloned(),
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

        // Named function declarations
        let fn_query = r#"
            (function_declaration
                name: (identifier) @func.name
            ) @func.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, fn_query) {
            for (m, ranges) in &matches {
                let name = match m.get("func.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let fn_id = make_id(&[&file_id, &name, "()"]);
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
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, fn_node);
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        // Class method definitions
        let method_query = r#"
            (method_definition
                name: (property_identifier) @method.name
            ) @method.node
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, method_query) {
            for (m, ranges) in &matches {
                let name = match m.get("method.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let method_id = make_id(&[&file_id, &name, "()"]);
                let source_location = ranges
                    .get("method.node")
                    .map(|(start, end)| format!("{}:{}", start, end));
                let method_node = Node {
                    id: method_id.clone(),
                    label: format!("{}()", name),
                    file_type: "method".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, method_node);
                add_contains_edge(&mut fragment, &file_id, method_id, path);
            }
        }

        // Arrow functions and function expressions assigned to variables
        let arrow_query = r#"
            (variable_declarator
                name: (identifier) @func.name
                value: [
                    (arrow_function)
                    (function_expression)
                ] @func.node
            )
        "#;
        if let Ok(matches) = run_query_matches_ranged(source, &lang, arrow_query) {
            for (m, ranges) in &matches {
                let name = match m.get("func.name") {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let fn_id = make_id(&[&file_id, &name, "()"]);
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
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, fn_node);
                add_contains_edge(&mut fragment, &file_id, fn_id, path);
            }
        }

        // ESM imports
        // IMPORTANT: query children must be in tree-sibling order.
        // `import_clause` (child 1) comes BEFORE `string` (child 3),
        // so `(import_clause ...)` must precede `source: (string)`.
        let import_query = r#"
            (import_statement
                (import_clause
                    (named_imports
                        (import_specifier
                            (identifier) @import.name
                        )
                    )?
                )?
                source: (string) @import.source
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, import_query) {
            let sources = captures.remove("import.source").unwrap_or_default();
            let import_names = captures.remove("import.name").unwrap_or_default();

            let mut source_modules: Vec<String> = Vec::new();
            for s in &sources {
                let s = s.trim().trim_matches('"').trim_matches('\'');
                source_modules.push(s.to_string());
            }

            let mut all_import_names: Vec<String> = Vec::new();
            for name in &import_names {
                all_import_names.push(name.to_string());
            }

            for mod_name in source_modules.iter() {
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
                    relation: "imports_from".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }

            for name in &all_import_names {
                let spec_id = make_id(&[&file_id, name]);
                let spec_node = Node {
                    id: spec_id.clone(),
                    label: name.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                };
                add_node_if_missing(&mut fragment, spec_node);
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: spec_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        // Dynamic imports and require - fallback to line-based for CJS patterns
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.contains("import(") {
                let start = trimmed.find("import(");
                if let Some(start) = start {
                    let rest = &trimmed[start + 7..];
                    let source_str = rest
                        .split(')')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .trim_matches('\'')
                        .trim_matches('"');
                    if !source_str.is_empty() {
                        let mod_id = make_id(&[&file_id, source_str]);
                        let mod_node = Node {
                            id: mod_id.clone(),
                            label: source_str.to_string(),
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
                            relation: "imports_from".to_string(),
                            confidence: "EXTRACTED".to_string(),
                            source_file: Some(path.to_string_lossy().to_string()),
                            weight: 1.0,
                            context: None,
                        });
                    }
                }
            }
            if trimmed.contains("require(") {
                let start = trimmed.find("require(");
                if let Some(start) = start {
                    let rest = &trimmed[start + 8..];
                    let source_str = rest
                        .split(')')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .trim_matches('\'')
                        .trim_matches('"');
                    if !source_str.is_empty() {
                        let mod_id = make_id(&[&file_id, source_str]);
                        let mod_node = Node {
                            id: mod_id.clone(),
                            label: source_str.to_string(),
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
                            relation: "imports_from".to_string(),
                            confidence: "EXTRACTED".to_string(),
                            source_file: Some(path.to_string_lossy().to_string()),
                            weight: 1.0,
                            context: None,
                        });
                    }
                }
            }
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for TsJavaScriptExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["js", "jsx", "mjs", "cjs"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
