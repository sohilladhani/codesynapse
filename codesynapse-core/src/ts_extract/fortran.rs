use super::make_file_node;
use crate::error::{CodeSynapseError, Result};
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node as TsNode, Parser};

pub struct FortranExtractor;

fn ft_text(source: &[u8], node: &TsNode<'_>) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

fn ft_node(id: String, label: String, file_type: &str, path: &Path) -> Node {
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

fn ft_edge(fragment: &mut ExtractionFragment, src: &str, tgt: &str, rel: &str, path: &Path) {
    fragment.edges.push(Edge {
        source: src.to_string(),
        target: tgt.to_string(),
        relation: rel.to_string(),
        confidence: "EXTRACTED".to_string(),
        source_file: Some(path.to_string_lossy().to_string()),
        weight: 1.0,
        context: None,
    });
}

fn add_node_if_missing(fragment: &mut ExtractionFragment, node: Node) {
    if !fragment.nodes.iter().any(|n| n.id == node.id) {
        fragment.nodes.push(node);
    }
}

/// Extract the name from a Fortran statement node (program_statement,
/// module_statement, subroutine_statement, function_statement, or use_statement).
/// Fortran is case-insensitive — returns lowercase.
fn fortran_name(source: &[u8], stmt: &TsNode<'_>) -> Option<String> {
    for i in 0..stmt.child_count() {
        if let Some(child) = stmt.child(i) {
            match child.kind() {
                "name" | "identifier" | "module_name" => {
                    let t = ft_text(source, &child);
                    if !t.is_empty() {
                        return Some(t);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn walk_ft<'t>(
    node: TsNode<'t>,
    source: &[u8],
    file_id: &str,
    stem: &str,
    path: &Path,
    fragment: &mut ExtractionFragment,
    scope_nid: String,
) {
    match node.kind() {
        "program" => {
            let stmt = (0..node.child_count())
                .filter_map(|i| node.child(i))
                .find(|c| c.kind() == "program_statement");
            let name = stmt.and_then(|s| fortran_name(source, &s));
            if let Some(name) = name {
                let nid = make_id(&[stem, &name]);
                fragment
                    .nodes
                    .push(ft_node(nid.clone(), name, "code", path));
                ft_edge(fragment, file_id, &nid, "defines", path);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        walk_ft(child, source, file_id, stem, path, fragment, nid.clone());
                    }
                }
            }
        }
        "module" => {
            let stmt = (0..node.child_count())
                .filter_map(|i| node.child(i))
                .find(|c| c.kind() == "module_statement");
            let name = stmt.and_then(|s| fortran_name(source, &s));
            if let Some(name) = name {
                let nid = make_id(&[stem, &name]);
                fragment
                    .nodes
                    .push(ft_node(nid.clone(), name, "module", path));
                ft_edge(fragment, file_id, &nid, "defines", path);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        walk_ft(child, source, file_id, stem, path, fragment, nid.clone());
                    }
                }
            }
        }
        "internal_procedures" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    walk_ft(
                        child,
                        source,
                        file_id,
                        stem,
                        path,
                        fragment,
                        scope_nid.clone(),
                    );
                }
            }
        }
        "subroutine" => {
            let stmt = (0..node.child_count())
                .filter_map(|i| node.child(i))
                .find(|c| c.kind() == "subroutine_statement");
            let name = stmt.and_then(|s| fortran_name(source, &s));
            if let Some(name) = name {
                let nid = make_id(&[stem, &name]);
                fragment.nodes.push(ft_node(
                    nid.clone(),
                    format!("{}()", name),
                    "function",
                    path,
                ));
                ft_edge(fragment, &scope_nid, &nid, "defines", path);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        walk_ft(child, source, file_id, stem, path, fragment, nid.clone());
                    }
                }
            }
        }
        "function" => {
            let stmt = (0..node.child_count())
                .filter_map(|i| node.child(i))
                .find(|c| c.kind() == "function_statement");
            let name = stmt.and_then(|s| fortran_name(source, &s));
            if let Some(name) = name {
                let nid = make_id(&[stem, &name]);
                fragment.nodes.push(ft_node(
                    nid.clone(),
                    format!("{}()", name),
                    "function",
                    path,
                ));
                ft_edge(fragment, &scope_nid, &nid, "defines", path);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        walk_ft(child, source, file_id, stem, path, fragment, nid.clone());
                    }
                }
            }
        }
        "use_statement" => {
            let name = fortran_name(source, &node);
            if let Some(mod_name) = name {
                let imp_nid = make_id(&[&mod_name]);
                let imp_node = ft_node(imp_nid.clone(), mod_name, "module", path);
                add_node_if_missing(fragment, imp_node);
                fragment.edges.push(Edge {
                    source: scope_nid.to_string(),
                    target: imp_nid,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: Some("use".to_string()),
                });
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    walk_ft(
                        child,
                        source,
                        file_id,
                        stem,
                        path,
                        fragment,
                        scope_nid.clone(),
                    );
                }
            }
        }
    }
}

impl FortranExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let stem = file_id.clone();
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let lang: tree_sitter::Language = tree_sitter_fortran::LANGUAGE.into();
        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| CodeSynapseError::Parse(format!("fortran set_language: {e}")))?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| CodeSynapseError::Parse("fortran parse failed".to_string()))?;

        let scope = file_id.clone();
        walk_ft(
            tree.root_node(),
            source,
            &file_id,
            &stem,
            path,
            &mut fragment,
            scope,
        );

        Ok(fragment)
    }
}

impl LanguageExtractor for FortranExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec![
            "f", "f90", "f95", "f03", "f08", "F", "F90", "F95", "F03", "F08",
        ]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
