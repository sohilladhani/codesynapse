use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_named};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsCExtractor;
impl TsCExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_c::LANGUAGE.into();

        // Function definitions
        let fn_query = r#"
            (function_definition
                declarator: (function_declarator
                    declarator: (identifier) @func.name
                )
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, fn_query) {
            let names = captures.remove("func.name").unwrap_or_default();
            for name in &names {
                let fn_id = make_id(&[&file_id, name, "()"]);
                fragment.nodes.push(Node {
                    id: fn_id.clone(),
                    label: format!("{}()", name),
                    file_type: "function".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
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
            let paths = captures.remove("include.path").unwrap_or_default();
            for p in &paths {
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

        // Call expressions with context="call"
        let call_query = r#"
            (call_expression
                function: (identifier) @call.name
            )
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, call_query) {
            for name in captures.remove("call.name").unwrap_or_default() {
                let callee_id = make_id(&[&file_id, &name, "()"]);
                // Only emit call edges to known functions in this file
                if fragment.nodes.iter().any(|n| n.id == callee_id) {
                    let already = fragment.edges.iter().any(|e| {
                        e.relation == "calls" && e.source == file_id && e.target == callee_id
                    });
                    if !already {
                        fragment.edges.push(Edge {
                            source: file_id.clone(),
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

        Ok(fragment)
    }
}

impl LanguageExtractor for TsCExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["c", "h"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
