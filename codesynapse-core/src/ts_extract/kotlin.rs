use super::{add_contains_edge, add_node_if_missing, make_file_node};
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct TsKotlinExtractor;

fn kt_node(id: String, label: String, file_type: &str, path: &Path) -> Node {
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

/// Parse Kotlin delegation specifiers string (after the `:`) into (name, is_class) pairs.
/// "BaseProcessor(), Loggable" → [("BaseProcessor", true), ("Loggable", false)]
fn parse_delegation_specs(specs_str: &str) -> Vec<(String, bool)> {
    let mut result = Vec::new();
    let mut depth = 0i32;
    let mut angle = 0i32;
    let mut current = String::new();

    for ch in specs_str.chars() {
        match ch {
            '<' => {
                angle += 1;
                current.push(ch);
            }
            '>' => {
                angle -= 1;
                if angle < 0 {
                    angle = 0;
                }
                current.push(ch);
            }
            '(' if angle == 0 => {
                depth += 1;
                current.push(ch);
            }
            ')' if angle == 0 => {
                depth -= 1;
                if depth < 0 {
                    depth = 0;
                }
                current.push(ch);
            }
            ',' if depth == 0 && angle == 0 => {
                let spec = current.trim().to_string();
                if !spec.is_empty() {
                    let is_class = spec.contains('(');
                    let name = spec
                        .split('<')
                        .next()
                        .unwrap_or(&spec)
                        .split('(')
                        .next()
                        .unwrap_or(&spec)
                        .trim()
                        .to_string();
                    if !name.is_empty() {
                        result.push((name, is_class));
                    }
                }
                current = String::new();
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.trim().is_empty() {
        let spec = current.trim().to_string();
        let is_class = spec.contains('(');
        let name = spec
            .split('<')
            .next()
            .unwrap_or(&spec)
            .split('(')
            .next()
            .unwrap_or(&spec)
            .trim()
            .to_string();
        if !name.is_empty() {
            result.push((name, is_class));
        }
    }
    result
}

/// Extract a Kotlin type from a type annotation string like "Result<DataProcessor>".
/// Returns (bare_type, vec_of_generic_args)
fn parse_kotlin_type(type_str: &str) -> (String, Vec<String>) {
    let trimmed = type_str.trim().trim_end_matches('?');
    if let Some(lt) = trimmed.find('<') {
        let base = trimmed[..lt].trim().to_string();
        let args_str = trimmed[lt + 1..].trim_end_matches('>');
        let args: Vec<String> = args_str
            .split(',')
            .map(|s| s.trim().trim_end_matches('?').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        (base, args)
    } else {
        (trimmed.to_string(), vec![])
    }
}

impl TsKotlinExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");
        let stem = file_id.clone();

        let mut current_class: Option<String> = None;

        for line in content.lines() {
            let trimmed = line.trim();

            // import header
            if let Some(rest) = trimmed.strip_prefix("import ") {
                let full = rest.split_whitespace().next().unwrap_or("").trim();
                let leaf = full.split('.').next_back().unwrap_or(full).to_string();
                if !leaf.is_empty() {
                    let id = make_id(&[&leaf]);
                    add_node_if_missing(&mut fragment, kt_node(id.clone(), leaf, "module", path));
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
                continue;
            }

            // class / interface / object / data class declaration
            let is_class_line = {
                let mut kw = None;
                for prefix in &[
                    "class ",
                    "data class ",
                    "open class ",
                    "abstract class ",
                    "sealed class ",
                    "interface ",
                    "object ",
                    "enum class ",
                ] {
                    if let Some(rest) = trimmed.strip_prefix(prefix) {
                        kw = Some((*prefix, rest));
                        break;
                    }
                }
                kw
            };

            if let Some((kw, rest)) = is_class_line {
                // Extract name (up to ':', '(', '<', '{', ' ')
                let class_name = rest
                    .split([':', '(', '<', '{'])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();

                let class_file_type = match kw {
                    "interface " => "interface",
                    "enum class " => "enum",
                    _ => "class",
                };

                if !class_name.is_empty() {
                    let class_id = make_id(&[&stem, &class_name]);
                    add_node_if_missing(
                        &mut fragment,
                        kt_node(class_id.clone(), class_name.clone(), class_file_type, path),
                    );
                    add_contains_edge(&mut fragment, &file_id, class_id.clone(), path);
                    current_class = Some(class_id.clone());

                    // Parse delegation specifiers (after ':')
                    if let Some(colon_pos) = trimmed.find(':') {
                        let after_colon = &trimmed[colon_pos + 1..];
                        // Strip trailing '{', '{'
                        let specs_str = after_colon.split('{').next().unwrap_or(after_colon).trim();

                        for (base_name, is_class_base) in parse_delegation_specs(specs_str) {
                            let base_id = make_id(&[&stem, &base_name]);
                            add_node_if_missing(
                                &mut fragment,
                                kt_node(base_id.clone(), base_name, "code", path),
                            );
                            let relation = if is_class_base {
                                "inherits"
                            } else {
                                "implements"
                            };
                            fragment.edges.push(Edge {
                                source: class_id.clone(),
                                target: base_id,
                                relation: relation.to_string(),
                                confidence: "EXTRACTED".to_string(),
                                source_file: Some(path.to_string_lossy().to_string()),
                                weight: 1.0,
                                context: None,
                            });
                        }
                    }
                }
                continue;
            }

            // fun declaration inside or outside class
            if let Some(rest) = trimmed.strip_prefix("fun ").or_else(|| {
                // modifiers before fun
                let stripped = trimmed
                    .trim_start_matches("override ")
                    .trim_start_matches("private ")
                    .trim_start_matches("public ")
                    .trim_start_matches("protected ")
                    .trim_start_matches("internal ")
                    .trim_start_matches("suspend ")
                    .trim_start_matches("inline ")
                    .trim_start_matches("abstract ");
                if stripped.starts_with("fun ") {
                    stripped.strip_prefix("fun ")
                } else {
                    None
                }
            }) {
                // Extract function name and type info
                let func_name = rest
                    .split(['(', '<', ':', ' '])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();

                if func_name.is_empty() {
                    continue;
                }

                let func_id = make_id(&[&stem, &func_name, "()"]);
                let label = format!(".{}()", func_name);
                let fn_file_type = if current_class.is_some() {
                    "method"
                } else {
                    "function"
                };
                add_node_if_missing(
                    &mut fragment,
                    kt_node(func_id.clone(), label, fn_file_type, path),
                );

                let owner = current_class.as_deref().unwrap_or(&file_id);
                fragment.edges.push(Edge {
                    source: owner.to_string(),
                    target: func_id.clone(),
                    relation: "method".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: func_id.clone(),
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });

                // Extract return type (after last `)`): `): Type`
                if let Some(paren_end) = rest.rfind(')') {
                    let after_paren = rest[paren_end + 1..].trim();
                    if let Some(ret_str) = after_paren.strip_prefix(':') {
                        let ret_type_str = ret_str
                            .split('{')
                            .next()
                            .unwrap_or(ret_str)
                            .split('=')
                            .next()
                            .unwrap_or(ret_str)
                            .trim();
                        let (base_type, generic_args) = parse_kotlin_type(ret_type_str);
                        if !base_type.is_empty() && base_type != "Unit" {
                            let id = make_id(&[&base_type]);
                            add_node_if_missing(
                                &mut fragment,
                                kt_node(id.clone(), base_type, "code", path),
                            );
                            fragment.edges.push(Edge {
                                source: func_id.clone(),
                                target: id,
                                relation: "references".to_string(),
                                confidence: "EXTRACTED".to_string(),
                                source_file: Some(path.to_string_lossy().to_string()),
                                weight: 1.0,
                                context: Some("return_type".to_string()),
                            });
                        }
                        for arg in generic_args {
                            if !arg.is_empty() {
                                let id = make_id(&[&arg]);
                                add_node_if_missing(
                                    &mut fragment,
                                    kt_node(id.clone(), arg, "code", path),
                                );
                                fragment.edges.push(Edge {
                                    source: func_id.clone(),
                                    target: id,
                                    relation: "references".to_string(),
                                    confidence: "EXTRACTED".to_string(),
                                    source_file: Some(path.to_string_lossy().to_string()),
                                    weight: 1.0,
                                    context: Some("generic_arg".to_string()),
                                });
                            }
                        }
                    }
                }

                // Extract parameter types from `(param: Type, ...)`
                if let Some(paren_start) = rest.find('(') {
                    let paren_end = rest.rfind(')').unwrap_or(rest.len());
                    if paren_end > paren_start {
                        let params_str = &rest[paren_start + 1..paren_end];
                        for param in params_str.split(',') {
                            let param = param.trim();
                            if let Some(colon_pos) = param.find(':') {
                                let type_str = param[colon_pos + 1..].trim();
                                let (base_type, _generic_args) = parse_kotlin_type(type_str);
                                if !base_type.is_empty()
                                    && !matches!(
                                        base_type.as_str(),
                                        "String"
                                            | "Int"
                                            | "Boolean"
                                            | "Long"
                                            | "Double"
                                            | "Float"
                                            | "Any"
                                    )
                                {
                                    let id = make_id(&[&base_type]);
                                    add_node_if_missing(
                                        &mut fragment,
                                        kt_node(id.clone(), base_type.clone(), "code", path),
                                    );
                                    fragment.edges.push(Edge {
                                        source: func_id.clone(),
                                        target: id,
                                        relation: "references".to_string(),
                                        confidence: "EXTRACTED".to_string(),
                                        source_file: Some(path.to_string_lossy().to_string()),
                                        weight: 1.0,
                                        context: Some("parameter_type".to_string()),
                                    });
                                }
                            }
                        }
                    }
                }
                continue;
            }

            // Property/field declaration: `var name: Type` or `val name: Type`
            if let Some(rest) = trimmed
                .strip_prefix("var ")
                .or_else(|| trimmed.strip_prefix("val "))
            {
                if let Some(owner) = &current_class {
                    if let Some(colon_pos) = rest.find(':') {
                        let type_str = rest[colon_pos + 1..].split('=').next().unwrap_or("").trim();
                        let (base_type, _) = parse_kotlin_type(type_str);
                        if !base_type.is_empty()
                            && !matches!(
                                base_type.as_str(),
                                "String" | "Int" | "Boolean" | "Long" | "Double" | "Float"
                            )
                        {
                            let id = make_id(&[&base_type]);
                            add_node_if_missing(
                                &mut fragment,
                                kt_node(id.clone(), base_type, "code", path),
                            );
                            fragment.edges.push(Edge {
                                source: owner.clone(),
                                target: id,
                                relation: "references".to_string(),
                                confidence: "EXTRACTED".to_string(),
                                source_file: Some(path.to_string_lossy().to_string()),
                                weight: 1.0,
                                context: Some("field".to_string()),
                            });
                        }
                    }
                }
                continue;
            }

            // Track closing of class body (simple heuristic: line with just "}")
            if trimmed == "}" {
                // Don't pop current_class on every '}' - keep it for the whole file
                // since Kotlin functions can be nested inside class body
            }
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for TsKotlinExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["kt", "kts"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
