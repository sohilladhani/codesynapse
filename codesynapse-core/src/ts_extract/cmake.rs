use super::{add_contains_edge, add_node_if_missing, make_file_node, run_query_named};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsCmakeExtractor;

impl TsCmakeExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang = tree_sitter_cmake::LANGUAGE.into();

        let func_query = r#"
            (function_command
                (argument_list
                    (argument
                        (unquoted_argument) @func.name)))
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

        let macro_query = r#"
            (macro_command
                (argument_list
                    (argument
                        (unquoted_argument) @macro.name)))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, macro_query) {
            for name in captures.remove("macro.name").unwrap_or_default() {
                let label = format!("{}()", name);
                let id = make_id(&[&file_id, &name]);
                fragment.nodes.push(Node {
                    id: id.clone(),
                    label,
                    file_type: "macro".to_string(),
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

        let include_query = r#"
            ((normal_command
                (identifier) @cmd.name
                (argument_list
                    (argument
                        (quoted_argument) @include.path)))
             (#eq? @cmd.name "include"))
        "#;
        if let Ok(mut captures) = run_query_named(source, &lang, include_query) {
            for path_str in captures.remove("include.path").unwrap_or_default() {
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

impl LanguageExtractor for TsCmakeExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["cmake", "CMakeLists.txt"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
