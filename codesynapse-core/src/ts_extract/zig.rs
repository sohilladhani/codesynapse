use super::{add_contains_edge, add_node_if_missing, make_file_node, node_text, run_query_named};
use crate::error::{CodeSynapseError, Result};
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Node as TsNode;

fn contains_struct_decl(node: TsNode) -> bool {
    if node.kind() == "struct_declaration" {
        return true;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if contains_struct_decl(child) {
            return true;
        }
    }
    false
}

fn tree_walk_structs(
    node: TsNode,
    source: &[u8],
    file_id: &str,
    fragment: &mut ExtractionFragment,
    path: &Path,
) {
    if node.kind() == "variable_declaration" {
        let mut name = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                if let Ok(text) = node_text(source, &child) {
                    name = Some(text.to_string());
                }
            }
        }
        if let Some(n) = name {
            if contains_struct_decl(node) {
                let id = make_id(&[file_id, &n]);
                fragment.nodes.push(Node {
                    id: id.clone(),
                    label: n,
                    file_type: "struct".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
                add_contains_edge(fragment, &file_id.to_string(), id, path);
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        tree_walk_structs(child, source, file_id, fragment, path);
    }
}

pub struct TsZigExtractor;

impl TsZigExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_zig::LANGUAGE.into();

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| CodeSynapseError::Parse(format!("failed to set zig language: {}", e)))?;
        if let Some(tree) = parser.parse(source, None) {
            let root = tree.root_node();
            tree_walk_structs(root, source, &file_id, &mut fragment, path);
        }

        let func_query = r#"
            (function_declaration
                name: (identifier) @func.name
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, func_query) {
            for name in captures.remove("func.name").unwrap_or_default() {
                let label = format!("{}()", name);
                let id = make_id(&[&file_id, &name]);
                fragment.nodes.push(Node {
                    id: id.clone(),
                    label,
                    file_type: "function".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
                add_contains_edge(&mut fragment, &file_id, id, path);
            }
        }

        let import_query = r#"
            ((builtin_function
                (builtin_identifier) @builtin.name
                (arguments (string) @import.path))
             (#eq? @builtin.name "@import"))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, import_query) {
            for path_str in captures.remove("import.path").unwrap_or_default() {
                let module = path_str.trim_matches('"').trim();
                let mod_id = make_id(&[&file_id, module]);
                let mod_node = Node {
                    id: mod_id.clone(),
                    label: module.to_string(),
                    file_type: "module".to_string(),
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
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for TsZigExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["zig"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
