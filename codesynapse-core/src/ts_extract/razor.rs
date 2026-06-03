use super::make_file_node;
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct RazorExtractor;

impl RazorExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");

        // @using directives → import edges
        let using_re = regex::Regex::new(r"(?m)^@using\s+([\w.]+)");
        if let Ok(r) = using_re {
            for cap in r.captures_iter(content) {
                let ns = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if !ns.is_empty() {
                    let id = make_id(&[ns]);
                    fragment.nodes.push(Node {
                        id: id.clone(),
                        label: ns.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: id,
                        relation: "imports".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: Some("import".to_string()),
                    });
                }
            }
        }

        // @inject directives
        let inject_re = regex::Regex::new(r"(?m)^@inject\s+(\w+)\s+\w+");
        if let Ok(r) = inject_re {
            for cap in r.captures_iter(content) {
                let service = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if !service.is_empty() {
                    let id = make_id(&[service]);
                    if !fragment.nodes.iter().any(|n| n.id == id) {
                        fragment.nodes.push(Node {
                            id: id.clone(),
                            label: service.to_string(),
                            file_type: "code".to_string(),
                            source_file: path.to_string_lossy().to_string(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        });
                    }
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: id,
                        relation: "imports".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: Some("import".to_string()),
                    });
                }
            }
        }

        // @inherits → inherits edge
        let inherits_re = regex::Regex::new(r"(?m)^@inherits\s+(\w+)");
        if let Ok(r) = inherits_re {
            for cap in r.captures_iter(content) {
                let base = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if !base.is_empty() {
                    let base_id = make_id(&[base]);
                    if !fragment.nodes.iter().any(|n| n.id == base_id) {
                        fragment.nodes.push(Node {
                            id: base_id.clone(),
                            label: base.to_string(),
                            file_type: "code".to_string(),
                            source_file: path.to_string_lossy().to_string(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        });
                    }
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
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

        // Component tags: <ComponentName ...> where name starts with uppercase
        let component_re = regex::Regex::new(r"<([A-Z][A-Za-z]*)\b[^/]*(?:/>|>)");
        if let Ok(r) = component_re {
            for cap in r.captures_iter(content) {
                let comp = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                // Skip standard HTML-like tags
                if comp.is_empty() {
                    continue;
                }
                let comp_id = make_id(&[comp]);
                if !fragment.nodes.iter().any(|n| n.id == comp_id) {
                    fragment.nodes.push(Node {
                        id: comp_id.clone(),
                        label: comp.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                }
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: comp_id,
                    relation: "calls".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        // @code { ... } block: extract methods
        // Simple regex for method signatures: (private|public|protected|async)? (void|Task|...) MethodName(
        if let Some(code_start) = content.find("@code") {
            let code_block = &content[code_start..];
            let method_re = regex::Regex::new(
                r"(?m)(?:(?:private|public|protected|override|async|virtual|static)\s+)*(?:\w+(?:<[^>]*>)?(?:\?)?)\s+(\w+)\s*\(",
            );
            if let Ok(r) = method_re {
                for cap in r.captures_iter(code_block) {
                    let method_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    if method_name.is_empty() || is_csharp_keyword(method_name) {
                        continue;
                    }
                    let id = make_id(&[&file_id, method_name, "()"]);
                    if !fragment.nodes.iter().any(|n| n.id == id) {
                        fragment.nodes.push(Node {
                            id: id.clone(),
                            label: format!("{}()", method_name),
                            file_type: "code".to_string(),
                            source_file: path.to_string_lossy().to_string(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        });
                        fragment.edges.push(Edge {
                            source: file_id.clone(),
                            target: id,
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

        Ok(fragment)
    }
}

fn is_csharp_keyword(s: &str) -> bool {
    matches!(
        s,
        "if" | "else"
            | "for"
            | "foreach"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "return"
            | "new"
            | "var"
            | "class"
            | "namespace"
            | "using"
            | "await"
            | "async"
            | "base"
            | "this"
            | "null"
            | "true"
            | "false"
            | "int"
            | "string"
            | "bool"
            | "void"
            | "List"
            | "Task"
            | "override"
            | "protected"
            | "private"
            | "public"
            | "static"
            | "virtual"
            | "abstract"
    )
}

impl LanguageExtractor for RazorExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["razor", "cshtml"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
